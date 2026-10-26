use super::{App, WriteType};
use crate::constants::message;
use crate::frames;
use crate::logger;
use crate::modbus::Interface;
use crate::num_ops::step_hscroll;
use crate::state::{LogViewParams, LogsParams, Popup, State, StatusMessage, scroll_window};
use crate::writes_log::{self, SharedWritesLog, WriteKind};
use chrono::Local;
use std::fs;

impl App {
    pub fn clear_session_data(&mut self) {
        self.clear_read_accumulation();
        log::info!("Cleared session read data");
        self.set_read_status(StatusMessage::ok(message::SESSION_CLEARED));
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
        self.writes_log_dir().join(name)
    }

    pub fn writes_log_dir(&self) -> std::path::PathBuf {
        let directory = self.config.write_log.directory.trim();
        if directory.is_empty() {
            self.config_path
                .parent()
                .map_or_else(std::path::PathBuf::new, std::path::Path::to_path_buf)
        } else {
            std::path::PathBuf::from(directory)
        }
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

    pub const fn log_view(&self) -> Option<&LogViewParams> {
        match &self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub const fn log_view_mut(&mut self) -> Option<&mut LogViewParams> {
        match &mut self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub fn open_log_view(&mut self) {
        let previous = std::mem::take(self.read_mut());
        self.state = State::Logs(LogViewParams {
            top: 0,
            selected: 0,
            follow: true,
            h_offset: 0,
            wrap: false,
            frames: false,
            previous,
        });
        self.log_view_scroll(i32::MAX);
    }

    pub fn log_view_toggle_frames(&mut self) {
        if let Some(l) = self.log_view_mut() {
            l.frames = !l.frames;
            l.h_offset = 0;
        }
        self.log_view_scroll(i32::MAX);
    }

    fn log_view_shows_frames(&self) -> bool {
        self.log_view().is_some_and(|l| l.frames)
    }

    pub fn log_view_hscroll(&mut self, right: bool) {
        let max = self.layout.h_max_offset;
        if let Some(l) = self.log_view_mut() {
            if l.wrap {
                return;
            }
            l.h_offset = step_hscroll(l.h_offset, max, right);
        }
    }

    pub const fn log_view_toggle_wrap(&mut self) {
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

    fn log_view_export(&self) -> (&'static str, usize, String) {
        if self.log_view_shows_frames() {
            let entries = frames::snapshot();
            ("frame", entries.len(), frames::export(&entries))
        } else {
            let entries = logger::snapshot();
            ("log line", entries.len(), logger::export(&entries))
        }
    }

    pub fn copy_app_logs(&mut self) {
        let (noun, count, text) = self.log_view_export();
        if self.set_clipboard(text) {
            log::info!("Copied {count} {noun}(s) to clipboard");
        } else {
            log::error!("Clipboard unavailable");
        }
    }

    pub fn dump_app_logs(&mut self) {
        let (noun, count, text) = self.log_view_export();
        let prefix = if self.log_view_shows_frames() {
            "frames"
        } else {
            "logs"
        };
        let filename = super::file_name(
            &[
                prefix,
                &self.config.name,
                &Local::now().format("%Y%m%d_%H%M%S").to_string(),
            ],
            "txt",
        );
        match fs::write(&filename, text) {
            Ok(()) => log::info!("Saved {count} {noun}(s) to {filename}"),
            Err(e) => log::error!("Log dump failed | {e}"),
        }
    }

    pub fn log_view_scroll(&mut self, delta: i32) {
        let shows_frames = self.log_view_shows_frames();
        let len = if shows_frames {
            frames::count()
        } else {
            logger::count()
        } as i32;
        let visible = self.layout.visible_rows.max(1) as i32;
        let max_top = (len - visible).max(0);
        if let Some(l) = self.log_view_mut() {
            if shows_frames {
                let last = (len - 1).max(0);
                let from = if l.follow {
                    l.top = max_top as u16;
                    last
                } else {
                    i32::from(l.selected)
                };
                l.selected = from.saturating_add(delta).clamp(0, last) as u16;
                scroll_window(&mut l.selected, &mut l.top, visible as u16, len as u16);
                l.follow = i32::from(l.selected) >= last;
            } else {
                let new = (l.top as i32).saturating_add(delta).clamp(0, max_top);
                l.top = new as u16;
                l.follow = new >= max_top;
            }
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
            pending.func.code(pending.write_type),
        );
    }
}
