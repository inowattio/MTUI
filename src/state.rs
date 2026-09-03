use crate::app::WriteType;
use crate::compat::Instant;
use crate::config::Column;
use crate::custom::{BitEntry, CustomOp, CustomRepr, EnumEntry};
use crate::modbus::{
    DataBits, DeviceConfig, DeviceIdAccess, Interface, InterfaceNetworkParams,
    InterfaceWiredParams, Parity, StopBits, WordOrder,
};
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
        Wired,
        Network,
        RtuOverTcp,
    }
}

impl InterfaceKind {
    pub fn label(self) -> &'static str {
        match self {
            InterfaceKind::Mock => "Mock",
            InterfaceKind::Wired => "Wired (serial)",
            InterfaceKind::Network => "Network (TCP)",
            InterfaceKind::RtuOverTcp => "RTU over TCP",
        }
    }

    pub fn uses_tcp(self) -> bool {
        matches!(self, InterfaceKind::Network | InterfaceKind::RtuOverTcp)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryField {
    Interface,
    SlaveId,
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
    pub port_index: u16,
    pub custom_path: String,
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
            port_index: 0,
            custom_path: String::new(),
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            ip: "127.0.0.1".to_string(),
            net_port: 502,
            slave_id: 1,
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
        DiscoveryField::SlaveId,
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
            InterfaceKind::Wired => (0..self.ports.len())
                .map(Port)
                .chain([CustomPath, Baud, DataBits, Parity, StopBits])
                .collect(),
            InterfaceKind::Network | InterfaceKind::RtuOverTcp => [Ip, NetPort, ScanNetwork]
                .into_iter()
                .chain((0..self.found.len()).map(Found))
                .collect(),
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

    fn network_params(&self) -> InterfaceNetworkParams {
        InterfaceNetworkParams {
            ip: self.ip.clone(),
            port: self.net_port,
        }
    }

    pub fn device_config(&self) -> DeviceConfig {
        let interface = match self.interface {
            InterfaceKind::Mock => Interface::Mock,
            InterfaceKind::Wired => Interface::Wired(InterfaceWiredParams {
                path: self.serial_path().unwrap_or_default(),
                baud_rate: self.baud_rate,
                data_bits: self.data_bits,
                parity: self.parity,
                stop_bits: self.stop_bits,
            }),
            InterfaceKind::Network => Interface::Network(self.network_params()),
            InterfaceKind::RtuOverTcp => Interface::RtuOverTcp(self.network_params()),
        };
        DeviceConfig {
            interface,
            slave_id: self.slave_id,
            timeout_connect_ms: self.connect_timeout_ms,
            timeout_command_ms: self.command_timeout_ms,
            time_between_commands_ms: self.between_commands_ms,
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
                if count > 0 {
                    self.side_selected = wrap_index(self.side_selected.min(count - 1), count, down);
                }
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
pub enum SlaveField {
    Id,
    From,
    To,
    Mode,
    Scan,
    Hit(usize),
}

#[derive(Debug, PartialEq)]
pub struct SlaveScanHit {
    pub slave_id: u8,
    pub result: Result<Vec<u16>, String>,
}

#[derive(Debug, PartialEq)]
pub struct SlaveParams {
    pub id: u8,
    pub selected: u16,
    pub from: u8,
    pub to: u8,
    pub stop_at_first: bool,
    pub active: bool,
    pub current: u8,
    pub register_type: RegisterType,
    pub address: u16,
    pub amount: u16,
    pub hits: Vec<SlaveScanHit>,
    pub status: Option<StatusMessage>,
}

impl Default for SlaveParams {
    fn default() -> Self {
        Self {
            id: 0,
            selected: 0,
            from: 1,
            to: 247,
            stop_at_first: false,
            active: false,
            current: 0,
            register_type: RegisterType::default(),
            address: 0,
            amount: 1,
            hits: Vec::new(),
            status: None,
        }
    }
}

impl SlaveParams {
    const FIXED: [SlaveField; 5] = [
        SlaveField::Id,
        SlaveField::From,
        SlaveField::To,
        SlaveField::Mode,
        SlaveField::Scan,
    ];

    pub fn fields(&self) -> Vec<SlaveField> {
        let mut fields = Self::FIXED.to_vec();
        fields.extend((0..self.hits.len()).map(SlaveField::Hit));
        fields
    }

    pub fn current_field(&self) -> SlaveField {
        let fields = self.fields();
        fields[(self.selected as usize).min(fields.len() - 1)]
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
    *top = (*top).min(len.saturating_sub(rows));
    if *cursor < *top {
        *top = *cursor;
    } else if *cursor >= top.saturating_add(rows) {
        *top = cursor.saturating_sub(rows - 1);
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
        BatchAnchor,
        ReadFullCustoms,
        CustomBatchBySize,
        AutoUpdate,
        ReconnectOnTimeout,
        HistoryCap,
        MatrixCols,
        ReadOnly,
        LogWrites,
        ApiPort,
        ApiSlaveOverride,
        StartupPanel,
        StartupType,
        StartupAddress,
        CycleHoldings,
        CycleInputs,
        CycleCoils,
        CycleDiscretes,
        IgnoreDirty,
        ShowMock,
        ClearPins,
        ClearLabels,
        ClearCustom,
        ShowContinuation,
        ShowClock,
        ShowFrameTime,
        ShowRam,
        ShowAscii,
        ShowInactiveTabs,
        ShowReadWindow,
        GraphTimeAxis,
        ChangedExpiry,
        PaddingHorizontal,
        PaddingVertical,
        ThemePreset,
        ThemeBg,
        ThemeBorder,
        ThemeAccent,
        ThemeText,
        ThemeDim,
        ThemeChanged,
        ThemeZebra,
        ThemeOk,
        ThemeWarn,
        ThemeErr,
        ThemeSelectedFg,
        ThemeSelectedBg,
        Save,
        LoadConfig,
        NextConfig,
    }
}

impl SettingsField {
    pub fn is_text_input(self) -> bool {
        matches!(
            self,
            SettingsField::Name | SettingsField::LoadConfig | SettingsField::NextConfig
        )
    }

    pub fn is_toggle(self) -> bool {
        matches!(
            self,
            SettingsField::ReadOnly
                | SettingsField::BatchAnchor
                | SettingsField::ReadFullCustoms
                | SettingsField::CustomBatchBySize
                | SettingsField::ApiSlaveOverride
                | SettingsField::LogWrites
                | SettingsField::ReconnectOnTimeout
                | SettingsField::ShowContinuation
                | SettingsField::ShowClock
                | SettingsField::ShowFrameTime
                | SettingsField::ShowRam
                | SettingsField::ShowAscii
                | SettingsField::ShowInactiveTabs
                | SettingsField::ShowReadWindow
                | SettingsField::GraphTimeAxis
                | SettingsField::StartupPanel
                | SettingsField::StartupType
                | SettingsField::IgnoreDirty
                | SettingsField::ShowMock
                | SettingsField::CycleHoldings
                | SettingsField::CycleInputs
                | SettingsField::CycleCoils
                | SettingsField::CycleDiscretes
                | SettingsField::ThemePreset
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

    pub fn is_theme_color(self) -> bool {
        matches!(
            self,
            SettingsField::ThemeBg
                | SettingsField::ThemeBorder
                | SettingsField::ThemeAccent
                | SettingsField::ThemeText
                | SettingsField::ThemeDim
                | SettingsField::ThemeChanged
                | SettingsField::ThemeZebra
                | SettingsField::ThemeOk
                | SettingsField::ThemeWarn
                | SettingsField::ThemeErr
                | SettingsField::ThemeSelectedFg
                | SettingsField::ThemeSelectedBg
        )
    }

    pub fn is_action(self) -> bool {
        matches!(
            self,
            SettingsField::ClearPins
                | SettingsField::ClearLabels
                | SettingsField::ClearCustom
                | SettingsField::Save
        )
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SettingsCategory {
        General,
        Data,
        Api,
        Display,
        Theme,
        Keybinds,
        Config,
    }
}

impl SettingsCategory {
    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::General => "General",
            SettingsCategory::Data => "Data",
            SettingsCategory::Api => "API",
            SettingsCategory::Display => "Display",
            SettingsCategory::Theme => "Theme",
            SettingsCategory::Keybinds => "Keybinds",
            SettingsCategory::Config => "Config",
        }
    }

    pub fn fields(self) -> &'static [SettingsField] {
        use SettingsField::*;
        match self {
            SettingsCategory::General => &[
                Name,
                StartupPanel,
                StartupType,
                StartupAddress,
                IgnoreDirty,
                ShowMock,
            ],
            SettingsCategory::Data => &[
                RegistersBatch,
                BatchAnchor,
                ReadFullCustoms,
                CustomBatchBySize,
                AutoUpdate,
                ReconnectOnTimeout,
                ReadOnly,
                HistoryCap,
                MatrixCols,
                CycleHoldings,
                CycleInputs,
                CycleCoils,
                CycleDiscretes,
            ],
            SettingsCategory::Api => &[ApiPort, ApiSlaveOverride, LogWrites],
            SettingsCategory::Display => &[
                ShowClock,
                ShowFrameTime,
                ShowRam,
                ShowAscii,
                ShowInactiveTabs,
                ShowReadWindow,
                GraphTimeAxis,
                ChangedExpiry,
                ShowContinuation,
                PaddingHorizontal,
                PaddingVertical,
            ],
            SettingsCategory::Theme => &[
                ThemePreset,
                ThemeBorder,
                ThemeAccent,
                ThemeText,
                ThemeBg,
                ThemeDim,
                ThemeChanged,
                ThemeZebra,
                ThemeOk,
                ThemeWarn,
                ThemeErr,
                ThemeSelectedFg,
                ThemeSelectedBg,
            ],
            SettingsCategory::Keybinds => &[],
            SettingsCategory::Config => &[
                ClearPins,
                ClearLabels,
                ClearCustom,
                Save,
                LoadConfig,
                NextConfig,
            ],
        }
    }

    pub fn is_keybinds(self) -> bool {
        matches!(self, SettingsCategory::Keybinds)
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
    pub kb_top: u16,
    pub kb_capturing: bool,
}

impl SettingsParams {
    pub const KB_VISIBLE: u16 = 14;

    pub fn current_category(&self) -> SettingsCategory {
        SettingsCategory::ALL[self.category as usize]
    }

    pub fn current_fields(&self) -> &'static [SettingsField] {
        self.current_category().fields()
    }

    pub fn current_field(&self) -> Option<SettingsField> {
        self.current_fields().get(self.field as usize).copied()
    }

    pub fn enter_category(&mut self) {
        self.focus = SettingsFocus::Fields;
        self.field = 0;
        if self.current_category().is_keybinds() {
            self.kb_selected = 0;
            self.kb_top = 0;
            self.kb_capturing = false;
        }
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
            InspectMode::Now => "now",
            InspectMode::Min => "min",
            InspectMode::Max => "max",
            InspectMode::Avg => "avg",
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
    Slave(SlaveParams),
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
            graph_column: Column::Custom,
            graph_series: Vec::new(),
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

    pub fn toggle_panel(&mut self, forward: bool) {
        self.panel = cycle(&ReadPanel::ALL, self.panel, forward);
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
    use super::scroll_window;

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
}
