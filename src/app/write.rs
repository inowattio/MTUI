use super::{App, BackgroundTask, PendingWrite, WriteOutcome, WriteType};
use crate::compat;
use crate::constants::UNINTERPRETABLE;
use crate::constants::message;
use crate::modbus::WordOrder;
use crate::num_ops::cycle;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{Popup, StatusMessage, WriteFunc, WriteParams};

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
            .is_some_and(|rule| rule.repr.register_count() >= 2)
        {
            WriteType::DWord
        } else {
            WriteType::Word
        };

        let value = match write_type {
            WriteType::DWord => self.dword_value((kind, write_pos)).map(i64::from),
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

    fn dword_value(&self, (kind, address): RegisterCell) -> Option<u32> {
        let lo = self.cell_value((kind, address))?;
        let hi = self.cell_value((kind, address.wrapping_add(1)))?;
        Some(self.config.device.word_order.make_word(lo, hi))
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

        let (value, second) = match w.write_type {
            WriteType::Coil => ((number != 0) as u16, None),
            WriteType::Word => (number as u16, None),
            WriteType::DWord => {
                let [first, second] = self.config.device.word_order.split_word(number as u32);
                (first, Some(second))
            }
        };
        let at = |address: u16| {
            if address == w.position.wrapping_add(1) && second.is_some() {
                return second;
            }
            self.read_log.get(&(kind, address)).map(|e| e.value)
        };
        self.custom_value(cell, value, &at)
    }

    pub fn commit_write(&mut self) {
        if self.config.read_only {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info(message::READ_ONLY));
            }
            return;
        }
        if let Some(task) = &self.background_task {
            let message = if matches!(task, BackgroundTask::Refresh(_)) {
                message::DEVICE_READING
            } else {
                message::DEVICE_BUSY
            };
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info(message));
            }
            return;
        }

        let Some(device) = self.device.clone() else {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::err(message::NO_DEVICE));
            }
            return;
        };

        let (position, number, write_type, func) = {
            let Some(w) = self.write_mut() else {
                return;
            };
            let Some(number) = w.value else {
                w.result = Some(StatusMessage::info(message::ENTER_VALUE));
                return;
            };
            w.result = Some(StatusMessage::info(message::WRITING));
            (w.position, number, w.write_type, w.func)
        };

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
                self.dword_value(cell).map(u64::from),
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

        let word_order = self.config.device.word_order;
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
                    .map(|()| Vec::new()),
                (WriteType::Word, WriteFunc::Single) => device
                    .write_register(position, number as u16)
                    .await
                    .map(|()| Vec::new()),
                (_, WriteFunc::ReadWrite) => device.read_write_registers(position, &words).await,
                (_, _) => device
                    .write_registers(position, &words)
                    .await
                    .map(|()| Vec::new()),
            };
            match result {
                Ok(read) => WriteOutcome {
                    ok: true,
                    message: read_back_message(word_order.assemble(&read), new_value),
                    read_back: read
                        .into_iter()
                        .enumerate()
                        .map(|(i, value)| ((RegisterType::Holding, position + i as u16), value))
                        .collect(),
                },
                Err(e) => WriteOutcome {
                    ok: false,
                    message: format!("Write failed: {e}"),
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

fn read_back_message(read_back: Option<u128>, written: u64) -> String {
    match read_back {
        None => "Write OK".to_string(),
        Some(value) if value == u128::from(written) => format!("Write OK | read back {value}"),
        Some(value) => format!("Write OK | read back {value} (differs)"),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::read_back_message;
    use crate::app::{App, BackgroundTask};
    use crate::config::Config;
    use crate::constants::message;
    use crate::register::RegisterType;
    use crate::state::{MessageKind, WriteParams};

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
    async fn committing_during_a_read_says_the_device_is_reading() {
        let mut app = write_popup().await;
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
