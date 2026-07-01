use crate::app::WriteType;
use crate::compat::Instant;
use crate::custom::{CustomOp, CustomRepr, EnumEntry};
use crate::modbus::{DataBits, DeviceIdAccess, Parity, StopBits, WordOrder};
use crate::num_ops::{cycle, wrap_index};
use crate::register::{RegisterCell, RegisterType};
use serde::{Deserialize, Serialize};
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

macro_rules! popups {
    ( $( $variant:ident $( ( $payload:ty ) )? ),+ $(,)? ) => {
        #[derive(Debug, PartialEq)]
        pub enum Popup {
            $( $variant $( ( $payload ) )? ),+
        }

        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum PopupKind {
            $( $variant ),+
        }

        impl Popup {
            pub fn kind(&self) -> PopupKind {
                match self {
                    $( Popup::$variant { .. } => PopupKind::$variant ),+
                }
            }
        }
    };
}

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
    ScanNetwork,
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
    pub found: Vec<String>,
    pub scan_open: bool,
    pub scan_selected: u16,
    pub status: Option<StatusMessage>,
    pub previous: Option<ReadParams>,
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn local_subnet_prefix() -> Option<String> {
    match local_ip_address::local_ip().ok()? {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => {
            let [a, b, c, _] = ip.octets();
            Some(format!("{a}.{b}.{c}."))
        }
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn local_subnet_prefix() -> Option<String> {
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
            found: Vec::new(),
            scan_open: false,
            scan_selected: 0,
            status: None,
            previous: None,
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
            InterfaceKind::Network => fields.extend([Ip, NetPort, ScanNetwork]),
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
    pub result: Option<StatusMessage>,
    pub value: Option<i64>,
    pub write_type: WriteType,
    pub bit_cursor: u16,
    pub force_multiple: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct LabelParams {
    pub position: u16,
    pub register_type: RegisterType,
    pub text: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct DumpParams {
    pub result: Option<StatusMessage>,
}

#[derive(Debug, Default, PartialEq)]
pub struct ImportParams {
    pub pins: usize,
    pub labels: usize,
    pub rules: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct DeviceIdParams {
    pub access: DeviceIdAccess,
    pub objects: Vec<(u8, String)>,
    pub status: Option<StatusMessage>,
    pub loading: bool,
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RawField {
        Code,
        Data,
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct RawParams {
    pub code: String,
    pub data: String,
    pub selected: u16,
    pub response: Option<String>,
    pub status: Option<StatusMessage>,
}

fn clamp_pick<const N: usize, T: Copy>(selected: u16, all: &[T; N]) -> T {
    all[(selected as usize).min(N - 1)]
}

impl RawParams {
    pub fn current_field(&self) -> RawField {
        clamp_pick(self.selected, &RawField::ALL)
    }
}

field_enum! {
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
        clamp_pick(self.selected, &CustomField::ALL)
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SweepField {
        From,
        To,
        Mode,
        Action,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepConfigParams {
    pub from: u16,
    pub to: u16,
    pub continuous: bool,
    pub selected: u16,
}

impl SweepConfigParams {
    pub fn current_field(&self) -> SweepField {
        clamp_pick(self.selected, &SweepField::ALL)
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

#[derive(Debug, Default, PartialEq)]
pub struct HelpParams {
    pub query: String,
    pub selected: u16,
}

#[derive(Debug, Default, PartialEq)]
pub struct ColumnsParams {
    pub query: String,
    pub selected: u16,
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

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
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

field_enum! {
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
        ApiSlaveOverride,
        StartupPanel,
        CycleHoldings,
        CycleInputs,
        CycleCoils,
        CycleDiscretes,
        IgnoreDirty,
        ClearPins,
        ClearLabels,
        ClearCustom,
        ShowContinuation,
        ShowFrameTime,
        EditKeybinds,
        Save,
        LoadConfig,
    }
}

impl SettingsField {
    pub fn is_text_input(self) -> bool {
        matches!(self, SettingsField::Name | SettingsField::LoadConfig)
    }

    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            SettingsField::ReadOnly
                | SettingsField::ApiSlaveOverride
                | SettingsField::LogWrites
                | SettingsField::ShowContinuation
                | SettingsField::ShowFrameTime
                | SettingsField::StartupPanel
                | SettingsField::IgnoreDirty
                | SettingsField::CycleHoldings
                | SettingsField::CycleInputs
                | SettingsField::CycleCoils
                | SettingsField::CycleDiscretes
        )
    }

    pub fn cycle_register_type(self) -> Option<RegisterType> {
        Some(match self {
            SettingsField::CycleHoldings => RegisterType::Holding,
            SettingsField::CycleInputs => RegisterType::Input,
            SettingsField::CycleCoils => RegisterType::Coil,
            SettingsField::CycleDiscretes => RegisterType::Discrete,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Ok,
    Warn,
    Err,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
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

#[derive(Debug, Default, PartialEq)]
pub struct SettingsParams {
    pub selected: u16,
    pub status: Option<StatusMessage>,
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
        self.kb_selected = wrap_index(self.kb_selected, count, !up);
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
        scroll_window(
            &mut self.kb_selected,
            &mut self.kb_top,
            Self::KB_VISIBLE,
            count,
        );
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

popups! {
    Help(HelpParams),
    Dump(DumpParams),
    Search(SearchParams),
    Label(LabelParams),
    Custom(CustomParams),
    Columns(ColumnsParams),
    Write(WriteParams),
    Slave(u16),
    Logs(LogsParams),
    SweepConfig(SweepConfigParams),
    Inspect,
    DeviceId(DeviceIdParams),
    Raw(RawParams),
    Import(ImportParams),
    Quit,
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub window_start: u16,
    pub col_offset: u16,
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
            status: None,
            status_at: Instant::now(),
        }
    }
}

impl ReadParams {
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

    pub fn toggle_panel(&mut self) {
        self.panel = cycle(&ReadPanel::ALL, self.panel, true);
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
    Reconnecting,
    Error(String),
}

impl ConnectionStatus {
    pub fn code(&self) -> u8 {
        match self {
            ConnectionStatus::Unknown => 0,
            ConnectionStatus::Reading => 1,
            ConnectionStatus::Connected => 2,
            ConnectionStatus::Reconnecting => 3,
            ConnectionStatus::Error(_) => 4,
        }
    }

    pub fn label_from_code(code: u8) -> &'static str {
        match code {
            1 => "reading",
            2 => "connected",
            3 => "reconnecting",
            4 => "error",
            _ => "unknown",
        }
    }

    pub fn code_serving(code: u8) -> bool {
        matches!(code, 0..=2)
    }
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
