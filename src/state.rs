use crate::app::WriteType;
use crate::modbus::{DataBits, Parity, StopBits};
use crate::register::{RegisterCell, RegisterType};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryStage {
    Select,
    Wired,
    Network,
}

#[derive(Debug, PartialEq)]
pub struct DiscoveryParams {
    pub stage: DiscoveryStage,
    pub selected: u16,
    pub ports: Vec<String>,
    pub port_index: u16,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub ip: String,
    pub port: String,
    pub status: Option<String>,
}

impl Default for DiscoveryParams {
    fn default() -> Self {
        Self {
            stage: DiscoveryStage::Select,
            selected: 0,
            ports: Vec::new(),
            port_index: 0,
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            ip: "127.0.0.1".to_string(),
            port: "502".to_string(),
            status: None,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<String>,
    pub value: Option<i64>,
    pub write_type: WriteType,
    pub bit_cursor: u16,
}

#[derive(Debug, Default, PartialEq)]
pub struct LabelParams {
    pub position: u16,
    pub register_type: RegisterType,
    pub text: String,
    pub result: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct SaveParams {
    pub result: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct DumpParams {
    pub result: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct SearchParams {
    pub query: String,
    pub matches: Vec<(RegisterCell, String)>,
    pub selected: u16,
    pub top: u16,
}

impl SearchParams {
    pub fn scroll(&mut self, rows: u16) {
        let len = self.matches.len() as u16;
        scroll_window(&mut self.selected, &mut self.top, rows, len);
    }
}

fn scroll_window(cursor: &mut u16, top: &mut u16, rows: u16, len: u16) {
    let rows = rows.max(1);
    if len == 0 {
        *cursor = 0;
        *top = 0;
        return;
    }
    *cursor = (*cursor).min(len - 1);
    if *cursor < *top {
        *top = *cursor;
    } else if *cursor >= top.saturating_add(rows) {
        *top = cursor.saturating_sub(rows - 1);
    }
    if *top >= len {
        *top = len.saturating_sub(rows).min(*cursor);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ReadPanel {
    #[default]
    Main,
    Pinned,
}

#[derive(Debug, PartialEq)]
pub enum Popup {
    Help,
    Save(SaveParams),
    Dump(DumpParams),
    Search(SearchParams),
    Label(LabelParams),
    Columns(u16),
    Write(WriteParams),
    Slave(u16),
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Help,
    Save,
    Dump,
    Search,
    Label,
    Columns,
    Write,
    Slave,
    Quit,
}

impl Popup {
    pub fn kind(&self) -> PopupKind {
        match self {
            Popup::Help => PopupKind::Help,
            Popup::Save(_) => PopupKind::Save,
            Popup::Dump(_) => PopupKind::Dump,
            Popup::Search(_) => PopupKind::Search,
            Popup::Label(_) => PopupKind::Label,
            Popup::Columns(_) => PopupKind::Columns,
            Popup::Write(_) => PopupKind::Write,
            Popup::Slave(_) => PopupKind::Slave,
            Popup::Quit => PopupKind::Quit,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub window_start: u16,
    pub data_start: u16,
    pub panel: ReadPanel,
    pub pinned_index: u16,
    pub pinned_top: u16,
    pub popup: Option<Popup>,
    pub graph: bool,
    pub graph_dword: bool,
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
            popup: None,
            graph: false,
            graph_dword: false,
            main_rows: Vec::new(),
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

    pub fn scroll_pinned(&mut self, rows: u16, len: u16) {
        scroll_window(&mut self.pinned_index, &mut self.pinned_top, rows, len);
    }
}

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
    Discovery(DiscoveryParams),
}
