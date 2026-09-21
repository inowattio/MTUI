use super::{App, BackgroundTask, RawTaskResult};
use crate::compat;
use crate::state::{Popup, RawField, RawParams, StatusMessage};

fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if !cleaned.len().is_multiple_of(2) {
        return Err("hex data needs an even number of digits".to_string());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("invalid hex byte '{}'", &cleaned[i..i + 2]))
        })
        .collect()
}

impl App {
    pub fn open_raw(&mut self) {
        self.read_mut().popup = Some(Popup::Raw(RawParams::default()));
    }

    fn raw_mut(&mut self) -> Option<&mut RawParams> {
        self.popup_as_mut()
    }

    pub fn raw_move(&mut self) {
        if let Some(p) = self.raw_mut() {
            p.field = match p.field {
                RawField::Code => RawField::Data,
                RawField::Data => RawField::Code,
            };
        }
    }

    pub fn raw_input(&mut self, c: char) {
        if let Some(p) = self.raw_mut() {
            match p.field {
                RawField::Code if c.is_ascii_digit() && p.code.len() < 3 => p.code.push(c),
                RawField::Data if c.is_ascii_hexdigit() || c == ' ' => p.data.push(c),
                _ => {}
            }
        }
    }

    pub fn raw_backspace(&mut self) {
        if let Some(p) = self.raw_mut() {
            match p.field {
                RawField::Code => {
                    p.code.pop();
                }
                RawField::Data => {
                    p.data.pop();
                }
            }
        }
    }

    pub fn raw_send(&mut self) {
        if self.config.read_only {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::warn(
                    "Read-only mode is on - custom calls may write and are disabled",
                ));
            }
            return;
        }

        let (code_str, data_str) = match self.raw_mut() {
            Some(p) => (p.code.trim().to_string(), p.data.clone()),
            None => return,
        };

        let code = match code_str.parse::<u16>() {
            Ok(value) if value <= u8::MAX as u16 => value as u8,
            _ => {
                if let Some(p) = self.raw_mut() {
                    p.status = Some(StatusMessage::err("Function code must be 0-255"));
                }
                return;
            }
        };

        let data = match parse_hex_bytes(&data_str) {
            Ok(data) => data,
            Err(e) => {
                if let Some(p) = self.raw_mut() {
                    p.status = Some(StatusMessage::err(e));
                }
                return;
            }
        };

        let Some(device) = self.device.clone() else {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::err("No device connected"));
            }
            return;
        };

        if !self.free_background_slot() {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::info("Device is busy."));
            }
            return;
        }

        if let Some(p) = self.raw_mut() {
            p.status = Some(StatusMessage::info("Sending..."));
        }

        let sent = data.len();
        self.background_task = Some(BackgroundTask::Raw(compat::spawn(async move {
            let result = device.custom(code, &data).await.map_err(|e| e.to_string());
            RawTaskResult { code, sent, result }
        })));
    }

    pub(super) fn apply_raw_result(&mut self, result: Option<RawTaskResult>) {
        // The popup may have been closed while the call was in flight.
        let Some(p) = self.raw_mut() else {
            return;
        };
        let Some(RawTaskResult { code, sent, result }) = result else {
            p.status = Some(StatusMessage::err("Failed: task stopped unexpectedly"));
            return;
        };
        match result {
            Ok(bytes) => {
                log::info!(
                    "Raw function {code:#04X} | {sent} byte(s) in, {} byte(s) out",
                    bytes.len()
                );
                p.response = Some(if bytes.is_empty() {
                    "(empty)".to_string()
                } else {
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                p.status = Some(StatusMessage::ok(format!(
                    "{} byte(s) returned",
                    bytes.len()
                )));
            }
            Err(e) => {
                log::error!("Raw function {code:#04X} failed | {e}");
                p.response = None;
                p.status = Some(StatusMessage::err(format!("Failed: {e}")));
            }
        }
    }
}
