use super::{App, BackgroundTask, RawTaskResult, StatTarget};
use crate::compat;
use crate::constants::message;
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
                p.status = Some(StatusMessage::warn(message::RAW_READ_ONLY));
            }
            return;
        }

        let (code_str, data_str) = match self.raw_mut() {
            Some(p) => (p.code.trim().to_string(), p.data.clone()),
            None => return,
        };

        let code = match code_str.parse::<u16>() {
            Ok(value) if u8::try_from(value).is_ok() => value as u8,
            _ => {
                if let Some(p) = self.raw_mut() {
                    p.status = Some(StatusMessage::err(message::RAW_BAD_FUNCTION_CODE));
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
                p.status = Some(StatusMessage::err(message::NO_DEVICE));
            }
            return;
        };

        if !self.free_background_slot() {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::info(message::DEVICE_BUSY));
            }
            return;
        }

        if let Some(p) = self.raw_mut() {
            p.status = Some(StatusMessage::info(message::SENDING));
        }

        let sent = data.len();
        self.background_task = Some(BackgroundTask::Raw(compat::spawn(async move {
            let result = device
                .custom_timed(None, code, &data)
                .await
                .map_err(|e| e.to_string());
            RawTaskResult { code, sent, result }
        })));
    }

    pub(super) fn apply_raw_result(&mut self, result: Option<RawTaskResult>) {
        match &result {
            Some(RawTaskResult {
                result: Ok((_, latency)),
                ..
            }) => self.stats.others.record_ok(*latency),
            Some(RawTaskResult {
                code,
                result: Err(e),
                ..
            }) => self
                .stats
                .others
                .record_error(e, Some(StatTarget::Function(*code))),
            None => self
                .stats
                .others
                .record_error("raw task stopped unexpectedly", None),
        }
        // The popup may have been closed while the call was in flight.
        let Some(p) = self.raw_mut() else {
            return;
        };
        let Some(RawTaskResult { code, sent, result }) = result else {
            p.status = Some(StatusMessage::err(message::TASK_STOPPED));
            return;
        };
        match result {
            Ok((bytes, _)) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;

    fn outcome(result: Result<(Vec<u8>, Duration), String>) -> Option<RawTaskResult> {
        Some(RawTaskResult {
            code: 0x41,
            sent: 0,
            result,
        })
    }

    #[tokio::test]
    async fn raw_calls_count_as_others_even_with_the_popup_closed() {
        let mut app = App::boot(Config::default(), String::new()).await;
        assert!(app.raw_mut().is_none());

        app.apply_raw_result(outcome(Ok((vec![0x01], Duration::from_millis(3)))));
        app.apply_raw_result(outcome(Err("Illegal function".to_string())));

        let others = &app.stats.others;
        assert_eq!((others.ok, others.errors), (1, 1));
        assert_eq!(
            others.latency().map(|(min, _, _)| min),
            Some(Duration::from_millis(3))
        );
        let error = others.last_error().expect("the failure is kept");
        assert_eq!(error.target, Some(StatTarget::Function(0x41)));
        assert_eq!(app.stats.reads.ok + app.stats.writes.ok, 0);

        app.apply_raw_result(None);
        let error = app
            .stats
            .others
            .last_error()
            .expect("the dead task is kept");
        assert_eq!(error.message, "raw task stopped unexpectedly");
        assert_eq!(error.target, None);
    }
}
