use super::{App, BackgroundTask, UnitProbeOutcome, UnitScanTaskResult};
use crate::compat;
use crate::constants::message;
use crate::num_ops::{digit_add, digit_remove, wrap_index};
use crate::state::{ScanState, StatusMessage, UnitField, UnitParams, UnitScanHit};

impl App {
    fn unit_mut(&mut self) -> Option<&mut UnitParams> {
        self.popup_as_mut()
    }

    pub fn unit_move(&mut self, down: bool) {
        if let Some(p) = self.unit_mut() {
            let n = p.fields().len() as u16;
            p.selected = wrap_index(p.selected, n, down);
        }
    }

    pub fn unit_switch_column(&mut self) {
        if let Some(p) = self.unit_mut() {
            p.switch_column();
        }
    }

    pub fn unit_toggle(&mut self, field: UnitField) {
        if let Some(p) = self.unit_mut() {
            match field {
                UnitField::Mode => p.stop_at_first = !p.stop_at_first,
                UnitField::Repr => p.ascii = !p.ascii,
                UnitField::Exceptions => p.show_exceptions = !p.show_exceptions,
                _ => {}
            }
        }
    }

    pub fn unit_digit(&mut self, field: UnitField, c: char) {
        if !c.is_ascii_digit() {
            return;
        }
        let digit = c as u8 - b'0';
        if let Some(p) = self.unit_mut() {
            match field {
                UnitField::Id => digit_add(&mut p.id, digit),
                UnitField::From => digit_add(&mut p.from, digit),
                UnitField::To => digit_add(&mut p.to, digit),
                UnitField::Mode
                | UnitField::Repr
                | UnitField::Exceptions
                | UnitField::Scan
                | UnitField::Hit(_) => {}
            }
        }
    }

    pub fn unit_backspace(&mut self, field: UnitField) {
        if let Some(p) = self.unit_mut() {
            match field {
                UnitField::Id => digit_remove(&mut p.id),
                UnitField::From => digit_remove(&mut p.from),
                UnitField::To => digit_remove(&mut p.to),
                UnitField::Mode
                | UnitField::Repr
                | UnitField::Exceptions
                | UnitField::Scan
                | UnitField::Hit(_) => {}
            }
        }
    }

    pub fn unit_scan_action(&mut self) {
        let Some(p) = self.popup_as::<UnitParams>() else {
            return;
        };
        if p.active() {
            if let Some(p) = self.unit_mut() {
                p.scan = ScanState::Stopped;
            }
            log::info!("Unit scan stopped");
            return;
        }

        if self.device.is_none() {
            if let Some(p) = self.unit_mut() {
                p.status = Some(StatusMessage::err(message::NO_DEVICE));
            }
            return;
        }
        if !self.free_background_slot() {
            if let Some(p) = self.unit_mut() {
                p.status = Some(StatusMessage::info(message::DEVICE_BUSY));
            }
            return;
        }

        let Some(p) = self.unit_mut() else {
            return;
        };
        if p.to < p.from {
            std::mem::swap(&mut p.from, &mut p.to);
        }
        p.hits.clear();
        p.scan = ScanState::Probing;
        p.current = p.from;
        p.status = None;
        log::info!(
            "Unit scan started | {}..={} | {:?} @ {} x{}{}",
            p.from,
            p.to,
            p.register_type,
            p.address,
            p.amount,
            if p.stop_at_first {
                " (stop at first hit)"
            } else {
                ""
            }
        );
        let first = p.from;
        self.spawn_unit_probe(first);
    }

    fn spawn_unit_probe(&mut self, unit_id: u8) {
        let Some(device) = self.device.clone() else {
            return;
        };
        let Some(p) = self.popup_as::<UnitParams>() else {
            return;
        };
        let (register_type, address, amount) = (p.register_type, p.address, p.amount);
        self.background_task = Some(BackgroundTask::UnitScan(compat::spawn(async move {
            let outcome = match device
                .read_typed(Some(unit_id), register_type, address, amount)
                .await
            {
                Ok(values) => UnitProbeOutcome::Response(values),
                Err(e) => e
                    .downcast_ref::<tokio_modbus::ExceptionCode>()
                    .map(|code| UnitProbeOutcome::Exception(code.to_string()))
                    .unwrap_or_else(|| UnitProbeOutcome::Silent),
            };
            UnitScanTaskResult { unit_id, outcome }
        })));
    }

    pub(super) fn apply_unit_scan_result(&mut self, result: Option<UnitScanTaskResult>) {
        let Some(p) = self.unit_mut() else {
            return;
        };
        if !p.active() {
            return;
        }
        let Some(UnitScanTaskResult { unit_id, outcome }) = result else {
            p.scan = ScanState::Failed;
            log::error!("Unit scan failed | task stopped unexpectedly");
            return;
        };

        let responded = matches!(outcome, UnitProbeOutcome::Response(_));
        match outcome {
            UnitProbeOutcome::Response(values) => {
                log::info!("Unit scan | unit {unit_id} responded | {values:?}");
                p.hits.push(UnitScanHit {
                    unit_id,
                    result: Ok(values),
                });
            }
            UnitProbeOutcome::Exception(text) => p.hits.push(UnitScanHit {
                unit_id,
                result: Err(text),
            }),
            UnitProbeOutcome::Silent => {}
        }

        if (p.stop_at_first && responded) || unit_id >= p.to {
            p.scan = ScanState::Done;
            let ok = p.hits.iter().filter(|h| h.result.is_ok()).count();
            let exceptions = p.hits.len() - ok;
            log::info!("Unit scan finished | {ok} response(s), {exceptions} exception(s)");
            return;
        }

        p.current = unit_id + 1;
        let next = p.current;
        self.spawn_unit_probe(next);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::register::RegisterType;
    use crate::state::{ScanState, UnitField, UnitParams};

    async fn drive_scan(app: &mut App) {
        crate::app::settle_until(
            app,
            |app| app.popup_as::<UnitParams>().is_none_or(|p| !p.active()),
            "scan",
        )
        .await;
    }

    async fn unit_popup() -> App {
        // The mock device answers units 0..10 and stays silent otherwise.
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_unit();
        app
    }

    #[tokio::test]
    async fn scan_lists_responding_units() {
        let mut app = unit_popup().await;
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.from = 1;
            p.to = 3;
        }
        app.unit_scan_action();
        drive_scan(&mut app).await;

        let p = app.popup_as::<UnitParams>().unwrap();
        assert_eq!(p.scan, ScanState::Done);
        assert_eq!(p.status, None, "progress is not a status message");
        let ids: Vec<u8> = p.hits.iter().map(|h| h.unit_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(p.hits.iter().all(|h| h.result.is_ok()));
    }

    #[tokio::test]
    async fn scan_stops_at_first_hit() {
        let mut app = unit_popup().await;
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.from = 2;
            p.to = 247;
            p.stop_at_first = true;
        }
        app.unit_scan_action();
        drive_scan(&mut app).await;

        let p = app.popup_as::<UnitParams>().unwrap();
        assert_eq!(p.scan, ScanState::Done);
        let ids: Vec<u8> = p.hits.iter().map(|h| h.unit_id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[tokio::test]
    async fn selecting_a_hit_applies_that_unit_id() {
        let mut app = unit_popup().await;
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.from = 4;
            p.to = 5;
        }
        app.unit_scan_action();
        drive_scan(&mut app).await;

        // Hits follow the fixed fields, move down onto the second hit (id 5).
        let p = app.popup_as::<UnitParams>().unwrap();
        let hit_index = p
            .fields()
            .iter()
            .position(|f| *f == UnitField::Hit(1))
            .unwrap();
        for _ in 0..hit_index {
            app.unit_move(true);
        }
        assert_eq!(
            app.popup_as::<UnitParams>().unwrap().current_field(),
            UnitField::Hit(1)
        );

        app.commit_unit_hit(1).await;
        assert_eq!(app.config.device.unit_id, 5);
        let p = app.popup_as::<UnitParams>().expect("the popup stays open");
        assert_eq!(p.id, 5, "the Unit id field follows the pick");
        assert_eq!(p.status, None, "the list marker is the only confirmation");
    }

    #[tokio::test]
    async fn the_last_scan_is_remembered_until_the_device_changes() {
        use crate::modbus::{Interface, InterfaceTcpParams};
        let mut app = unit_popup().await;
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.from = 2;
            p.to = 4;
        }
        app.unit_scan_action();
        drive_scan(&mut app).await;
        app.close_popup();
        assert!(app.popup_as::<UnitParams>().is_none());

        app.config.device.unit_id = 3;
        app.open_unit();
        let p = app.popup_as::<UnitParams>().unwrap();
        assert_eq!(p.scan, ScanState::Done);
        assert_eq!((p.from, p.to), (2, 4));
        let ids: Vec<u8> = p.hits.iter().map(|h| h.unit_id).collect();
        assert_eq!(ids, vec![2, 3, 4]);
        assert_eq!(p.id, 3, "the id field follows the config");

        app.commit_unit().await;
        assert!(app.popup_as::<UnitParams>().is_none());
        app.open_unit();
        assert_eq!(
            app.popup_as::<UnitParams>().unwrap().hits.len(),
            3,
            "setting the id keeps the session"
        );

        app.close_popup();
        app.config.device.interface = Interface::Tcp(InterfaceTcpParams {
            ip: "10.0.0.1".into(),
            port: 502,
        });
        app.open_unit();
        let p = app.popup_as::<UnitParams>().unwrap();
        assert_eq!(p.scan, ScanState::Idle, "another device starts fresh");
        assert!(p.hits.is_empty());
        assert_eq!((p.from, p.to), (1, 247));
    }

    #[tokio::test]
    async fn an_exception_entry_sets_the_unit_id_too() {
        let mut app = unit_popup().await;
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.register_type = RegisterType::Input;
            p.address = 38;
            p.amount = 1;
            p.from = 7;
            p.to = 7;
        }
        app.unit_scan_action();
        drive_scan(&mut app).await;

        let p = app.popup_as::<UnitParams>().unwrap();
        assert_eq!(p.hits.len(), 1);
        assert!(p.hits[0].result.is_err());

        app.commit_unit_hit(0).await;
        assert_eq!(app.config.device.unit_id, 7);
        assert_eq!(app.popup_as::<UnitParams>().unwrap().id, 7);
    }
}
