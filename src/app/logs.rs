use super::{App, WriteType};
use crate::logger;
use crate::modbus::Interface;
use crate::num_ops::step_hscroll;
use crate::state::{LogViewParams, LogsParams, Popup, State, StatusMessage};
use crate::writes_log::{self, SharedWritesLog, WriteKind};
use chrono::Local;
use std::fs;

impl App {
    pub fn clear_session_data(&mut self) {
        self.clear_read_accumulation();
        log::info!("Cleared session read data");
        self.set_read_status(StatusMessage::ok("Cleared session read data"));
    }

    pub fn writes_log_path(&self) -> std::path::PathBuf {
        let kind = match &self.config.device.interface {
            Interface::Mock => "mock",
            Interface::Serial(_) => "serial",
            Interface::Tcp(_) => "tcp",
            Interface::RtuOverTcp(_) => "rtu-tcp",
        };
        let name = super::file_name(
            &[
                "writes",
                &self.config.name,
                kind,
                &self.config.device.unit_id.to_string(),
            ],
            "csv",
        );
        let directory = self.config.write_log.directory.trim();
        let dir = if directory.is_empty() {
            self.config_path
                .parent()
                .map_or_else(std::path::PathBuf::new, std::path::Path::to_path_buf)
        } else {
            std::path::PathBuf::from(directory)
        };
        dir.join(name)
    }

    pub fn open_logs(&mut self) {
        let path = self.writes_log_path();
        let mut params = LogsParams {
            path: path.display().to_string(),
            entries: writes_log::read_entries(&path).unwrap_or_default(),
            top: 0,
        };
        params.scroll_to_bottom();
        self.read_mut().popup = Some(Popup::Logs(params));
    }

    pub fn logs_scroll(&mut self, delta: i32) {
        if let Some(l) = self.popup_as_mut::<LogsParams>() {
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
            h_offset: 0,
            wrap: false,
            previous,
        });
        self.log_view_scroll(i32::MAX);
    }

    pub fn log_view_hscroll(&mut self, right: bool) {
        let max = self.h_max_offset.get();
        if let Some(l) = self.log_view_mut() {
            if l.wrap {
                return;
            }
            l.h_offset = step_hscroll(l.h_offset, max, right);
        }
    }

    pub fn log_view_toggle_wrap(&mut self) {
        if let Some(l) = self.log_view_mut() {
            l.wrap = !l.wrap;
            l.h_offset = 0;
        }
    }

    pub fn close_log_view(&mut self) {
        let previous = match &mut self.state {
            State::Logs(l) => std::mem::take(&mut l.previous),
            _ => return,
        };
        self.state = State::Read(previous);
    }

    pub fn copy_app_logs(&mut self) {
        let entries = logger::snapshot();
        let count = entries.len();
        if self.set_clipboard(logger::export(&entries)) {
            log::info!("Copied {count} log line(s) to clipboard");
        } else {
            log::error!("Clipboard unavailable");
        }
    }

    pub fn dump_app_logs(&mut self) {
        let entries = logger::snapshot();
        let filename = super::file_name(
            &[
                "logs",
                &self.config.name,
                &Local::now().format("%Y%m%d_%H%M%S").to_string(),
            ],
            "txt",
        );
        match fs::write(&filename, logger::export(&entries)) {
            Ok(()) => log::info!("Saved {} log line(s) to {filename}", entries.len()),
            Err(e) => log::error!("Log dump failed | {e}"),
        }
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
            state.enabled = self.config.write_log.enabled;
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
        writes_log::append(
            &self.writes_log,
            pending.unit,
            pending.address,
            kind,
            pending.previous,
        );
    }
}
