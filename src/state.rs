use crate::app::WriteType;
use crate::compat::Instant;
use crate::config::Column;
use crate::custom::{BitEntry, CustomOp, CustomRepr, EnumEntry};
use crate::modbus::{
    DataBits, DeviceConfig, DeviceIdAccess, Interface, InterfaceSerialParams, InterfaceTcpParams,
    Parity, StopBits, WordOrder,
};
use crate::num_ops::wrap_index;
use crate::register::{RegisterCell, RegisterType};
use crate::writes_log::WriteEntry;
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
            pub fn kind(&self) -> PopupKind {
                match self {
                    $( Popup::$variant { .. } => PopupKind::$variant ),+
                }
            }
        }

        $( popups!(@accessor $variant $( ( $payload ) )? ); )+
    };
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum InterfaceKind {
        Mock,
        Serial,
        Tcp,
        RtuOverTcp,
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum ScanMethod {
        #[default]
        Ping,
        Port,
    }
}

impl ScanMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ping => "ICMP ping",
            Self::Port => "TCP port",
        }
    }
}

impl InterfaceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mock => "Mock",
            Self::Serial => "Serial",
            Self::Tcp => "TCP",
            Self::RtuOverTcp => "RTU over TCP",
        }
    }

    pub fn uses_tcp(self) -> bool {
        matches!(self, Self::Tcp | Self::RtuOverTcp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryField {
    Interface,
    UnitId,
    ConnectTimeout,
    CommandTimeout,
    BetweenCommands,
    WordOrder,
    Connect,
    Port(usize),
    CustomPath,
    Baud,
    DataBits,
    Parity,
    StopBits,
    Ip,
    NetPort,
    ScanMethod,
    ScanNetwork,
    Found(usize),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiscoveryColumn {
    #[default]
    Common,
    Side,
}

#[derive(Debug, PartialEq)]
pub struct DiscoveryParams {
    pub interface: InterfaceKind,
    pub column: DiscoveryColumn,
    pub selected: u16,
    pub side_selected: u16,
    pub ports: Vec<String>,
    pub ports_pending: bool,
    pub port_index: u16,
    pub custom_path: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
    pub ip: String,
    pub net_port: u16,
    pub scan_method: ScanMethod,
    pub unit_id: u8,
    pub connect_timeout_ms: u64,
    pub command_timeout_ms: u64,
    pub between_commands_ms: u64,
    pub word_order: WordOrder,
    pub found: Vec<String>,
    pub status: Option<StatusMessage>,
}

impl Default for DiscoveryParams {
    fn default() -> Self {
        Self {
            interface: InterfaceKind::Mock,
            column: DiscoveryColumn::Common,
            selected: 0,
            side_selected: 0,
            ports: Vec::new(),
            ports_pending: false,
            port_index: 0,
            custom_path: String::new(),
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            ip: "127.0.0.1".to_string(),
            net_port: 502,
            scan_method: ScanMethod::default(),
            unit_id: 1,
            connect_timeout_ms: 1000,
            command_timeout_ms: 2000,
            between_commands_ms: 3,
            word_order: WordOrder::default(),
            found: Vec::new(),
            status: None,
        }
    }
}

impl DiscoveryParams {
    pub const COMMON: [DiscoveryField; 7] = [
        DiscoveryField::Interface,
        DiscoveryField::UnitId,
        DiscoveryField::ConnectTimeout,
        DiscoveryField::CommandTimeout,
        DiscoveryField::BetweenCommands,
        DiscoveryField::WordOrder,
        DiscoveryField::Connect,
    ];

    pub fn side_fields(&self) -> Vec<DiscoveryField> {
        use DiscoveryField::*;
        match self.interface {
            InterfaceKind::Mock => Vec::new(),
            InterfaceKind::Serial => (0..self.ports.len())
                .map(Port)
                .chain([CustomPath, Baud, DataBits, Parity, StopBits])
                .collect(),
            InterfaceKind::Tcp | InterfaceKind::RtuOverTcp => {
                [Ip, NetPort, ScanMethod, ScanNetwork]
                    .into_iter()
                    .chain((0..self.found.len()).map(Found))
                    .collect()
            }
        }
    }

    pub fn custom_path_active(&self) -> bool {
        !self.custom_path.trim().is_empty()
    }

    pub const BAUD_PRESETS: [u32; 11] = [
        1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
    ];

    pub fn is_preset_baud(&self) -> bool {
        Self::BAUD_PRESETS.contains(&self.baud_rate)
    }

    pub fn cycle_scan_method(&mut self, forward: bool) {
        self.scan_method = crate::num_ops::cycle(&ScanMethod::ALL, self.scan_method, forward);
    }

    pub fn cycle_baud(&mut self, forward: bool) {
        let presets = Self::BAUD_PRESETS;
        let current = self.baud_rate;
        self.baud_rate = if forward {
            presets
                .iter()
                .copied()
                .find(|&rate| rate > current)
                .unwrap_or(presets[0])
        } else {
            presets
                .iter()
                .rev()
                .copied()
                .find(|&rate| rate < current)
                .unwrap_or(presets[presets.len() - 1])
        };
    }

    pub fn serial_path(&self) -> Option<String> {
        if self.custom_path_active() {
            return Some(self.custom_path.trim().to_string());
        }
        self.ports.get(self.port_index as usize).cloned()
    }

    fn network_params(&self) -> InterfaceTcpParams {
        InterfaceTcpParams {
            ip: self.ip.clone(),
            port: self.net_port,
        }
    }

    pub fn device_config(&self) -> DeviceConfig {
        let interface = match self.interface {
            InterfaceKind::Mock => Interface::Mock,
            InterfaceKind::Serial => Interface::Serial(InterfaceSerialParams {
                path: self.serial_path().unwrap_or_default(),
                baud_rate: self.baud_rate,
                data_bits: self.data_bits,
                parity: self.parity,
                stop_bits: self.stop_bits,
            }),
            InterfaceKind::Tcp => Interface::Tcp(self.network_params()),
            InterfaceKind::RtuOverTcp => Interface::RtuOverTcp(self.network_params()),
        };
        DeviceConfig {
            interface,
            unit_id: self.unit_id,
            connect_timeout_ms: self.connect_timeout_ms,
            request_timeout_ms: self.command_timeout_ms,
            request_gap_ms: self.between_commands_ms,
            word_order: self.word_order,
        }
    }

    pub fn current_field(&self) -> DiscoveryField {
        if self.column == DiscoveryColumn::Side {
            let side = self.side_fields();
            let index = (self.side_selected as usize).min(side.len().saturating_sub(1));
            if let Some(&field) = side.get(index) {
                return field;
            }
        }
        clamp_pick(self.selected, &Self::COMMON)
    }

    pub fn move_cursor(&mut self, down: bool) {
        match self.column {
            DiscoveryColumn::Common => {
                self.selected = wrap_index(self.selected, Self::COMMON.len() as u16, down);
            }
            DiscoveryColumn::Side => {
                let count = self.side_fields().len() as u16;
                self.side_selected = wrap_index(self.side_selected, count, down);
            }
        }
    }

    pub fn toggle_column(&mut self) {
        self.column = match self.column {
            DiscoveryColumn::Common if !self.side_fields().is_empty() => DiscoveryColumn::Side,
            _ => DiscoveryColumn::Common,
        };
        self.clamp_side();
    }

    pub fn set_interface(&mut self, interface: InterfaceKind) {
        self.interface = interface;
        self.side_selected = 0;
        self.clamp_side();
    }

    pub fn set_found(&mut self, found: Vec<String>) {
        self.found = found;
        self.clamp_side();
    }

    pub fn set_ports(&mut self, ports: Vec<String>) {
        let before = self.ports.len() as u16;
        let after = ports.len() as u16;
        if self.interface == InterfaceKind::Serial && self.side_selected >= before {
            self.side_selected = self.side_selected - before + after;
        }
        if let Some(i) = ports.iter().position(|p| p == self.custom_path.trim()) {
            self.port_index = i as u16;
            self.custom_path.clear();
        }
        self.ports = ports;
        self.ports_pending = false;
        self.clamp_side();
    }

    pub fn focus_connect(&mut self) {
        self.column = DiscoveryColumn::Common;
        self.selected = Self::COMMON
            .iter()
            .position(|&f| f == DiscoveryField::Connect)
            .map_or(0, |i| i as u16);
    }

    fn clamp_side(&mut self) {
        let count = self.side_fields().len() as u16;
        if count == 0 {
            self.column = DiscoveryColumn::Common;
        } else {
            self.side_selected = self.side_selected.min(count - 1);
        }
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
    pub h_offset: u16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RawField {
    #[default]
    Code,
    Data,
}

#[derive(Debug, Default, PartialEq)]
pub struct RawParams {
    pub code: String,
    pub data: String,
    pub field: RawField,
    pub response: Option<String>,
    pub status: Option<StatusMessage>,
}

fn clamp_pick<const N: usize, T: Copy>(selected: u16, all: &[T; N]) -> T {
    all[(selected as usize).min(N - 1)]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomField {
    Repr,
    WordOrder,
    Next,
    Ops,
    Enum,
    Bits,
    Decimals,
    Prefix,
    Suffix,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomParams {
    pub address: u16,
    pub register_type: RegisterType,
    pub repr: CustomRepr,
    pub word_order: Option<WordOrder>,
    pub next: Vec<u16>,
    pub ops: Vec<CustomOp>,
    pub enum_map: Vec<EnumEntry>,
    pub bits: Vec<BitEntry>,
    pub decimals: String,
    pub prefix: String,
    pub suffix: String,
    pub op_buffer: String,
    pub enum_buffer: String,
    pub bit_buffer: String,
    pub next_buffer: String,
    pub selected: u16,
    pub existed: bool,
    pub error: Option<String>,
}

impl CustomParams {
    pub fn fields(&self) -> Vec<CustomField> {
        use CustomField::*;
        let multi = self.repr.register_count() > 1;
        let bits_active = !self.bits.is_empty();

        let mut fields = vec![Repr];
        if multi || self.word_order.is_some() {
            fields.push(WordOrder);
        }
        if multi || !self.next.is_empty() {
            fields.push(Next);
        }
        if !bits_active || !self.ops.is_empty() {
            fields.push(Ops);
        }
        fields.push(Enum);
        fields.push(Bits);
        if !bits_active || !self.decimals.is_empty() {
            fields.push(Decimals);
        }
        fields.push(Prefix);
        fields.push(Suffix);
        fields
    }

    pub fn current_field(&self) -> CustomField {
        let fields = self.fields();
        fields[(self.selected as usize).min(fields.len() - 1)]
    }

    pub fn reselect(&mut self, field: CustomField) {
        let fields = self.fields();
        self.selected = match fields.iter().position(|&f| f == field) {
            Some(i) => i as u16,
            None => self.selected.min(fields.len() as u16 - 1),
        };
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitField {
    Id,
    From,
    To,
    Mode,
    Repr,
    Exceptions,
    Scan,
    Hit(usize),
}

impl UnitField {
    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            Self::Mode | Self::Repr | Self::Exceptions
        )
    }
}

#[derive(Debug, PartialEq)]
pub struct UnitScanHit {
    pub unit_id: u8,
    pub result: Result<Vec<u16>, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanState {
    #[default]
    Idle,
    Probing,
    Done,
    Stopped,
    Failed,
}

#[derive(Debug, PartialEq)]
pub struct UnitParams {
    pub id: u8,
    pub selected: u16,
    pub from: u8,
    pub to: u8,
    pub stop_at_first: bool,
    pub ascii: bool,
    pub show_exceptions: bool,
    pub scan: ScanState,
    pub current: u8,
    pub register_type: RegisterType,
    pub address: u16,
    pub amount: u16,
    pub hits: Vec<UnitScanHit>,
    pub status: Option<StatusMessage>,
}

impl Default for UnitParams {
    fn default() -> Self {
        Self {
            id: 0,
            selected: 0,
            from: 1,
            to: 247,
            stop_at_first: false,
            ascii: false,
            show_exceptions: true,
            scan: ScanState::Idle,
            current: 0,
            register_type: RegisterType::default(),
            address: 0,
            amount: 1,
            hits: Vec::new(),
            status: None,
        }
    }
}

impl UnitParams {
    const FIXED: [UnitField; 7] = [
        UnitField::Id,
        UnitField::From,
        UnitField::To,
        UnitField::Mode,
        UnitField::Repr,
        UnitField::Exceptions,
        UnitField::Scan,
    ];

    pub fn visible_hits(&self) -> impl Iterator<Item = (usize, &UnitScanHit)> {
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| self.show_exceptions || hit.result.is_ok())
    }

    pub fn fields(&self) -> Vec<UnitField> {
        let mut fields = Self::FIXED.to_vec();
        fields.extend(self.visible_hits().map(|(i, _)| UnitField::Hit(i)));
        fields
    }

    pub fn current_field(&self) -> UnitField {
        let fields = self.fields();
        fields[(self.selected as usize).min(fields.len() - 1)]
    }

    pub fn switch_column(&mut self) {
        let first_hit = Self::FIXED.len() as u16;
        if matches!(self.current_field(), UnitField::Hit(_)) {
            self.selected = 0;
        } else if self.fields().len() as u16 > first_hit {
            self.selected = first_hit;
        }
    }

    pub fn suspended(mut self) -> Self {
        if self.active() {
            self.scan = ScanState::Stopped;
        }
        self.status = None;
        self
    }

    pub fn resumed(
        mut self,
        id: u8,
        register_type: RegisterType,
        address: u16,
        amount: u16,
    ) -> Self {
        self.id = id;
        self.register_type = register_type;
        self.address = address;
        self.amount = amount;
        self
    }

    pub fn active(&self) -> bool {
        self.scan == ScanState::Probing
    }

    pub fn scanned(&self) -> bool {
        self.scan != ScanState::Idle
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    pub cell: RegisterCell,
    pub text: String,
    pub labeled: bool,
}

impl SearchMatch {
    pub fn label(cell: RegisterCell, text: String) -> Self {
        Self {
            cell,
            text,
            labeled: true,
        }
    }

    pub fn hint(cell: RegisterCell, text: String) -> Self {
        Self {
            cell,
            text,
            labeled: false,
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct SearchParams {
    pub query: String,
    pub matches: Vec<SearchMatch>,
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
    *top = (*top).min(len.saturating_sub(rows));
    if *cursor < *top {
        *top = *cursor;
    } else if *cursor >= top.saturating_add(rows) {
        *top = cursor.saturating_sub(rows - 1);
    }
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
    pub fn name(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Pinned => "Pinned",
            Self::Labeled => "Labeled",
            Self::Custom => "Custom",
            Self::Matrix => "Matrix",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Number,
    Text,
    Toggle,
    Action,
    Color,
    CycleType(RegisterType),
    CyclePanel(ReadPanel),
}

macro_rules! settings_fields {
    (
        $( $category:ident {
            $( [ $( $field:ident : $kind:expr => $label:literal, $description:literal ),+ $(,)? ] ),*
            $(,)?
        } )+
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SettingsField { $( $( $( $field, )+ )* )+ }

        impl SettingsField {
            pub fn label(self) -> &'static str {
                match self { $( $( $( SettingsField::$field => $label, )+ )* )+ }
            }

            pub fn description(self) -> &'static str {
                match self { $( $( $( SettingsField::$field => $description, )+ )* )+ }
            }

            pub fn kind(self) -> FieldKind {
                use FieldKind::*;
                match self { $( $( $( SettingsField::$field => $kind, )+ )* )+ }
            }
        }

        impl SettingsCategory {
            pub fn groups(self) -> &'static [&'static [SettingsField]] {
                match self {
                    $( SettingsCategory::$category => &[ $( &[ $( SettingsField::$field ),+ ] ),* ], )+
                }
            }
        }
    };
}

settings_fields! {
    Data {
        [
            BatchSize: Number => "Batch size", "How many registers each read request fetches around the cursor",
            BatchAnchor: Toggle => "Batch anchor", "Where the cursor sits inside the read batch: start, middle or end",
            ReadFullCustoms: Toggle => "Read full custom values", "Also read every register a custom rule spans, even outside the batch",
            CustomBatchByRegisters: Toggle => "Custom batch by registers", "In the Custom panel, size the batch by registers instead of rules",
        ],
        [
            RefreshInterval: Number => "Auto-refresh (ms)", "Delay between automatic reads, 0 turns auto-refresh off",
            ReconnectOnTimeout: Toggle => "Reconnect on timeout", "Reconnect to the device after a read times out",
            ReadOnly: Toggle => "Read-only", "Refuse all writes from the UI and the API",
            WriteLogEnabled: Toggle => "Log writes to file", "Append every write to a log file",
            WriteLogDirectory: Text => "Write log folder", "Folder for the write log, empty uses the config file's folder",
        ],
        [
            GraphHistory: Number => "Graph history", "Samples kept per register for the value graph",
            MatrixColumns: Number => "Matrix columns", "Registers per row in the Matrix panel, auto fits as many as the screen shows",
        ],
        [
            CycleHoldings: CycleType(RegisterType::Holding) => "Cycle holdings", "Include holding registers when cycling register types",
            CycleInputs: CycleType(RegisterType::Input) => "Cycle inputs", "Include input registers when cycling register types",
            CycleCoils: CycleType(RegisterType::Coil) => "Cycle coils", "Include coils when cycling register types",
            CycleDiscretes: CycleType(RegisterType::Discrete) => "Cycle discretes", "Include discrete inputs when cycling register types",
        ],
        [
            ShowMockDevice: Toggle => "Show mock device", "Offer the built-in mock device in the Device popup",
            SavePositionOnExit: Toggle => "Save position on exit", "Store the cursor position as the startup position when quitting",
            StartupPanel: Toggle => "Startup panel", "Panel opened on start",
            StartupType: Toggle => "Startup type", "Register type selected on start",
            StartupAddress: Number => "Startup address", "Address the cursor starts on",
        ],
    }
    Api {
        [
            ApiEnabled: Toggle => "API server", "Serve the HTTP API while the app runs",
            ApiPort: Number => "API port", "Port for the HTTP API, 0 picks any free port",
            ApiUnitIdOverride: Toggle => "API unit id override", "Let API requests target a unit id other than the configured one",
        ],
    }
    Display {
        [
            ShowClock: Toggle => "Show clock", "Show the current time in the bottom bar",
            ShowFrameTime: Toggle => "Show frame time", "Show how long each frame takes to render",
            ShowRam: Toggle => "Show RAM usage", "Show the memory used by the application",
            ShowConnectionLabel: Toggle => "Show connection label", "Show the connection state as a word next to the refresh countdown",
            ShowAsciiStrip: Toggle => "Show ASCII strip", "Show the read registers decoded as an ASCII string",
            ShowInactiveTabs: Toggle => "Show inactive tabs", "Show every panel and register type tab, not just the active one",
            FilterPanelsByType: Toggle => "Filter panels by type", "In Pinned, Labeled and Custom, list only the current register type",
        ],
        [
            CyclePinned: CyclePanel(ReadPanel::Pinned) => "Cycle pinned", "Include the Pinned panel when cycling panels",
            CycleLabeled: CyclePanel(ReadPanel::Labeled) => "Cycle labeled", "Include the Labeled panel when cycling panels",
            CycleCustom: CyclePanel(ReadPanel::Custom) => "Cycle custom", "Include the Custom panel when cycling panels",
            CycleMatrix: CyclePanel(ReadPanel::Matrix) => "Cycle matrix", "Include the Matrix panel when cycling panels",
        ],
        [
            TimeMode: Toggle => "Time format", "Show the read time as a timestamp or as how long ago it was",
            AddressMode: Toggle => "Address format", "Show addresses in decimal or hexadecimal",
            LabelWidth: Number => "Label width", "Characters the label column takes, auto fits the longest label",
            CustomWidth: Number => "Custom width", "Characters the custom column takes, auto fits the longest value",
        ],
        [
            ShowReadWindow: Toggle => "Show read window", "Highlight the address range covered by the current read batch",
            ShowMatrixContext: Toggle => "Show matrix context", "Show the custom value and label of the selected register under the Matrix panel",
            GraphTimeAxis: Toggle => "Graph time axis", "Plot the graph against time instead of sample count",
            ChangedExpiry: Number => "Changed highlight (ms)", "How long a changed value stays highlighted, 0 never clears it",
            ShowRuleContinuation: Toggle => "Show \"part of\" marker", "Mark registers that belong to a multi-register custom rule",
        ],
        [
            PaddingHorizontal: Number => "Horizontal padding", "Empty columns kept on both sides of the interface",
            PaddingVertical: Number => "Vertical padding", "Empty rows kept above and below the interface",
        ],
    }
    Theme {
        [ThemePreset: Toggle => "Preset", "Switch between the built-in color schemes"],
        [
            ThemeBorder: Color => "Frame border", "Color of the frame borders",
            ThemeAccent: Color => "Accent", "Color for titles, keys and highlights",
            ThemeText: Color => "Text", "Main text color",
            ThemeBackground: Color => "Background", "Background color",
            ThemeDim: Color => "Muted", "Color for secondary and muted text",
            ThemeChanged: Color => "Changed value", "Color for values that changed recently",
            ThemeZebra: Color => "Zebra stripe", "Background of alternating table rows",
            ThemeOk: Color => "Success", "Color for success and connected states",
            ThemeWarning: Color => "Warning", "Color for warnings",
            ThemeError: Color => "Error", "Color for errors",
            ThemeSelectedText: Color => "Selected text", "Text color of the selected row",
            ThemeSelectedBackground: Color => "Selected background", "Background color of the selected row",
        ],
    }
    Keybinds {}
    Config {
        [
            Name: Text => "Config name", "Name shown in the title bar for this configuration",
            SkipUnsavedWarning: Toggle => "Ignore unsaved warning", "Quit or switch configuration without asking about unsaved changes",
        ],
        [
            ClearPins: Action => "Clear pinned registers", "Remove every pinned register",
            ClearLabels: Action => "Clear labels", "Remove every label",
            ClearCustom: Action => "Clear custom rules", "Remove every custom rule",
            CopyData: Action => "Copy registers", "Copy pins, labels and custom rules as JSON, paste into another MTUI to import",
        ],
        [
            CopyConfig: Action => "Copy configuration", "Copy the whole configuration as JSON, as Save would write it",
            Save: Action => "Save configuration", "Write the current settings to the configuration file",
            LoadConfig: Text => "Load configuration", "Path of a configuration file to load now",
            NextConfig: Text => "Next configuration", "Configuration file loaded by the cycle config key",
        ],
    }
    Search {}
}

impl SettingsField {
    pub fn matches(self, query: &str) -> bool {
        let label = self.label().to_lowercase();
        let description = self.description().to_lowercase();
        query.split_whitespace().all(|word| {
            label.contains(&word.to_lowercase()) || description.contains(&word.to_lowercase())
        })
    }

    pub fn is_text_input(self) -> bool {
        self.kind() == FieldKind::Text
    }

    pub fn is_toggle(self) -> bool {
        matches!(
            self.kind(),
            FieldKind::Toggle | FieldKind::CycleType(_) | FieldKind::CyclePanel(_)
        )
    }

    pub fn is_startup(self) -> bool {
        matches!(
            self,
            Self::StartupPanel
                | Self::StartupType
                | Self::StartupAddress
        )
    }

    pub fn cycle_register_type(self) -> Option<RegisterType> {
        match self.kind() {
            FieldKind::CycleType(register_type) => Some(register_type),
            _ => None,
        }
    }

    pub fn cycle_panel(self) -> Option<ReadPanel> {
        match self.kind() {
            FieldKind::CyclePanel(panel) => Some(panel),
            _ => None,
        }
    }

    pub fn is_theme_color(self) -> bool {
        self.kind() == FieldKind::Color
    }

    pub fn is_action(self) -> bool {
        self.kind() == FieldKind::Action
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SettingsCategory {
        Data,
        Api,
        Display,
        Theme,
        Keybinds,
        Config,
        Search,
    }
}

impl SettingsCategory {
    pub fn label(self) -> &'static str {
        match self {
            Self::Data => "Data",
            Self::Api => "API",
            Self::Display => "Display",
            Self::Theme => "Theme",
            Self::Keybinds => "Keybinds",
            Self::Config => "Config",
            Self::Search => "Search",
        }
    }

    pub fn search(query: &str) -> Vec<(Option<Self>, Vec<SettingsField>)> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        Self::ALL
            .into_iter()
            .filter(|c| c.is_searchable())
            .map(|c| {
                let fields: Vec<SettingsField> = c
                    .fields()
                    .into_iter()
                    .filter(|f| f.matches(query))
                    .collect();
                (Some(c), fields)
            })
            .filter(|(_, fields)| !fields.is_empty())
            .collect()
    }

    pub fn fields(self) -> Vec<SettingsField> {
        self.groups()
            .iter()
            .flat_map(|g| g.iter().copied())
            .collect()
    }

    pub fn is_keybinds(self) -> bool {
        matches!(self, Self::Keybinds)
    }

    pub fn is_search(self) -> bool {
        matches!(self, Self::Search)
    }

    pub fn is_searchable(self) -> bool {
        !matches!(
            self,
            Self::Keybinds | Self::Theme | Self::Search
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Categories,
    Fields,
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
    pub category: u16,
    pub field: u16,
    pub focus: SettingsFocus,
    pub status: Option<StatusMessage>,
    pub load_path: String,
    pub previous: ReadParams,
    pub kb_selected: u16,
    pub kb_capturing: bool,
    pub query: String,
}

impl SettingsParams {
    pub const KB_PAGE: u16 = 10;

    pub fn current_category(&self) -> SettingsCategory {
        SettingsCategory::ALL[self.category as usize]
    }

    pub fn current_groups(&self) -> Vec<(Option<SettingsCategory>, Vec<SettingsField>)> {
        let category = self.current_category();
        if category.is_search() {
            return SettingsCategory::search(&self.query);
        }
        category
            .groups()
            .iter()
            .map(|group| (None, group.to_vec()))
            .collect()
    }

    pub fn current_fields(&self) -> Vec<SettingsField> {
        self.current_groups()
            .into_iter()
            .flat_map(|(_, fields)| fields)
            .collect()
    }

    pub fn query_push(&mut self, c: char) {
        self.query.push(c);
        self.field = 0;
    }

    pub fn query_pop(&mut self) {
        self.query.pop();
        self.field = 0;
    }

    pub fn current_field(&self) -> Option<SettingsField> {
        self.current_fields().get(self.field as usize).copied()
    }

    pub fn enter_category(&mut self) {
        if !self.current_category().is_keybinds() && self.current_fields().is_empty() {
            return;
        }
        self.focus = SettingsFocus::Fields;
        self.field = 0;
        if self.current_category().is_keybinds() {
            self.kb_selected = 0;
            self.kb_capturing = false;
        }
    }

    pub fn cycle_category(&mut self) {
        self.category = wrap_index(self.category, SettingsCategory::ALL.len() as u16, true);
        self.field = 0;
        self.kb_selected = 0;
        self.kb_capturing = false;
    }

    pub fn kb_move(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = wrap_index(self.kb_selected, count, !up);
    }

    pub fn kb_page(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = if up {
            self.kb_selected.saturating_sub(Self::KB_PAGE)
        } else {
            self.kb_selected
                .saturating_add(Self::KB_PAGE)
                .min(count - 1)
        };
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct LogsParams {
    pub path: String,
    pub entries: Vec<WriteEntry>,
    pub top: u16,
}

impl LogsParams {
    pub const VISIBLE: u16 = 16;

    pub fn scroll(&mut self, delta: i32) {
        let len = self.entries.len() as i32;
        let max_top = (len - Self::VISIBLE as i32).max(0);
        self.top = (self.top as i32 + delta).clamp(0, max_top) as u16;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll(i32::MAX);
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum InspectMode {
        #[default]
        Now,
        Min,
        Max,
        Avg,
    }
}

impl InspectMode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::Min => "min",
            Self::Max => "max",
            Self::Avg => "avg",
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
    Logs(LogsParams),
    SweepConfig(SweepConfigParams),
    Inspect(InspectMode),
    Stats,
    DeviceId(DeviceIdParams),
    Raw(RawParams),
    Import(ImportParams),
    CycleConfig,
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
            Self::Unknown => 0,
            Self::Reading => 1,
            Self::Connected => 2,
            Self::Reconnecting => 3,
            Self::Error(_) => 4,
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
    pub h_offset: u16,
    pub wrap: bool,
    pub previous: ReadParams,
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Settings(SettingsParams),
    Logs(LogViewParams),
}

#[cfg(test)]
mod tests {
    use super::{SettingsCategory, SettingsField, scroll_window};

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
    fn hidden_exception_hits_leave_the_field_list_but_keep_their_index() {
        use super::{UnitField, UnitParams, UnitScanHit};
        let hit = |id: u8, result: Result<Vec<u16>, String>| UnitScanHit {
            unit_id: id,
            result,
        };
        let mut params = UnitParams {
            hits: vec![
                hit(1, Ok(vec![1])),
                hit(2, Err("IllegalDataAddress".into())),
                hit(3, Ok(vec![3])),
            ],
            ..UnitParams::default()
        };
        let hits = |p: &UnitParams| -> Vec<UnitField> {
            p.fields()
                .into_iter()
                .filter(|f| matches!(f, UnitField::Hit(_)))
                .collect()
        };
        assert_eq!(
            hits(&params),
            vec![UnitField::Hit(0), UnitField::Hit(1), UnitField::Hit(2)]
        );

        params.show_exceptions = false;
        assert_eq!(hits(&params), vec![UnitField::Hit(0), UnitField::Hit(2)]);
    }

    #[test]
    fn a_remembered_scan_keeps_its_hits_but_follows_the_current_request() {
        use super::{ScanState, StatusMessage, UnitParams, UnitScanHit};
        use crate::register::RegisterType;
        let params = UnitParams {
            scan: ScanState::Probing,
            from: 3,
            to: 9,
            status: Some(StatusMessage::info("Device is busy.")),
            hits: vec![UnitScanHit {
                unit_id: 5,
                result: Ok(vec![1]),
            }],
            ..UnitParams::default()
        };
        let resumed = params.suspended().resumed(7, RegisterType::Input, 40, 2);
        assert_eq!(
            resumed.scan,
            ScanState::Stopped,
            "a closed popup ends its scan"
        );
        assert_eq!(resumed.status, None);
        assert_eq!((resumed.from, resumed.to), (3, 9));
        assert_eq!(resumed.hits.len(), 1);
        assert_eq!(resumed.id, 7);
        assert_eq!(
            (resumed.register_type, resumed.address, resumed.amount),
            (RegisterType::Input, 40, 2)
        );

        let done = UnitParams {
            scan: ScanState::Done,
            ..UnitParams::default()
        };
        assert_eq!(done.suspended().scan, ScanState::Done);
    }

    #[test]
    fn tab_jumps_between_the_form_and_the_hit_list() {
        use super::{UnitField, UnitParams, UnitScanHit};
        let mut params = UnitParams {
            selected: 3,
            ..UnitParams::default()
        };
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Mode, "no hits, stay put");

        params.hits.push(UnitScanHit {
            unit_id: 5,
            result: Ok(vec![1]),
        });
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Hit(0));
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Id);
    }

    #[test]
    fn the_save_position_toggle_sits_above_the_startup_fields() {
        let fields = SettingsCategory::Data.fields();
        let position = |field| fields.iter().position(|&f| f == field).unwrap();
        assert!(
            position(SettingsField::SavePositionOnExit) < position(SettingsField::StartupPanel)
        );
        assert_eq!(fields.iter().filter(|f| f.is_startup()).count(), 3);
    }
}

#[cfg(test)]
mod settings_params_tests {
    use super::{SettingsCategory, SettingsField, SettingsFocus, SettingsParams};

    #[test]
    fn cycling_categories_wraps_and_keeps_focus() {
        let last = SettingsCategory::ALL.len() as u16 - 1;
        let mut s = SettingsParams {
            category: last,
            field: 3,
            focus: SettingsFocus::Fields,
            ..SettingsParams::default()
        };
        s.cycle_category();
        assert_eq!(
            (s.category, s.field, s.focus),
            (0, 0, SettingsFocus::Fields)
        );
    }

    #[test]
    fn search_is_the_last_category_and_lists_no_static_fields() {
        assert_eq!(
            *SettingsCategory::ALL.last().unwrap(),
            SettingsCategory::Search
        );
        assert!(SettingsCategory::Search.fields().is_empty());
    }

    #[test]
    fn searching_matches_labels_and_descriptions_grouped_by_category() {
        let groups = SettingsCategory::search("clock");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, Some(SettingsCategory::Display));
        assert_eq!(groups[0].1, vec![SettingsField::ShowClock]);

        let by_description = SettingsCategory::search("auto-refresh");
        assert_eq!(by_description[0].1, vec![SettingsField::RefreshInterval]);

        let words = SettingsCategory::search("CYCLE panel");
        let fields: Vec<_> = words.iter().flat_map(|(_, f)| f.iter().copied()).collect();
        assert!(fields.contains(&SettingsField::CyclePinned));
        assert!(!fields.contains(&SettingsField::CycleHoldings));

        let colors = SettingsCategory::search("color");
        assert!(
            colors
                .iter()
                .all(|(c, _)| *c != Some(SettingsCategory::Theme))
        );

        assert!(SettingsCategory::search("   ").is_empty());
        assert!(SettingsCategory::search("no such setting").is_empty());
    }

    #[test]
    fn typing_a_query_resets_the_field_cursor() {
        let search = SettingsCategory::ALL.len() as u16 - 1;
        let mut s = SettingsParams {
            category: search,
            field: 4,
            ..SettingsParams::default()
        };
        for c in "memory".chars() {
            s.query_push(c);
        }
        assert_eq!(s.field, 0);
        assert_eq!(s.current_fields(), vec![SettingsField::ShowRam]);
        s.query_pop();
        assert_eq!(s.query, "memor");
    }
}
