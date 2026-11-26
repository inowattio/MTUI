use crate::compat::Instant;
use crate::config::Column;
use crate::register::{RegisterCell, RegisterType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

macro_rules! field_enum {
    ( $(#[$meta:meta])* $vis:vis enum $name:ident { $( $(#[$vmeta:meta])* $variant:ident ),+ $(,)? } ) => {
        $(#[$meta])*
        $vis enum $name { $( $(#[$vmeta])* $variant ),+ }
        impl $name {
            pub const ALL: [$name; field_enum!(@count $($variant)+)] = [$($name::$variant),+];
        }
    };
    (@count) => (0usize);
    (@count $head:ident $($tail:ident)*) => (1usize + field_enum!(@count $($tail)*));
}

mod custom;
mod discovery;
mod export;
mod functions;
mod import;
mod library;
mod popups;
mod settings;
mod unit;
mod views;
mod write;

pub use custom::*;
pub use discovery::*;
pub use export::*;
pub use functions::*;
pub use import::*;
pub use library::*;
pub use popups::*;
pub use settings::*;
pub use unit::*;
pub use views::*;
pub use write::*;

pub trait PopupPayload: Sized {
    fn from_popup(popup: &Popup) -> Option<&Self>;
    fn from_popup_mut(popup: &mut Popup) -> Option<&mut Self>;
}

macro_rules! popups {
    (@accessor $variant:ident ( $payload:ty )) => {
        impl PopupPayload for $payload {
            fn from_popup(popup: &Popup) -> Option<&Self> {
                match popup {
                    Popup::$variant(inner) => Some(inner),
                    _ => None,
                }
            }
            fn from_popup_mut(popup: &mut Popup) -> Option<&mut Self> {
                match popup {
                    Popup::$variant(inner) => Some(inner),
                    _ => None,
                }
            }
        }
    };
    (@accessor $variant:ident) => {};

    ( $( $variant:ident $( ( $payload:ty ) )? ),+ $(,)? ) => {
        #[derive(Debug, PartialEq)]
        pub enum Popup {
            $( $variant $( ( $payload ) )? ),+
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum PopupKind {
            $( $variant ),+
        }

        impl Popup {
            pub const fn kind(&self) -> PopupKind {
                match self {
                    $( Popup::$variant { .. } => PopupKind::$variant ),+
                }
            }
        }

        $( popups!(@accessor $variant $( ( $payload ) )? ); )+
    };
}

fn clamp_pick<const N: usize, T: Copy>(selected: u16, all: &[T; N]) -> T {
    all[(selected as usize).min(N - 1)]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenLayout {
    pub visible_rows: u16,
    pub h_max_offset: u16,
    pub v_max_offset: u16,
    pub search_rows: u16,
}

impl Default for ScreenLayout {
    fn default() -> Self {
        Self {
            visible_rows: 1,
            h_max_offset: 0,
            v_max_offset: 0,
            search_rows: 1,
        }
    }
}

pub fn scroll_window(cursor: &mut u16, top: &mut u16, rows: u16, len: u16) {
    let rows = rows.max(1);
    if len == 0 {
        *cursor = 0;
        *top = 0;
        return;
    }
    *cursor = (*cursor).min(len - 1);
    *top = (*top).min(len.saturating_sub(rows));
    if *cursor < *top {
        *top = *cursor;
    } else if *cursor >= top.saturating_add(rows) {
        *top = cursor.saturating_sub(rows - 1);
    }
}

pub fn center_window(cursor: &mut u16, top: &mut u16, rows: u16, len: u16) {
    let rows = rows.max(1);
    if len == 0 {
        *cursor = 0;
        *top = 0;
        return;
    }
    *cursor = (*cursor).min(len - 1);
    *top = cursor
        .saturating_sub(rows / 2)
        .min(len.saturating_sub(rows));
}

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ReadPanel {
        #[default]
        Main,
        Pinned,
        Labeled,
        Custom,
        Matrix,
    }
}

impl ReadPanel {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Pinned => "Pinned",
            Self::Labeled => "Labeled",
            Self::Custom => "Custom",
            Self::Matrix => "Matrix",
        }
    }

    pub const fn follows_position(self) -> bool {
        matches!(self, Self::Main | Self::Matrix)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Ok,
    Warn,
    Err,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMessage {
    pub text: String,
    pub kind: MessageKind,
}

impl StatusMessage {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: MessageKind::Ok,
        }
    }

    pub fn warn(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: MessageKind::Warn,
        }
    }

    pub fn err(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: MessageKind::Err,
        }
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: MessageKind::Info,
        }
    }
}

pub type Outcome = Result<String, String>;

impl From<Outcome> for StatusMessage {
    fn from(result: Outcome) -> Self {
        match result {
            Ok(text) => Self::ok(text),
            Err(text) => Self::err(text),
        }
    }
}

popups! {
    Discovery(DiscoveryParams),
    Help(HelpParams),
    About,
    Dump(DumpParams),
    Search(SearchParams),
    Label(LabelParams),
    Custom(CustomParams),
    Columns(ColumnsParams),
    Write(WriteParams),
    Unit(UnitParams),
    SweepConfig(SweepConfigParams),
    Inspect(InspectMode),
    Stats(StatsTab),
    Functions(FunctionsParams),
    Raw(RawParams),
    Import(ImportParams),
    NoLibraries,
    LibraryDisclaimer,
    LoadConfig(PathBuf),
    Quit,
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub window_start: u16,
    pub col_offset: u16,
    pub copy_column: Option<u16>,
    pub panel: ReadPanel,
    pub pinned_index: u16,
    pub pinned_top: u16,
    pub popup: Option<Popup>,
    pub graph: bool,
    pub graph_column: Column,
    pub graph_series: Vec<RegisterCell>,
    pub hex_letters: bool,
    pub refresh_timer: Instant,
    pub register_type: RegisterType,
    pub read_duration: Option<Duration>,
    pub loading: bool,
    pub read_started: Instant,
    pub read_error: Option<String>,
    pub status: Option<StatusMessage>,
    pub status_at: Instant,
}

const STATUS_TTL: Duration = Duration::from_secs(4);

impl Default for ReadParams {
    fn default() -> Self {
        Self {
            position: 0,
            window_start: 0,
            col_offset: 0,
            copy_column: None,
            panel: ReadPanel::Main,
            pinned_index: 0,
            pinned_top: 0,
            popup: None,
            graph: false,
            graph_column: Column::Custom,
            graph_series: Vec::new(),
            hex_letters: false,
            refresh_timer: Instant::now(),
            register_type: Default::default(),
            read_duration: None,
            loading: false,
            read_started: Instant::now(),
            read_error: None,
            status: None,
            status_at: Instant::now(),
        }
    }
}

impl ReadParams {
    pub fn finish_read(&mut self) {
        self.loading = false;
        self.refresh_timer = Instant::now();
    }

    pub fn set_status(&mut self, message: StatusMessage) {
        self.status = Some(message);
        self.status_at = Instant::now();
    }

    pub fn active_status(&self) -> Option<&StatusMessage> {
        self.status
            .as_ref()
            .filter(|_| self.status_at.elapsed() < STATUS_TTL)
    }

    pub fn scroll_to_cursor(&mut self, rows: u16, matrix_cols: u16) {
        let rows = rows.max(1);
        if self.panel == ReadPanel::Matrix {
            let cols = matrix_cols.max(1);
            let last_row = u16::MAX / cols;
            let max_start_row = last_row.saturating_sub(rows - 1);
            let row = self.position / cols;
            let start_row = row.saturating_sub(rows / 2).min(max_start_row);
            self.window_start = start_row.saturating_mul(cols);
            return;
        }
        let max_start = u16::MAX - (rows - 1);
        self.window_start = self.position.saturating_sub(rows / 2).min(max_start);
    }

    pub fn scroll_pinned(&mut self, rows: u16, len: u16) {
        center_window(&mut self.pinned_index, &mut self.pinned_top, rows, len);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConnectionStatus {
    #[default]
    Unknown,
    Reading,
    Connected,
    Reconnecting,
    Error(String),
}

impl ConnectionStatus {
    pub const fn code(&self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::Reading => 1,
            Self::Connected => 2,
            Self::Reconnecting => 3,
            Self::Error(_) => 4,
        }
    }

    pub const fn label_from_code(code: u8) -> &'static str {
        match code {
            1 => "reading",
            2 => "connected",
            3 => "reconnecting",
            4 => "error",
            _ => "unknown",
        }
    }

    pub const fn code_serving(code: u8) -> bool {
        matches!(code, 0..=2)
    }
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Settings(SettingsParams),
    Logs(LogViewParams),
    Diff(DiffViewParams),
    Library(LibraryViewParams),
}

#[cfg(test)]
mod tests {
    use super::{center_window, scroll_window};

    fn run(cursor: u16, top: u16, rows: u16, len: u16) -> (u16, u16) {
        let (mut cursor, mut top) = (cursor, top);
        scroll_window(&mut cursor, &mut top, rows, len);
        (cursor, top)
    }

    #[test]
    fn scroll_window_follows_cursor() {
        assert_eq!(run(25, 10, 10, 100), (25, 16));
        assert_eq!(run(5, 10, 10, 100), (5, 5));
        assert_eq!(run(12, 10, 10, 100), (12, 10));
    }

    #[test]
    fn scroll_window_clamps_to_shorter_list() {
        // Switching from a long panel to a short one must not leave the
        // window hanging past the end with only the last item visible
        assert_eq!(run(50, 45, 20, 4), (3, 0));
        assert_eq!(run(50, 45, 3, 10), (9, 7));
        assert_eq!(run(0, 0, 5, 0), (0, 0));
    }

    #[test]
    fn center_window_keeps_cursor_in_the_middle() {
        let center = |cursor: u16, top: u16, rows: u16, len: u16| {
            let (mut cursor, mut top) = (cursor, top);
            center_window(&mut cursor, &mut top, rows, len);
            (cursor, top)
        };
        assert_eq!(center(25, 10, 10, 100), (25, 20));
        assert_eq!(center(3, 10, 10, 100), (3, 0));
        assert_eq!(center(97, 0, 10, 100), (97, 90));
        assert_eq!(center(50, 45, 20, 4), (3, 0));
        assert_eq!(center(0, 0, 5, 0), (0, 0));
    }
}
