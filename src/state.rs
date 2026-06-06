use crate::app::WriteType;
use crate::register::{RegisterCell, RegisterType};
use std::time::{Duration, Instant};

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<String>,
    pub value: Option<i32>,
    pub write_type: WriteType,
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

/// A modal overlay shown over the Read screen. Only one is open at a time, held
/// in `ReadParams.popup`.
#[derive(Debug, PartialEq)]
pub enum Popup {
    Help,
    Save(SaveParams),
    Dump(DumpParams),
    Search(SearchParams),
    Label(LabelParams),
    /// Column picker; the value is the cursor index into `Column::ALL`.
    Columns(u16),
    Write(WriteParams),
}

/// Lightweight, copyable tag of which popup is open (so the handler can decide
/// how to route a key without holding a borrow on the popup data).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Help,
    Save,
    Dump,
    Search,
    Label,
    Columns,
    Write,
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
    /// The open modal overlay, if any.
    pub popup: Option<Popup>,
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

/// The app is always on the Read screen; everything else is a `Popup` over it.
#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
}
