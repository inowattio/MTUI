use super::{App, BackgroundTask, PendingWrite, WriteOutcome, WriteType};
use crate::compat;
use crate::constants::UNINTERPRETABLE;
use crate::register::RegisterType;
use crate::state::{Popup, StatusMessage, WriteParams};

impl App {
    pub fn open_write(&mut self) {
        if self.config.read_only {
            self.set_read_status(StatusMessage::warn(
                "Read-only mode is on \u{2014} writes are disabled (toggle in settings)",
            ));
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
                "{what} are read-only \u{2014} cannot write"
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
            WriteType::DWord => {
                let lo = self.previous_values.get(&(kind, write_pos));
                let hi = self.previous_values.get(&(kind, write_pos.wrapping_add(1)));
                match (lo, hi) {
                    (Some(&a), Some(&b)) => {
                        Some(self.config.device.word_order.make_word(a, b) as i64)
                    }
                    _ => None,
                }
            }
            _ => self
                .previous_values
                .get(&(kind, write_pos))
                .map(|&v| v as i64),
        };

        let bit_cursor = write_type.bits() - 1;
        let p = self.read_mut();
        p.status = None;
        p.popup = Some(Popup::Write(WriteParams {
            position: write_pos,
            value,
            write_type,
            bit_cursor,
            ..Default::default()
        }));
    }

    pub fn write_mut(&mut self) -> Option<&mut WriteParams> {
        self.popup_as_mut()
    }

    fn write(&self) -> Option<&WriteParams> {
        self.popup_as()
    }

    pub fn toggle_word_order(&mut self) {
        let next = self.config.device.word_order.next();
        self.config.device.word_order = next;
        self.interpreter.set_word_order(next);
        if let Some(device) = &mut self.device {
            device.set_word_order(next);
        }
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

        let order = self.config.device.word_order;
        let (value, second) = match w.write_type {
            WriteType::Coil => ((number != 0) as u16, None),
            WriteType::Word => (number as u16, None),
            WriteType::DWord => {
                let [first, second] = order.split_word(number as u32);
                (first, Some(second))
            }
        };
        let at = |address: u16| {
            if address == w.position.wrapping_add(1) && second.is_some() {
                return second;
            }
            self.read_log.get(&(kind, address)).map(|&(v, _)| v)
        };
        self.custom_value(cell, value, order, &at)
    }

    pub fn commit_write(&mut self) {
        if self.config.read_only {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info("Read-only mode."));
            }
            return;
        }
        if self.background_task.is_some() {
            if let Some(w) = self.write_mut() {
                w.result = Some(StatusMessage::info("Device is busy."));
            }
            return;
        }

        let (position, number, write_type, force_multiple) = {
            let Some(w) = self.write_mut() else {
                return;
            };
            let Some(number) = w.value else {
                w.result = Some(StatusMessage::info("Enter a value first."));
                return;
            };
            w.result = Some(StatusMessage::info("Writing..."));
            (w.position, number, w.write_type, w.force_multiple)
        };

        let Some(device) = self.device.clone() else {
            return;
        };

        let kind = if write_type == WriteType::Coil {
            RegisterType::Coil
        } else {
            RegisterType::Holding
        };
        let cell = (kind, position);
        let (previous, new_value) = match write_type {
            WriteType::Word => (
                self.previous_values.get(&cell).map(|&v| v as u64),
                (number as u16) as u64,
            ),
            WriteType::Coil => (
                self.previous_values.get(&cell).map(|&v| v as u64),
                (number != 0) as u64,
            ),
            WriteType::DWord => {
                let order = self.config.device.word_order;
                let lo = self.previous_values.get(&cell);
                let hi = self
                    .previous_values
                    .get(&(RegisterType::Holding, position.wrapping_add(1)));
                let previous = match (lo, hi) {
                    (Some(&a), Some(&b)) => Some(order.make_word(a, b) as u64),
                    _ => None,
                };
                (previous, (number as u32) as u64)
            }
        };
        self.pending_write = Some(PendingWrite {
            address: position,
            write_type,
            previous,
            new_value,
        });

        self.background_task = Some(BackgroundTask::Write(compat::spawn(async move {
            let result = match write_type {
                WriteType::Word if force_multiple => {
                    device.write_registers(position, &[number as u16]).await
                }
                WriteType::Word => device.write_register(position, number as u16).await,
                WriteType::DWord => device.write_register_word(position, number as i32).await,
                WriteType::Coil => device.write_coil(position, number != 0).await,
            };
            match result {
                Ok(()) => WriteOutcome {
                    ok: true,
                    message: "Write OK".to_string(),
                },
                Err(e) => WriteOutcome {
                    ok: false,
                    message: format!("Write failed: {e}"),
                },
            }
        })));
    }

    fn write_bit_count(&self) -> u16 {
        self.write().map_or(16, |w| w.write_type.bits())
    }

    pub fn write_toggle_type(&mut self) {
        if let Some(w) = self.write_mut() {
            match (w.write_type, w.force_multiple) {
                (WriteType::Word, false) => w.force_multiple = true,
                (WriteType::Word, true) => {
                    w.write_type = WriteType::DWord;
                    w.force_multiple = false;
                }
                (WriteType::DWord, _) => {
                    w.write_type = WriteType::Word;
                    w.force_multiple = false;
                }
                (WriteType::Coil, _) => {}
            }
            let bits = w.write_type.bits();
            w.bit_cursor = w.bit_cursor.min(bits - 1);
        }
        self.clamp_write_value();
    }

    pub fn clamp_write_value(&mut self) {
        if let Some(w) = self.write_mut() {
            if let Some(value) = w.value {
                let (lo, hi) = match w.write_type {
                    WriteType::Coil => (0, 1),
                    WriteType::Word => (i16::MIN as i64, u16::MAX as i64),
                    WriteType::DWord => (i32::MIN as i64, u32::MAX as i64),
                };
                w.value = Some(value.clamp(lo, hi));
            }
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
        if let Some(w) = self.write_mut() {
            let mask = 1u32 << w.bit_cursor;
            let current = w.value.unwrap_or(0) as u32;
            w.value = Some((current ^ mask) as i64);
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
    }

    pub(super) fn notify_no_cycle_types(&mut self) {
        let jump = self.config.keybinds.jump;
        let settings = self.config.keybinds.settings;
        self.set_read_status(StatusMessage::warn(format!(
            "No other register type to cycle to \u{2014} jump to one with [{jump}] or change in settings [{settings}]"
        )));
    }

    fn next_cycle_type(&self, from: RegisterType) -> RegisterType {
        let cycle = self.config.cycle_types;
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
