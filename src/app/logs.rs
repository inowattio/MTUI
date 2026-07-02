use super::{App, WriteType};
use crate::modbus::Interface;
use crate::state::{LogViewParams, LogsParams, Popup, ReadPanel, State, StatusMessage};
use crate::writes_log::{SharedWritesLog, WriteKind};
use chrono::{Local, SecondsFormat};
use std::fs;

impl App {
    fn note_cleared(&mut self, n: usize, noun: &str) {
        self.dirty = true;
        log::info!("Cleared {n} {noun}(s)");
        self.set_settings_status(StatusMessage::ok(format!("Cleared {n} {noun}(s)")));
    }

    pub fn clear_pins(&mut self) {
        let n = self.pinned_registers.len();
        self.pinned_registers.clear();
        self.note_cleared(n, "pinned register");
    }

    pub fn clear_labels(&mut self) {
        let n = self.labels.len();
        self.labels.clear();
        self.note_cleared(n, "label");
    }

    pub fn clear_custom(&mut self) {
        let n = self.custom_rules.len();
        self.custom_rules.clear();
        self.note_cleared(n, "custom rule");
    }

    pub fn clear_session_data(&mut self) {
        self.clear_read_accumulation();
        log::info!("Cleared session read data");
        self.set_read_status(StatusMessage::ok("Cleared session read data"));
    }

    pub fn writes_log_path(&self) -> std::path::PathBuf {
        let kind = match &self.config.device.interface {
            Interface::Mock => "mock",
            Interface::Wired(_) => "wired",
            Interface::Network(_) => "network",
        };
        let name = format!("writes_{kind}_{}.txt", self.config.device.slave_id);
        #[cfg(not(target_arch = "wasm32"))]
        let dir = std::env::temp_dir();
        #[cfg(target_arch = "wasm32")]
        let dir = std::path::PathBuf::new();
        dir.join(name)
    }

    pub fn writes_log_path_string(&self) -> String {
        self.writes_log_path().display().to_string()
    }

    pub fn open_logs(&mut self) {
        let path = self.writes_log_path();
        let lines: Vec<String> = match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                content.lines().map(str::to_string).collect()
            }
            Ok(_) => vec!["(no writes logged yet)".to_string()],
            Err(_) => vec!["(log file not found — enable \"Log writes\" in settings)".to_string()],
        };
        let mut params = LogsParams {
            path: path.display().to_string(),
            lines,
            top: 0,
        };
        params.scroll_to_bottom();
        self.read_mut().popup = Some(Popup::Logs(params));
    }

    pub fn logs_scroll(&mut self, delta: i32) {
        if let Some(Popup::Logs(l)) = &mut self.read_mut().popup {
            l.scroll(delta);
        }
    }

    pub fn log_view(&self) -> Option<&LogViewParams> {
        match &self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub fn log_view_mut(&mut self) -> Option<&mut LogViewParams> {
        match &mut self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub fn open_log_view(&mut self) {
        let previous = std::mem::take(self.read_mut());
        self.state = State::Logs(LogViewParams {
            top: 0,
            follow: true,
            previous,
        });
        self.log_view_scroll(i32::MAX);
    }

    pub fn close_log_view(&mut self) {
        let previous = match &mut self.state {
            State::Logs(l) => std::mem::take(&mut l.previous),
            _ => return,
        };
        self.state = State::Read(previous);
    }

    pub fn log_view_scroll(&mut self, delta: i32) {
        let len = crate::logger::count() as i32;
        let visible = self.visible_rows.get().max(1) as i32;
        let max_top = (len - visible).max(0);
        if let Some(l) = self.log_view_mut() {
            let new = (l.top as i32 + delta).clamp(0, max_top);
            l.top = new as u16;
            l.follow = new >= max_top;
        }
    }

    pub fn writes_log_handle(&self) -> SharedWritesLog {
        self.writes_log.clone()
    }

    pub(super) fn refresh_writes_log_state(&self) {
        if let Ok(mut state) = self.writes_log.lock() {
            state.enabled = self.config.log_writes;
            state.path = Some(self.writes_log_path());
        }
    }

    pub(super) fn log_write(&self) {
        let Some(pending) = self.pending_write.as_ref() else {
            return;
        };
        let kind = match pending.write_type {
            WriteType::Word => WriteKind::Word(pending.new_value as u16),
            WriteType::DWord => WriteKind::DWord(pending.new_value as u32),
            WriteType::Coil => WriteKind::Coil(pending.new_value != 0),
        };
        crate::writes_log::append(&self.writes_log, pending.address, kind, pending.previous);
    }

    pub(super) fn dump_read_log(&self) -> StatusMessage {
        if self.read_log.is_empty() {
            return StatusMessage::info("Nothing read yet to dump.");
        }

        let filename = format!("dump_{}.txt", Local::now().format("%Y%m%d_%H%M%S"));

        let mut out = String::from("read_at\ttype\taddress\thex\tdecimal\tlabel\n");
        for (&(kind, address), &(value, read_at)) in &self.read_log {
            let label = self
                .labels
                .get(&(kind, address))
                .map(String::as_str)
                .unwrap_or("");
            out.push_str(&format!(
                "{}\t{kind:?}\t{address}\t{value:04X}\t{value}\t{label}\n",
                read_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            ));
        }

        match fs::write(&filename, out) {
            Ok(()) => StatusMessage::ok(format!(
                "Dumped {} registers to {filename}",
                self.read_log.len()
            )),
            Err(e) => StatusMessage::err(format!("Dump failed: {e}")),
        }
    }

    pub fn pin(&mut self) {
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };

        let selection = match panel {
            ReadPanel::Main | ReadPanel::Matrix => (register_type, position),
            _ => match self.panel_cell_at(pinned_index as usize) {
                Some(cell) => cell,
                None => return,
            },
        };

        let pinned = if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
            false
        } else {
            self.pinned_registers.push(selection);
            true
        };

        self.pinned_registers.sort();
        self.dirty = true;

        let rows = self.visible_rows.get();
        let len = self.panel_len();
        self.read_mut().scroll_pinned(rows, len);

        let (kind, addr) = selection;
        let verb = if pinned { "Pinned" } else { "Unpinned" };
        self.set_read_status(StatusMessage::ok(format!("{verb} {kind:?} @{addr}")));
    }
}
