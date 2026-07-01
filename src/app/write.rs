use super::*;

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

        let value = self
            .previous_values
            .get(&(kind, write_pos))
            .map(|&v| v as i64);

        let write_type = if kind == RegisterType::Coil {
            WriteType::Coil
        } else {
            WriteType::Word
        };

        let p = self.read_mut();
        p.status = None;
        p.popup = Some(Popup::Write(WriteParams {
            position: write_pos,
            value,
            write_type,
            ..Default::default()
        }));
    }

    pub fn write_mut(&mut self) -> Option<&mut WriteParams> {
        match &mut self.state {
            State::Read(p) => match &mut p.popup {
                Some(Popup::Write(w)) => Some(w),
                _ => None,
            },
            _ => None,
        }
    }

    fn write(&self) -> Option<&WriteParams> {
        match &self.state {
            State::Read(p) => match &p.popup {
                Some(Popup::Write(w)) => Some(w),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn toggle_word_order(&mut self) {
        let next = self.config.device.word_order.next();
        self.config.device.word_order = next;
        self.interpreter.set_word_order(next);
        if let Some(device) = &mut self.device {
            device.set_word_order(next);
        }
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
