use super::{App, BackgroundTask, PendingWrite, QueuedWrite, WriteOutcome, WriteType};
use crate::compat;
use crate::constants::UNINTERPRETABLE;
use crate::constants::message;
use crate::custom::{CustomCell, CustomRule};
use crate::modbus::WordOrder;
use crate::num_ops::cycle;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{Popup, StatusMessage, WriteFunc, WriteParams};
use std::time::Duration;

impl App {
    pub fn open_write(&mut self) {
        if self.config.read_only {
            self.set_read_status(StatusMessage::warn(message::READ_ONLY_ON));
            return;
        }

        let (kind, write_pos) = self.cursor_cell();

        if !kind.is_writable() {
            let what = match kind {
                RegisterType::Input => "Input registers",
                RegisterType::Discrete => "Discrete inputs",
                _ => "These registers",
            };
            self.set_read_status(StatusMessage::warn(format!(
                "{what} are read-only - cannot write"
            )));
            return;
        }

        let write_type = if kind == RegisterType::Coil {
            WriteType::Coil
        } else if self
            .custom_rule((kind, write_pos))
            .is_some_and(|rule| rule.repr.register_count() == 2)
        {
            WriteType::DWord
        } else {
            WriteType::Word
        };

        let value = match write_type {
            WriteType::DWord => self.dword_value((kind, write_pos), false).map(i64::from),
            _ => self.cell_value((kind, write_pos)).map(i64::from),
        };

        let bit_cursor = write_type.bits() - 1;
        let func = if write_type == WriteType::DWord {
            WriteFunc::Multiple
        } else {
            WriteFunc::Single
        };
        let p = self.read_mut();
        p.status = None;
        p.popup = Some(Popup::Write(WriteParams {
            position: write_pos,
            value,
            write_type,
            bit_cursor,
            func,
            ..Default::default()
        }));
    }

    pub fn write_mut(&mut self) -> Option<&mut WriteParams> {
        self.popup_as_mut()
    }

    fn write(&self) -> Option<&WriteParams> {
        self.popup_as()
    }

    fn dword_layout(&self, cell: RegisterCell, raw: bool) -> ([u16; 2], WordOrder) {
        dword_layout(
            self.custom_rule(cell).filter(|_| !raw),
            cell.1,
            self.config.device.word_order,
        )
    }

    fn dword_value(&self, cell: RegisterCell, raw: bool) -> Option<u32> {
        let (kind, _) = cell;
        let ([first, second], order) = self.dword_layout(cell, raw);
        let lo = self.cell_value((kind, first))?;
        let hi = self.cell_value((kind, second))?;
        Some(order.make_word(lo, hi))
    }

    pub fn toggle_word_order(&mut self) {
        let next = cycle(&WordOrder::ALL, self.config.device.word_order, true);
        self.config.device.word_order = next;
        self.interpreter.set_word_order(next);
        if let Some(device) = &mut self.device {
            device.set_word_order(next);
        }
        self.refresh_dirty();
    }

    fn write_has_rule_layout(&self, w: &WriteParams) -> bool {
        w.write_type == WriteType::DWord
            && self
                .custom_rule((RegisterType::Holding, w.position))
                .is_some_and(|rule| rule.repr.register_count() == 2)
    }

    pub fn write_toggle_raw(&mut self) {
        let applies = self.write().is_some_and(|w| self.write_has_rule_layout(w));
        if applies && let Some(w) = self.write_mut() {
            w.raw = !w.raw;
            w.result = None;
        }
    }

    pub fn write_layout_text(&self, w: &WriteParams) -> Option<String> {
        if !self.write_has_rule_layout(w) {
            return None;
        }
        let ([first, second], order) =
            self.dword_layout((RegisterType::Holding, w.position), w.raw);
        let mode = self.interpreter.address_mode();
        let source = if w.raw { "Raw" } else { "Rule" };
        Some(format!(
            "{source} {} + {} ({order:?})",
            mode.display(first),
            mode.display(second)
        ))
    }

    pub fn write_custom_preview(&self, w: &WriteParams) -> Option<String> {
        let number = w.value?;
        let kind = if w.write_type == WriteType::Coil {
            RegisterType::Coil
        } else {
            RegisterType::Holding
        };
        let cell = (kind, w.position);
        let rule = self.custom_rule(cell)?;

        let write_registers = if w.write_type == WriteType::DWord {
            2
        } else {
            1
        };
        if rule.repr.register_count() < write_registers {
            return Some(UNINTERPRETABLE.to_string());
        }

        let ([_, second_address], order) = self.dword_layout(cell, w.raw);
        let (value, second) = match w.write_type {
            WriteType::Coil => ((number != 0) as u16, None),
            WriteType::Word => (number as u16, None),
            WriteType::DWord => {
                let [first, second] = order.split_word(number as u32);
                (first, Some(second))
            }
        };
        let at = |address: u16| {
            if address == second_address && second.is_some() {
                return second;
            }
            self.read_log.get(&(kind, address)).map(|e| e.value)
        };
        self.custom_value(cell, value, &at)
            .and_then(CustomCell::value)
    }

    pub fn commit_write(&mut self) {
        if self.config.read_only {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info(message::READ_ONLY));
            }
            return;
        }

        let queued = {
            let Some(w) = self.write_mut() else {
                return;
            };
            let Some(number) = w.value else {
                w.result = Some(StatusMessage::info(message::ENTER_VALUE));
                return;
            };
            QueuedWrite {
                position: w.position,
                number,
                write_type: w.write_type,
                func: w.func,
                raw: w.raw,
            }
        };

        if queued.write_type == WriteType::DWord {
            let ([first, second], _) =
                self.dword_layout((RegisterType::Holding, queued.position), queued.raw);
            if second != first.wrapping_add(1) {
                if let Some(w) = self.write_mut() {
                    w.result = Some(StatusMessage::err(message::DWORD_NOT_CONTIGUOUS));
                }
                return;
            }
        }

        if let Some(task) = &self.background_task {
            let reading = matches!(task, BackgroundTask::Refresh(_));
            let text = if reading && self.config.write_waits_for_read {
                self.queued_write = Some(queued);
                message::WRITE_QUEUED
            } else if reading {
                message::DEVICE_READING
            } else {
                message::DEVICE_BUSY
            };
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info(text));
            }
            return;
        }

        self.start_write(queued);
    }

    pub(super) fn start_write(&mut self, queued: QueuedWrite) {
        let Some(device) = self.device.clone() else {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::err(message::NO_DEVICE));
            }
            return;
        };
        if let Some(w) = self.write_mut() {
            w.result = Some(StatusMessage::info(message::WRITING));
        }

        let QueuedWrite {
            position,
            number,
            write_type,
            func,
            raw,
        } = queued;
        let kind = if write_type == WriteType::Coil {
            RegisterType::Coil
        } else {
            RegisterType::Holding
        };
        let cell = (kind, position);
        let (previous, new_value) = match write_type {
            WriteType::Word => (self.cell_value(cell).map(u64::from), (number as u16) as u64),
            WriteType::Coil => (self.cell_value(cell).map(u64::from), (number != 0) as u64),
            WriteType::DWord => (
                self.dword_value(cell, raw).map(u64::from),
                (number as u32) as u64,
            ),
        };
        self.pending_write = Some(PendingWrite {
            unit: self.config.device.unit_id,
            address: position,
            write_type,
            func,
            previous,
            new_value,
        });

        let (_, word_order) = self.dword_layout(cell, raw);
        self.background_task = Some(BackgroundTask::Write(compat::spawn(async move {
            let words = match write_type {
                WriteType::Word => vec![number as u16],
                WriteType::DWord => word_order.split_word(number as u32).to_vec(),
                WriteType::Coil => vec![],
            };
            let result = match (write_type, func) {
                (WriteType::Coil, _) => device
                    .write_coil(position, number != 0)
                    .await
                    .map(|((), latency)| (Vec::new(), latency)),
                (WriteType::Word, WriteFunc::Single) => device
                    .write_register(position, number as u16)
                    .await
                    .map(|((), latency)| (Vec::new(), latency)),
                (_, WriteFunc::ReadWrite) => device.read_write_registers(position, &words).await,
                (_, _) => device
                    .write_registers(position, &words)
                    .await
                    .map(|((), latency)| (Vec::new(), latency)),
            };
            match result {
                Ok((read, latency)) => WriteOutcome {
                    ok: true,
                    message: read_back_message(word_order.assemble(&read), new_value),
                    latency,
                    read_back: read
                        .into_iter()
                        .enumerate()
                        .map(|(i, value)| ((RegisterType::Holding, position + i as u16), value))
                        .collect(),
                },
                Err(e) => WriteOutcome {
                    ok: false,
                    message: format!("Write failed: {e}"),
                    latency: Duration::ZERO,
                    read_back: Vec::new(),
                },
            }
        })));
    }

    fn write_bit_count(&self) -> u16 {
        self.write().map_or(16, |w| w.write_type.bits())
    }

    pub fn write_toggle_type(&mut self) {
        if let Some(w) = self.write_mut() {
            w.cycle_mode();
        }
        self.clamp_write_value();
    }

    pub fn clamp_write_value(&mut self) {
        if let Some(w) = self.write_mut()
            && let Some(value) = w.value
        {
            let (lo, hi) = match w.write_type {
                WriteType::Coil => (0, 1),
                WriteType::Word => (i16::MIN as i64, u16::MAX as i64),
                WriteType::DWord => (i32::MIN as i64, u32::MAX as i64),
            };
            w.value = Some(value.clamp(lo, hi));
        }
    }

    pub fn write_move_bit(&mut self, left: bool) {
        let bits = self.write_bit_count();
        if let Some(w) = self.write_mut() {
            w.bit_cursor = if left {
                (w.bit_cursor + 1).min(bits - 1)
            } else {
                w.bit_cursor.saturating_sub(1)
            };
        }
    }

    pub fn write_toggle_bit(&mut self) {
        let width = (1u64 << self.write_bit_count()) - 1;
        if let Some(w) = self.write_mut() {
            let current = w.value.unwrap_or(0) as u64 & width;
            w.value = Some((current ^ (1u64 << w.bit_cursor)) as i64);
        }
    }

    pub fn toggle_type(&mut self) {
        let current = self.read().register_type;
        let next = self.next_cycle_type(current);
        if next == current {
            self.notify_no_cycle_types();
            return;
        }
        self.stop_sweep();
        let p = self.read_mut();
        p.read_duration = None;
        p.read_error = None;
        p.register_type = next;
        self.clamp_panel_cursor();
    }

    pub(super) fn notify_no_cycle_types(&mut self) {
        let jump = self.config.keybinds.go_to;
        let settings = self.config.keybinds.settings;
        self.set_read_status(StatusMessage::warn(format!(
            "No other register type to cycle to - jump to one with [{jump}] or change in settings [{settings}]"
        )));
    }

    fn next_cycle_type(&self, from: RegisterType) -> RegisterType {
        let cycle = self.config.cycle_register_types;
        let mut next = from;
        for _ in 0..RegisterType::ALL.len() {
            next.toggle();
            if cycle.enabled(next) {
                return next;
            }
        }
        from
    }
}

fn dword_layout(
    rule: Option<&CustomRule>,
    address: u16,
    device_order: WordOrder,
) -> ([u16; 2], WordOrder) {
    match rule {
        Some(rule) if rule.repr.register_count() == 2 => {
            let second = rule
                .word_addresses()
                .nth(1)
                .unwrap_or_else(|| address.wrapping_add(1));
            ([address, second], rule.word_order.unwrap_or(device_order))
        }
        _ => ([address, address.wrapping_add(1)], device_order),
    }
}

fn read_back_message(read_back: Option<u128>, written: u64) -> String {
    match read_back {
        None => "Write OK".to_string(),
        Some(value) if value == u128::from(written) => format!("Write OK | read back {value}"),
        Some(value) => format!("Write OK | read back {value} (differs)"),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{dword_layout, read_back_message};
    use crate::app::{App, BackgroundTask};
    use crate::config::Config;
    use crate::constants::message;
    use crate::custom::{CustomRepr, CustomRule};
    use crate::modbus::WordOrder;
    use crate::register::RegisterType;
    use crate::state::{MessageKind, WriteParams};

    fn rule(repr: CustomRepr, address: u16) -> CustomRule {
        CustomRule {
            repr,
            address,
            ..Default::default()
        }
    }

    #[test]
    fn dword_layout_without_a_rule_is_contiguous_in_device_order() {
        assert_eq!(
            dword_layout(None, 100, WordOrder::CDAB),
            ([100, 101], WordOrder::CDAB)
        );
    }

    #[test]
    fn dword_layout_follows_the_rules_next_address_and_word_order() {
        let mut r = rule(CustomRepr::U32, 520);
        r.next = vec![524];
        r.word_order = Some(WordOrder::DCBA);
        assert_eq!(
            dword_layout(Some(&r), 520, WordOrder::ABCD),
            ([520, 524], WordOrder::DCBA)
        );

        let r = rule(CustomRepr::F32, 520);
        assert_eq!(
            dword_layout(Some(&r), 520, WordOrder::BADC),
            ([520, 521], WordOrder::BADC)
        );
    }

    #[test]
    fn dword_layout_ignores_rules_that_are_not_two_registers_wide() {
        let mut r = rule(CustomRepr::F64, 520);
        r.next = vec![524];
        r.word_order = Some(WordOrder::DCBA);
        assert_eq!(
            dword_layout(Some(&r), 520, WordOrder::ABCD),
            ([520, 521], WordOrder::ABCD)
        );
    }

    async fn write_popup() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.read_mut().register_type = RegisterType::Holding;
        app.open_write();
        app.write_mut().expect("write popup").value = Some(1);
        app
    }

    #[test]
    fn read_back_message_reports_matching_and_differing_values() {
        assert_eq!(read_back_message(None, 5), "Write OK");
        assert_eq!(read_back_message(Some(5), 5), "Write OK | read back 5");
        assert_eq!(
            read_back_message(Some(4), 5),
            "Write OK | read back 4 (differs)"
        );
    }

    #[tokio::test]
    async fn committing_without_a_device_reports_it() {
        let mut app = write_popup().await;
        app.device = None;

        app.commit_write();

        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.kind, MessageKind::Err);
        assert_eq!(result.text, message::NO_DEVICE);
        assert!(app.background_task.is_none(), "nothing to run");
    }

    #[tokio::test]
    async fn toggling_a_bit_on_a_negative_word_stays_within_16_bits() {
        let mut app = write_popup().await;
        let w = app.write_mut().expect("write popup");
        w.value = Some(-1);
        w.bit_cursor = 0;

        app.write_toggle_bit();

        let value = app.write_mut().expect("write popup").value;
        assert_eq!(value, Some(0xFFFE));
    }

    #[tokio::test]
    async fn committing_with_a_device_starts_the_write() {
        let mut app = write_popup().await;

        app.commit_write();

        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.text, message::WRITING);
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::Write(_))
        ));
    }

    #[tokio::test]
    async fn committing_during_a_read_queues_the_write_until_the_read_finishes() {
        let mut app = write_popup().await;
        app.refresh();

        app.commit_write();

        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.text, message::WRITE_QUEUED);
        assert!(app.queued_write.is_some());
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::Refresh(_))
        ));

        for _ in 0..200 {
            app.complete_background_task();
            if matches!(app.background_task, Some(BackgroundTask::Write(_))) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(
            matches!(app.background_task, Some(BackgroundTask::Write(_))),
            "the queued write must start once the read is done"
        );
        assert!(app.queued_write.is_none());
        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.text, message::WRITING);
    }

    #[tokio::test]
    async fn closing_the_popup_drops_the_queued_write() {
        let mut app = write_popup().await;
        app.refresh();
        app.commit_write();
        assert!(app.queued_write.is_some());

        app.close_popup();

        assert!(app.queued_write.is_none());
    }

    #[tokio::test]
    async fn committing_during_a_read_says_the_device_is_reading() {
        let mut app = write_popup().await;
        app.config.write_waits_for_read = false;
        app.refresh();

        app.commit_write();

        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.text, message::DEVICE_READING);
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::Refresh(_))
        ));
    }

    #[tokio::test]
    async fn committing_during_another_task_says_the_device_is_busy() {
        let mut app = write_popup().await;
        app.commit_write();

        app.commit_write();

        let result = app
            .popup_as::<WriteParams>()
            .and_then(|w| w.result.clone())
            .expect("a status is shown");
        assert_eq!(result.text, message::DEVICE_BUSY);
    }
}
