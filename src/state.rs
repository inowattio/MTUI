use crate::app::WriteType;
use crate::constants::keybind;
use crate::register::RegisterType;
use std::time::{Duration, Instant};

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<String>,
    pub value: Option<i32>,
    pub write_type: WriteType,
}

#[derive(Debug, PartialEq, Default)]
pub struct DumpParams {
    pub started: bool,
    pub total_batches: Option<u16>,
    pub completed_batches: u16,
    pub start_position: u16,
    pub position: u16,
    pub header_written: bool,
    pub error: Option<String>,
    pub register_type: RegisterType,
}

#[derive(Debug, Default, PartialEq)]
pub struct JumpParams {
    pub from: u16,
    pub to: u16,
    pub register_type: RegisterType,
}

#[derive(Debug, Default, PartialEq)]
pub struct LabelParams {
    pub position: u16,
    pub register_type: RegisterType,
    pub text: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ReadPanel {
    #[default]
    Main,
    Pinned,
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub window_start: u16,
    pub data_start: u16,
    pub panel: ReadPanel,
    /// Cursor index into the pinned list, and the top visible index, mirroring
    /// `position`/`window_start` but for the (scattered) Pinned panel.
    pub pinned_index: u16,
    pub pinned_top: u16,
    pub main_rows: Vec<String>,
    pub pinned_rows: Vec<String>,
    pub refresh_timer: Instant,
    pub register_type: RegisterType,
    pub read_duration: Option<Duration>,
    pub loading: bool,
    pub ascii_string: String,
    pub pinned_ascii_string: String,
    pub main_changed: Vec<bool>,
    pub pinned_changed: Vec<bool>,
}

impl Default for ReadParams {
    fn default() -> Self {
        Self {
            position: 0,
            window_start: 0,
            data_start: 0,
            panel: ReadPanel::Main,
            pinned_index: 0,
            pinned_top: 0,
            main_rows: no_data_rows(),
            pinned_rows: Vec::new(),
            refresh_timer: Instant::now(),
            register_type: Default::default(),
            read_duration: None,
            loading: false,
            ascii_string: String::new(),
            pinned_ascii_string: String::new(),
            main_changed: Vec::new(),
            pinned_changed: Vec::new(),
        }
    }
}

impl ReadParams {
    pub fn scroll_to_cursor(&mut self, rows: u16) {
        let rows = rows.max(1);
        if self.position < self.window_start {
            self.window_start = self.position;
        } else if self.position >= self.window_start.saturating_add(rows) {
            self.window_start = self.position.saturating_sub(rows - 1);
        }
    }

    pub fn toggle_panel(&mut self) {
        self.panel = match self.panel {
            ReadPanel::Main => ReadPanel::Pinned,
            ReadPanel::Pinned => ReadPanel::Main,
        };
    }

    /// Clamp the pinned cursor to a list of `len` entries and scroll `pinned_top`
    /// so the cursor stays within the visible `rows`.
    pub fn scroll_pinned(&mut self, rows: u16, len: u16) {
        let rows = rows.max(1);
        if len == 0 {
            self.pinned_index = 0;
            self.pinned_top = 0;
            return;
        }
        self.pinned_index = self.pinned_index.min(len - 1);
        if self.pinned_index < self.pinned_top {
            self.pinned_top = self.pinned_index;
        } else if self.pinned_index >= self.pinned_top.saturating_add(rows) {
            self.pinned_top = self.pinned_index.saturating_sub(rows - 1);
        }
        if self.pinned_top >= len {
            self.pinned_top = len.saturating_sub(rows).min(self.pinned_index);
        }
    }
}

pub fn no_data_text() -> String {
    format!("No data, press '{}' to refresh.", keybind::REFRESH)
}

pub fn no_data_rows() -> Vec<String> {
    vec![no_data_text()]
}

/// Truthful device reachability derived from the most recent read.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ConnectionStatus {
    #[default]
    Unknown,
    Reading,
    Connected,
    Error(String),
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Jump(JumpParams),
    Write(WriteParams),
    Help,
    Dump(DumpParams),
    Label(LabelParams),
}

pub enum StateTransition {
    Read,
    Jump,
    Write,
    Help,
    Dump,
    Label,
}
