use super::{App, BackgroundTask, SlaveProbeOutcome, SlaveScanTaskResult};
use crate::compat;
use crate::num_ops::{digit_add, digit_remove, wrap_index};
use crate::state::{ScanState, SlaveField, SlaveParams, SlaveScanHit, StatusMessage};

impl App {
    fn slave_mut(&mut self) -> Option<&mut SlaveParams> {
        self.popup_as_mut()
    }

    pub fn slave_move(&mut self, down: bool) {
        if let Some(p) = self.slave_mut() {
            let n = p.fields().len() as u16;
            p.selected = wrap_index(p.selected.min(n - 1), n, down);
        }
    }

    pub fn slave_scan_toggle(&mut self) {
        if let Some(p) = self.slave_mut() {
            p.stop_at_first = !p.stop_at_first;
        }
    }

    pub fn slave_digit(&mut self, field: SlaveField, c: char) {
        if !c.is_ascii_digit() {
            return;
        }
        let digit = c as u8 - b'0';
        if let Some(p) = self.slave_mut() {
            match field {
                SlaveField::Id => digit_add(&mut p.id, digit),
                SlaveField::From => digit_add(&mut p.from, digit),
                SlaveField::To => digit_add(&mut p.to, digit),
                SlaveField::Mode | SlaveField::Scan | SlaveField::Hit(_) => {}
            }
        }
    }

    pub fn slave_backspace(&mut self, field: SlaveField) {
        if let Some(p) = self.slave_mut() {
            match field {
                SlaveField::Id => digit_remove(&mut p.id),
                SlaveField::From => digit_remove(&mut p.from),
                SlaveField::To => digit_remove(&mut p.to),
                SlaveField::Mode | SlaveField::Scan | SlaveField::Hit(_) => {}
            }
        }
    }

    pub fn slave_scan_action(&mut self) {
        let Some(p) = self.popup_as::<SlaveParams>() else {
            return;
        };
        if p.active() {
            if let Some(p) = self.slave_mut() {
                p.scan = ScanState::Stopped;
            }
            log::info!("Slave scan stopped");
            return;
        }

        if self.device.is_none() {
            if let Some(p) = self.slave_mut() {
                p.status = Some(StatusMessage::err("No device connected"));
            }
            return;
        }
        if !self.free_background_slot() {
            if let Some(p) = self.slave_mut() {
                p.status = Some(StatusMessage::info("Device is busy."));
            }
            return;
        }

        let Some(p) = self.slave_mut() else {
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
            "Slave scan started \u{b7} {}..={} \u{b7} {:?} @ {} \u{d7}{}{}",
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
        self.spawn_slave_probe(first);
    }

    fn spawn_slave_probe(&mut self, slave_id: u8) {
        let Some(device) = self.device.clone() else {
            return;
        };
        let Some(p) = self.popup_as::<SlaveParams>() else {
            return;
        };
        let (register_type, address, amount) = (p.register_type, p.address, p.amount);
        self.background_task = Some(BackgroundTask::SlaveScan(compat::spawn(async move {
            let outcome = match device
                .read_typed(Some(slave_id), register_type, address, amount)
                .await
            {
                Ok(values) => SlaveProbeOutcome::Response(values),
                Err(e) => e
                    .downcast_ref::<tokio_modbus::ExceptionCode>()
                    .map(|code| SlaveProbeOutcome::Exception(code.to_string()))
                    .unwrap_or_else(|| SlaveProbeOutcome::Silent),
            };
            SlaveScanTaskResult { slave_id, outcome }
        })));
    }

    pub(super) fn apply_slave_scan_result(&mut self, result: Option<SlaveScanTaskResult>) {
        let Some(p) = self.slave_mut() else {
            return;
        };
        if !p.active() {
            return;
        }
        let Some(SlaveScanTaskResult { slave_id, outcome }) = result else {
            p.scan = ScanState::Failed;
            log::error!("Slave scan failed \u{b7} task stopped unexpectedly");
            return;
        };

        let responded = matches!(outcome, SlaveProbeOutcome::Response(_));
        match outcome {
            SlaveProbeOutcome::Response(values) => {
                log::info!("Slave scan \u{b7} slave {slave_id} responded \u{b7} {values:?}");
                p.hits.push(SlaveScanHit {
                    slave_id,
                    result: Ok(values),
                });
            }
            SlaveProbeOutcome::Exception(text) => p.hits.push(SlaveScanHit {
                slave_id,
                result: Err(text),
            }),
            SlaveProbeOutcome::Silent => {}
        }

        if (p.stop_at_first && responded) || slave_id >= p.to {
            p.scan = ScanState::Done;
            let ok = p.hits.iter().filter(|h| h.result.is_ok()).count();
            let exceptions = p.hits.len() - ok;
            log::info!("Slave scan finished \u{b7} {ok} response(s), {exceptions} exception(s)");
            return;
        }

        p.current = slave_id + 1;
        let next = p.current;
        self.spawn_slave_probe(next);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::state::{ScanState, SlaveField, SlaveParams};
    use std::time::Duration;

    async fn drive_scan(app: &mut App) {
        for _ in 0..500 {
            app.complete_background_task().await;
            if app.popup_as::<SlaveParams>().is_none_or(|p| !p.active()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        panic!("scan never finished");
    }

    async fn slave_popup() -> App {
        // The mock device answers slaves 0..10 and stays silent otherwise.
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_slave();
        app
    }

    #[tokio::test]
    async fn scan_lists_responding_slaves() {
        let mut app = slave_popup().await;
        {
            let p = app.popup_as_mut::<SlaveParams>().unwrap();
            p.from = 1;
            p.to = 3;
        }
        app.slave_scan_action();
        drive_scan(&mut app).await;

        let p = app.popup_as::<SlaveParams>().unwrap();
        assert_eq!(p.scan, ScanState::Done);
        assert_eq!(p.status, None, "progress is not a status message");
        let ids: Vec<u8> = p.hits.iter().map(|h| h.slave_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
        assert!(p.hits.iter().all(|h| h.result.is_ok()));
    }

    #[tokio::test]
    async fn scan_stops_at_first_hit() {
        let mut app = slave_popup().await;
        {
            let p = app.popup_as_mut::<SlaveParams>().unwrap();
            p.from = 2;
            p.to = 247;
            p.stop_at_first = true;
        }
        app.slave_scan_action();
        drive_scan(&mut app).await;

        let p = app.popup_as::<SlaveParams>().unwrap();
        assert_eq!(p.scan, ScanState::Done);
        let ids: Vec<u8> = p.hits.iter().map(|h| h.slave_id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[tokio::test]
    async fn selecting_a_hit_applies_that_slave_id() {
        let mut app = slave_popup().await;
        {
            let p = app.popup_as_mut::<SlaveParams>().unwrap();
            p.from = 4;
            p.to = 5;
        }
        app.slave_scan_action();
        drive_scan(&mut app).await;

        // Hits follow the fixed fields, move down onto the second hit (id 5).
        let p = app.popup_as::<SlaveParams>().unwrap();
        let hit_index = p
            .fields()
            .iter()
            .position(|f| *f == SlaveField::Hit(1))
            .unwrap();
        for _ in 0..hit_index {
            app.slave_move(true);
        }
        assert_eq!(
            app.popup_as::<SlaveParams>().unwrap().current_field(),
            SlaveField::Hit(1)
        );

        app.commit_slave_hit(1).await;
        assert_eq!(app.config.device.slave_id, 5);
        assert!(app.popup_as::<SlaveParams>().is_none());
    }
}
