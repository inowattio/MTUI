use crate::app::WriteType;
use crate::compat::Instant;
use crate::custom::{CustomOp, CustomRepr, EnumEntry};
use crate::modbus::{DataBits, Parity, StopBits, WordOrder};
use crate::register::{RegisterCell, RegisterType};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceKind {
    Mock,
    Wired,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryField {
    Interface,
    Port,
    Baud,
    DataBits,
    Parity,
    StopBits,
    Ip,
    NetPort,
    SlaveId,
    ConnectTimeout,
    CommandTimeout,
    BetweenCommands,
    WordOrder,
    Connect,
}

#[derive(Debug, PartialEq)]
pub struct DiscoveryParams {
    pub interface: InterfaceKind,
    pub selected: u16,
    pub ports: Vec<String>,
    pub port_index: u16,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub ip: String,
    pub net_port: u16,
    pub slave_id: u8,
    pub connect_timeout_ms: u64,
    pub command_timeout_ms: u64,
    pub between_commands_ms: u64,
    pub word_order: WordOrder,
    pub status: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
fn local_subnet_prefix() -> Option<String> {
    match local_ip_address::local_ip().ok()? {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => {
            let [a, b, c, _] = ip.octets();
            Some(format!("{a}.{b}.{c}."))
        }
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
fn local_subnet_prefix() -> Option<String> {
    None
}

impl Default for DiscoveryParams {
    fn default() -> Self {
        Self {
            interface: InterfaceKind::Mock,
            selected: 0,
            ports: Vec::new(),
            port_index: 0,
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            ip: local_subnet_prefix().unwrap_or_else(|| "127.0.0.1".to_string()),
            net_port: 502,
            slave_id: 1,
            connect_timeout_ms: 1000,
            command_timeout_ms: 2000,
            between_commands_ms: 3,
            word_order: WordOrder::default(),
            status: None,
        }
    }
}

impl DiscoveryParams {
    pub fn fields(&self) -> Vec<DiscoveryField> {
        use DiscoveryField::*;
        let mut fields = vec![Interface];
        match self.interface {
            InterfaceKind::Mock => {}
            InterfaceKind::Wired => fields.extend([Port, Baud, DataBits, Parity, StopBits]),
            InterfaceKind::Network => fields.extend([Ip, NetPort]),
        }
        fields.extend([
            SlaveId,
            ConnectTimeout,
            CommandTimeout,
            BetweenCommands,
            WordOrder,
            Connect,
        ]);
        fields
    }

    pub fn current_field(&self) -> DiscoveryField {
        let fields = self.fields();
        let i = (self.selected as usize).min(fields.len() - 1);
        fields[i]
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
pub struct DumpParams {
    pub result: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomField {
    Repr,
    Ops,
    Enum,
    Decimals,
    Prefix,
    Suffix,
    Save,
    Remove,
}

impl CustomField {
    pub const ALL: [CustomField; 8] = [
        CustomField::Repr,
        CustomField::Ops,
        CustomField::Enum,
        CustomField::Decimals,
        CustomField::Prefix,
        CustomField::Suffix,
        CustomField::Save,
        CustomField::Remove,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub struct CustomParams {
    pub address: u16,
    pub register_type: RegisterType,
    pub repr: CustomRepr,
    pub ops: Vec<CustomOp>,
    pub enum_map: Vec<EnumEntry>,
    pub decimals: String,
    pub prefix: String,
    pub suffix: String,
    pub op_buffer: String,
    pub enum_buffer: String,
    pub selected: u16,
    pub existed: bool,
    pub error: Option<String>,
}

impl CustomParams {
    pub fn current_field(&self) -> CustomField {
        let i = (self.selected as usize).min(CustomField::ALL.len() - 1);
        CustomField::ALL[i]
    }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum ReadPanel {
    #[default]
    Main,
    Pinned,
    Labeled,
    Custom,
    Matrix,
}

impl ReadPanel {
    pub const ALL: [ReadPanel; 5] = [
        ReadPanel::Main,
        ReadPanel::Pinned,
        ReadPanel::Labeled,
        ReadPanel::Custom,
        ReadPanel::Matrix,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ReadPanel::Main => "Main",
            ReadPanel::Pinned => "Pinned",
            ReadPanel::Labeled => "Labeled",
            ReadPanel::Custom => "Custom",
            ReadPanel::Matrix => "Matrix",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    Name,
    RegistersBatch,
    AutoUpdate,
    HistoryCap,
    MatrixCols,
    ReadOnly,
    LogWrites,
    ApiPort,
    StartupPanel,
    IgnoreDirty,
    ClearPins,
    ClearLabels,
    ClearCustom,
    ShowContinuation,
    EditKeybinds,
    Save,
    LoadConfig,
}

impl SettingsField {
    pub const ALL: [SettingsField; 17] = [
        SettingsField::Name,
        SettingsField::RegistersBatch,
        SettingsField::AutoUpdate,
        SettingsField::HistoryCap,
        SettingsField::MatrixCols,
        SettingsField::ReadOnly,
        SettingsField::LogWrites,
        SettingsField::ApiPort,
        SettingsField::StartupPanel,
        SettingsField::IgnoreDirty,
        SettingsField::ClearPins,
        SettingsField::ClearLabels,
        SettingsField::ClearCustom,
        SettingsField::ShowContinuation,
        SettingsField::EditKeybinds,
        SettingsField::Save,
        SettingsField::LoadConfig,
    ];

    pub fn is_text_input(self) -> bool {
        matches!(self, SettingsField::Name | SettingsField::LoadConfig)
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct SettingsParams {
    pub selected: u16,
    pub status: Option<String>,
    pub load_path: String,
    pub previous: ReadParams,
    pub editing_keybinds: bool,
    pub kb_selected: u16,
    pub kb_top: u16,
    pub kb_capturing: bool,
}

impl SettingsParams {
    pub const KB_VISIBLE: u16 = 14;

    pub fn open_keybinds(&mut self) {
        self.editing_keybinds = true;
        self.kb_selected = 0;
        self.kb_top = 0;
        self.kb_capturing = false;
    }

    pub fn kb_move(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = if up {
            if self.kb_selected == 0 {
                count - 1
            } else {
                self.kb_selected - 1
            }
        } else {
            (self.kb_selected + 1) % count
        };
        self.kb_scroll_into_view(count);
    }

    pub fn kb_page(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = if up {
            self.kb_selected.saturating_sub(Self::KB_VISIBLE)
        } else {
            (self.kb_selected + Self::KB_VISIBLE).min(count - 1)
        };
        self.kb_scroll_into_view(count);
    }

    fn kb_scroll_into_view(&mut self, count: u16) {
        let visible = Self::KB_VISIBLE.min(count);
        if self.kb_selected < self.kb_top {
            self.kb_top = self.kb_selected;
        } else if self.kb_selected >= self.kb_top + visible {
            self.kb_top = self.kb_selected + 1 - visible;
        }
        self.kb_top = self.kb_top.min(count.saturating_sub(visible));
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct LogsParams {
    pub path: String,
    pub lines: Vec<String>,
    pub top: u16,
}

impl LogsParams {
    pub const VISIBLE: u16 = 16;

    pub fn scroll(&mut self, delta: i32) {
        let len = self.lines.len() as i32;
        let max_top = (len - Self::VISIBLE as i32).max(0);
        self.top = (self.top as i32 + delta).clamp(0, max_top) as u16;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll(i32::MAX);
    }
}

#[derive(Debug, PartialEq)]
pub enum Popup {
    Help,
    Dump(DumpParams),
    Search(SearchParams),
    Label(LabelParams),
    Custom(CustomParams),
    Columns(u16),
    Write(WriteParams),
    Slave(u16),
    Logs(LogsParams),
    Inspect,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Help,
    Dump,
    Search,
    Label,
    Custom,
    Columns,
    Write,
    Slave,
    Logs,
    Inspect,
    Quit,
}

impl Popup {
    pub fn kind(&self) -> PopupKind {
        match self {
            Popup::Help => PopupKind::Help,
            Popup::Dump(_) => PopupKind::Dump,
            Popup::Search(_) => PopupKind::Search,
            Popup::Label(_) => PopupKind::Label,
            Popup::Custom(_) => PopupKind::Custom,
            Popup::Columns(_) => PopupKind::Columns,
            Popup::Write(_) => PopupKind::Write,
            Popup::Slave(_) => PopupKind::Slave,
            Popup::Logs(_) => PopupKind::Logs,
            Popup::Inspect => PopupKind::Inspect,
            Popup::Quit => PopupKind::Quit,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub window_start: u16,
    pub panel: ReadPanel,
    pub pinned_index: u16,
    pub pinned_top: u16,
    pub popup: Option<Popup>,
    pub graph: bool,
    pub graph_dword: bool,
    pub refresh_timer: Instant,
    pub register_type: RegisterType,
    pub read_duration: Option<Duration>,
    pub loading: bool,
    pub read_error: Option<String>,
}

impl Default for ReadParams {
    fn default() -> Self {
        Self {
            position: 0,
            window_start: 0,
            panel: ReadPanel::Main,
            pinned_index: 0,
            pinned_top: 0,
            popup: None,
            graph: false,
            graph_dword: false,
            refresh_timer: Instant::now(),
            register_type: Default::default(),
            read_duration: None,
            loading: false,
            read_error: None,
        }
    }
}

impl ReadParams {
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

    pub fn toggle_panel(&mut self) {
        self.panel = match self.panel {
            ReadPanel::Main => ReadPanel::Pinned,
            ReadPanel::Pinned => ReadPanel::Labeled,
            ReadPanel::Labeled => ReadPanel::Custom,
            ReadPanel::Custom => ReadPanel::Matrix,
            ReadPanel::Matrix => ReadPanel::Main,
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
pub struct LogViewParams {
    pub top: u16,
    pub follow: bool,
    pub previous: ReadParams,
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Discovery(DiscoveryParams),
    Settings(SettingsParams),
    Logs(LogViewParams),
}
