use crate::custom::{BitEntry, CustomOp, CustomRepr, CustomRule, EnumEntry, OpKind};
use crate::input::KeyCode;
use crate::modbus::{DeviceConfig, Interface};
use crate::register::{RegisterCell, RegisterType};
use crate::state::ReadPanel;
use crate::tui::theme::Theme;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    #[serde(skip)]
    pub legacy: bool,
    pub name: String,
    pub next_config: String,
    pub device: DeviceConfig,
    pub startup: Startup,
    pub save_position_on_exit: bool,
    pub columns: InterpretorConfig,
    pub batch: BatchConfig,
    pub filter_panels_by_type: bool,
    pub refresh_interval_ms: u64,
    pub reconnect_on_timeout: bool,
    pub changed_expiry_ms: u64,
    pub graph: GraphConfig,
    pub matrix: MatrixConfig,
    pub read_only: bool,
    pub write_log: WriteLogConfig,
    pub skip_unsaved_warning: bool,
    pub display: DisplayConfig,
    pub cycle_register_types: CycleTypes,
    pub cycle_panels: CyclePanels,
    pub api: ApiConfig,
    pub registers: Registers,
    pub keybinds: Keybinds,
    pub theme: Theme,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct BatchConfig {
    pub size: u16,
    pub anchor: BatchAnchor,
    pub read_full_customs: bool,
    pub custom_by_registers: bool,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            size: 10,
            anchor: BatchAnchor::Middle,
            read_full_customs: false,
            custom_by_registers: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct GraphConfig {
    pub history: u16,
    pub time_axis: bool,
}

impl Default for GraphConfig {
    fn default() -> Self {
        Self {
            history: 180,
            time_axis: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct MatrixConfig {
    pub columns: u16,
    pub show_context: bool,
}

impl Default for MatrixConfig {
    fn default() -> Self {
        Self {
            columns: 0,
            show_context: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub clock: bool,
    pub frame_time: bool,
    pub ram: bool,
    pub connection_label: bool,
    pub ascii_strip: bool,
    pub inactive_tabs: bool,
    pub read_window: bool,
    pub rule_continuation: bool,
    pub mock_device: bool,
    pub padding: Padding,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            clock: true,
            frame_time: false,
            ram: false,
            connection_label: true,
            ascii_strip: true,
            inactive_tabs: true,
            read_window: true,
            rule_continuation: false,
            mock_device: true,
            padding: Padding::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Padding {
    pub horizontal: u16,
    pub vertical: u16,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct WriteLogConfig {
    pub enabled: bool,
    pub directory: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ApiConfig {
    pub enabled: bool,
    pub port: u16,
    pub unit_id_override: bool,
}

impl ApiConfig {
    pub fn desired_port(&self) -> Option<u16> {
        self.enabled.then_some(self.port)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BatchAnchor {
    Start,
    #[default]
    Middle,
    End,
}

impl BatchAnchor {
    pub const ALL: [Self; 3] = [Self::Start, Self::Middle, Self::End];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimeMode {
    #[default]
    ReadAt,
    Ago,
}

impl TimeMode {
    pub const ALL: [Self; 2] = [Self::ReadAt, Self::Ago];

    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadAt => "read at",
            Self::Ago => "ago",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AddressMode {
    #[default]
    Dec,
    Hex,
}

impl AddressMode {
    pub const ALL: [Self; 2] = [Self::Dec, Self::Hex];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dec => "decimal",
            Self::Hex => "hex",
        }
    }
}

macro_rules! keybinds {
    ($($action:ident => $field:ident : $label:literal = $default:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Deserialize, Serialize)]
        #[serde(default)]
        pub struct Keybinds {
            $(pub $field: KeyCode,)+
        }

        impl Default for Keybinds {
            fn default() -> Self {
                use crate::constants::keybind;
                Self { $($field: keybind::$default,)+ }
            }
        }

        impl Keybinds {
            pub const fn get(&self, action: KeybindAction) -> KeyCode {
                match action {
                    $(KeybindAction::$action => self.$field,)+
                }
            }

            pub const fn set(&mut self, action: KeybindAction, key: KeyCode) {
                match action {
                    $(KeybindAction::$action => self.$field = key,)+
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum KeybindAction {
            $($action,)+
        }

        impl KeybindAction {
            pub const ALL: &'static [KeybindAction] = &[$(KeybindAction::$action),+];

            pub const fn label(self) -> &'static str {
                match self {
                    $(KeybindAction::$action => $label,)+
                }
            }
        }
    };
}

keybinds! {
    About => about : "About" = ABOUT,
    Pin => pin : "Toggle pin" = PIN,
    Dump => dump : "Dump read data" = DUMP,
    Help => help : "Help" = HELP,
    Refresh => refresh : "Refresh" = REFRESH,
    SwitchType => register_type : "Switch register type" = REGISTER_TYPE,
    Write => write : "Write register" = WRITE,
    GoTo => go_to : "Go to address/label" = GO_TO,
    Label => label : "Label register" = LABEL,
    CustomRule => custom_rule : "Custom rule" = CUSTOM_RULE,
    Columns => columns : "Toggle columns" = COLUMNS,
    Pause => pause : "Toggle pause" = PAUSE,
    WordOrder => word_order : "Cycle word order" = WORD_ORDER,
    UnitId => unit_id : "Set unit id" = UNIT_ID,
    Inspect => inspect : "Inspect register" = INSPECT,
    DeviceId => device_id : "Device identification" = DEVICE_ID,
    RawRequest => raw_request : "Raw request" = RAW_REQUEST,
    Graph => graph : "Value graph" = GRAPH,
    Device => device : "Switch device" = DEVICE,
    Settings => settings : "Settings" = SETTINGS,
    CopyColumn => copy_column : "Copy column" = COPY_COLUMN,
    WriteLogs => write_logs : "Write logs" = WRITE_LOGS,
    AppLogs => app_logs : "App logs" = APP_LOGS,
    Stats => stats : "Statistics" = STATS,
    Sweep => sweep : "Sweep" = SWEEP,
    ClearSession => clear_session : "Clear session data" = CLEAR_SESSION,
    CycleConfig => cycle_config : "Cycle config" = CYCLE_CONFIG,
    Panel => panel : "Cycle panel" = PANEL,
    PageUp => page_up : "Page up" = PAGE_UP,
    PageDown => page_down : "Page down" = PAGE_DOWN,
    BatchDecrease => batch_decrease : "Decrease batch" = BATCH_DECREASE,
    BatchIncrease => batch_increase : "Increase batch" = BATCH_INCREASE,
}

impl Keybinds {
    pub fn action_for(&self, code: KeyCode) -> Option<KeybindAction> {
        KeybindAction::ALL
            .iter()
            .copied()
            .find(|&action| self.get(action) == code)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Startup {
    pub address: u16,
    #[serde(rename = "type")]
    pub register_type: RegisterType,
    pub panel: ReadPanel,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Registers {
    pub holdings: Vec<RegisterEntry>,
    pub inputs: Vec<RegisterEntry>,
    pub coils: Vec<RegisterEntry>,
    pub discretes: Vec<RegisterEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct RegisterEntry {
    pub address: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<CustomRule>,
}

pub type RegisterViews = (
    Vec<RegisterCell>,
    BTreeMap<RegisterCell, String>,
    BTreeMap<RegisterCell, CustomRule>,
);

impl Registers {
    fn section(&self, kind: RegisterType) -> &[RegisterEntry] {
        match kind {
            RegisterType::Holding => &self.holdings,
            RegisterType::Input => &self.inputs,
            RegisterType::Coil => &self.coils,
            RegisterType::Discrete => &self.discretes,
        }
    }

    const fn section_mut(&mut self, kind: RegisterType) -> &mut Vec<RegisterEntry> {
        match kind {
            RegisterType::Holding => &mut self.holdings,
            RegisterType::Input => &mut self.inputs,
            RegisterType::Coil => &mut self.coils,
            RegisterType::Discrete => &mut self.discretes,
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = (RegisterCell, &RegisterEntry)> {
        RegisterType::ALL.into_iter().flat_map(move |kind| {
            self.section(kind)
                .iter()
                .map(move |entry| ((kind, entry.address), entry))
        })
    }

    pub fn pins(&self) -> usize {
        self.entries().filter(|(_, e)| e.pinned).count()
    }

    pub fn labels(&self) -> usize {
        self.entries().filter(|(_, e)| e.label.is_some()).count()
    }

    pub fn rules(&self) -> usize {
        self.entries().filter(|(_, e)| e.custom.is_some()).count()
    }

    pub fn total(&self) -> usize {
        self.pins() + self.labels() + self.rules()
    }

    pub fn into_views(mut self) -> RegisterViews {
        let mut pinned = Vec::new();
        let mut labels = BTreeMap::new();
        let mut rules = BTreeMap::new();
        for kind in RegisterType::ALL {
            for entry in std::mem::take(self.section_mut(kind)) {
                let cell = (kind, entry.address);
                if entry.pinned {
                    pinned.push(cell);
                }
                if let Some(text) = entry.label {
                    labels.insert(cell, text);
                }
                if let Some(mut rule) = entry.custom {
                    rule.address = entry.address;
                    rules.insert(cell, rule);
                }
            }
        }
        (pinned, labels, rules)
    }

    pub fn from_views(
        pinned: &[RegisterCell],
        labels: &BTreeMap<RegisterCell, String>,
        rules: &BTreeMap<RegisterCell, CustomRule>,
    ) -> Self {
        fn entry(
            merged: &mut BTreeMap<RegisterCell, RegisterEntry>,
            cell: RegisterCell,
        ) -> &mut RegisterEntry {
            merged.entry(cell).or_insert_with(|| RegisterEntry {
                address: cell.1,
                ..Default::default()
            })
        }
        let mut merged = BTreeMap::new();
        for &cell in pinned {
            entry(&mut merged, cell).pinned = true;
        }
        for (&cell, text) in labels {
            entry(&mut merged, cell).label = Some(text.clone());
        }
        for (&cell, rule) in rules {
            entry(&mut merged, cell).custom = Some(rule.clone());
        }
        let mut registers = Self::default();
        for ((kind, _), entry) in merged {
            registers.section_mut(kind).push(entry);
        }
        registers
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct CycleTypes {
    pub holdings: bool,
    pub inputs: bool,
    pub coils: bool,
    pub discretes: bool,
}

impl Default for CycleTypes {
    fn default() -> Self {
        Self {
            holdings: true,
            inputs: true,
            coils: true,
            discretes: true,
        }
    }
}

impl CycleTypes {
    pub const fn enabled(&self, register_type: RegisterType) -> bool {
        match register_type {
            RegisterType::Holding => self.holdings,
            RegisterType::Input => self.inputs,
            RegisterType::Coil => self.coils,
            RegisterType::Discrete => self.discretes,
        }
    }

    pub const fn toggle(&mut self, register_type: RegisterType) {
        match register_type {
            RegisterType::Holding => self.holdings = !self.holdings,
            RegisterType::Input => self.inputs = !self.inputs,
            RegisterType::Coil => self.coils = !self.coils,
            RegisterType::Discrete => self.discretes = !self.discretes,
        }
    }

    pub fn enabled_count(&self) -> usize {
        RegisterType::ALL
            .iter()
            .filter(|&&t| self.enabled(t))
            .count()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct CyclePanels {
    pub pinned: bool,
    pub labeled: bool,
    pub custom: bool,
    pub matrix: bool,
}

impl Default for CyclePanels {
    fn default() -> Self {
        Self {
            pinned: true,
            labeled: true,
            custom: true,
            matrix: true,
        }
    }
}

impl CyclePanels {
    pub const fn enabled(&self, panel: ReadPanel) -> bool {
        match panel {
            ReadPanel::Main => true,
            ReadPanel::Pinned => self.pinned,
            ReadPanel::Labeled => self.labeled,
            ReadPanel::Custom => self.custom,
            ReadPanel::Matrix => self.matrix,
        }
    }

    pub const fn toggle(&mut self, panel: ReadPanel) {
        match panel {
            ReadPanel::Main => {}
            ReadPanel::Pinned => self.pinned = !self.pinned,
            ReadPanel::Labeled => self.labeled = !self.labeled,
            ReadPanel::Custom => self.custom = !self.custom,
            ReadPanel::Matrix => self.matrix = !self.matrix,
        }
    }
}

impl Config {
    pub fn display_device(&self) -> String {
        match &self.device.interface {
            Interface::Mock => "Mock".to_string(),
            Interface::Serial(p) => format!("{} ({})", p.path, p.baud_rate),
            Interface::Tcp(p) => format!("{}:{}", p.ip, p.port),
            Interface::RtuOverTcp(p) => format!("{}:{}", p.ip, p.port),
        }
    }
}

fn scaled(address: u16, repr: CustomRepr, div: f64, decimals: u8, suffix: &str) -> CustomRule {
    CustomRule {
        address,
        repr,
        ops: vec![CustomOp {
            op: OpKind::Div,
            v: div,
        }],
        decimals: Some(decimals),
        suffix: suffix.to_string(),
        ..Default::default()
    }
}

fn plain(address: u16, repr: CustomRepr, decimals: Option<u8>, suffix: &str) -> CustomRule {
    CustomRule {
        address,
        repr,
        decimals,
        suffix: suffix.to_string(),
        ..Default::default()
    }
}

fn flags(address: u16, entries: &[(u8, &str)]) -> CustomRule {
    CustomRule {
        address,
        repr: CustomRepr::U16,
        bits: entries
            .iter()
            .map(|&(bit, name)| BitEntry {
                bit,
                name: name.to_string(),
            })
            .collect(),
        ..Default::default()
    }
}

fn switch(address: u16, entries: &[(i64, &str)]) -> CustomRule {
    CustomRule {
        address,
        repr: CustomRepr::U16,
        enum_map: entries
            .iter()
            .map(|&(value, text)| EnumEntry {
                value,
                text: text.to_string(),
            })
            .collect(),
        ..Default::default()
    }
}

fn demo_labels() -> BTreeMap<RegisterCell, String> {
    let sections: [(RegisterType, &[(u16, &str)]); 4] = [
        (
            RegisterType::Holding,
            &[
                (0, "model (ascii)"),
                (8, "fw version (bcd)"),
                (9, "serial (u32)"),
                (11, "unit id"),
                (12, "uptime (u32 s)"),
                (50, "set: voltage"),
                (51, "set: current"),
                (52, "set: ripple"),
                (53, "set: noise"),
                (54, "set: time scale"),
                (1000, "energy (u32 Wh)"),
                (1002, "on-time (u32 s)"),
                (1004, "write count"),
                (1005, "energy (m10k)"),
                (1100, "status bits"),
                (1101, "alarm count"),
            ],
        ),
        (
            RegisterType::Input,
            &[
                (0, "voltage L1"),
                (1, "voltage L2"),
                (2, "voltage L3"),
                (3, "current L1"),
                (4, "current L2"),
                (5, "current L3"),
                (6, "frequency"),
                (7, "temperature"),
                (8, "active power (f32)"),
                (10, "power factor (f32)"),
                (12, "apparent (u32 VA)"),
                (14, "reactive (i32 var)"),
                (16, "energy (m10k)"),
                (20, "energy (f64 kWh)"),
                (30, "seconds"),
                (31, "sawtooth"),
                (32, "square"),
                (33, "noise"),
                (34, "random walk"),
            ],
        ),
        (
            RegisterType::Coil,
            &[
                (0, "main breaker"),
                (1, "phase L1 enable"),
                (2, "phase L2 enable"),
                (3, "phase L3 enable"),
                (4, "maintenance bypass"),
                (5, "auto mode"),
            ],
        ),
        (
            RegisterType::Discrete,
            &[
                (0, "device ready"),
                (1, "grid present"),
                (2, "warning"),
                (3, "heartbeat"),
                (7, "noise enabled"),
                (8, "fault"),
            ],
        ),
    ];
    sections
        .into_iter()
        .flat_map(|(kind, entries)| {
            entries
                .iter()
                .map(move |&(address, text)| ((kind, address), text.to_string()))
        })
        .collect()
}

fn demo_rules() -> BTreeMap<RegisterCell, CustomRule> {
    let sections = [
        (
            RegisterType::Holding,
            vec![
                scaled(50, CustomRepr::U16, 10.0, 1, " V"),
                scaled(51, CustomRepr::U16, 100.0, 2, " A"),
                switch(53, &[(0, "off"), (1, "on")]),
                plain(54, CustomRepr::U16, None, " %"),
                scaled(1000, CustomRepr::U32, 1000.0, 2, " kWh"),
                flags(1100, &[(0, "run"), (1, "grid"), (2, "warn"), (15, "beat")]),
            ],
        ),
        (
            RegisterType::Input,
            vec![
                scaled(0, CustomRepr::U16, 10.0, 1, " V"),
                scaled(1, CustomRepr::U16, 10.0, 1, " V"),
                scaled(2, CustomRepr::U16, 10.0, 1, " V"),
                scaled(3, CustomRepr::U16, 100.0, 2, " A"),
                scaled(4, CustomRepr::U16, 100.0, 2, " A"),
                scaled(5, CustomRepr::U16, 100.0, 2, " A"),
                scaled(6, CustomRepr::U16, 100.0, 2, " Hz"),
                scaled(7, CustomRepr::I16, 10.0, 1, " C"),
                plain(8, CustomRepr::F32, Some(2), " kW"),
                plain(10, CustomRepr::F32, Some(2), " pf"),
                plain(12, CustomRepr::U32, None, " VA"),
                plain(14, CustomRepr::I32, None, " var"),
                switch(32, &[(0, "low"), (1, "high")]),
            ],
        ),
        (
            RegisterType::Coil,
            vec![
                switch(0, &[(0, "open"), (1, "closed")]),
                switch(4, &[(0, "normal"), (1, "bypass")]),
            ],
        ),
        (
            RegisterType::Discrete,
            vec![
                switch(0, &[(0, "no"), (1, "yes")]),
                switch(8, &[(0, "ok"), (1, "FAULT")]),
            ],
        ),
    ];
    sections
        .into_iter()
        .flat_map(|(kind, rules)| {
            rules
                .into_iter()
                .map(move |rule| ((kind, rule.address), rule))
        })
        .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            legacy: false,
            name: "demo".to_string(),
            next_config: String::new(),
            device: DeviceConfig::default(),
            startup: Startup {
                address: 5,
                register_type: RegisterType::Input,
                panel: ReadPanel::Main,
            },
            save_position_on_exit: false,
            columns: InterpretorConfig::default(),
            batch: BatchConfig::default(),
            filter_panels_by_type: false,
            refresh_interval_ms: 1000,
            reconnect_on_timeout: true,
            changed_expiry_ms: 1000,
            graph: GraphConfig::default(),
            matrix: MatrixConfig::default(),
            read_only: false,
            write_log: WriteLogConfig::default(),
            skip_unsaved_warning: false,
            display: DisplayConfig::default(),
            cycle_register_types: CycleTypes::default(),
            cycle_panels: CyclePanels::default(),
            api: ApiConfig::default(),
            registers: Registers::default(),
            keybinds: Keybinds::default(),
            theme: Theme::default(),
        }
    }
}

impl Config {
    pub fn demo() -> Self {
        Self {
            registers: Registers::from_views(&[], &demo_labels(), &demo_rules()),
            display: DisplayConfig {
                rule_continuation: true,
                ..DisplayConfig::default()
            },
            ..Self::default()
        }
    }
}

fn column_keys<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Column>, D::Error> {
    let keys = Vec::<String>::deserialize(deserializer)?;
    let mut columns: Vec<Column> = Vec::new();
    for column in keys.iter().filter_map(|key| Column::from_key(key)) {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    Ok(columns)
}

impl Serialize for Column {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.key())
    }
}

macro_rules! interpretation_columns {
    ($($variant:ident => $field:ident : $name:literal = $default:literal),+ $(,)?) => {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(default)]
        pub struct InterpretorConfig {
            #[serde(deserialize_with = "column_keys")]
            pub visible: Vec<Column>,
            pub time_mode: TimeMode,
            pub address_mode: AddressMode,
            pub label_width: u16,
            pub custom_width: u16,
        }

        impl Default for InterpretorConfig {
            fn default() -> Self {
                Self {
                    visible: Column::ALL
                        .iter()
                        .copied()
                        .filter(|column| column.shown_by_default())
                        .collect(),
                    time_mode: TimeMode::ReadAt,
                    address_mode: AddressMode::Dec,
                    label_width: 20,
                    custom_width: 10,
                }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Column {
            $($variant,)+
        }

        impl Column {
            pub const ALL: &'static [Column] = &[$(Column::$variant),+];

            pub const fn key(self) -> &'static str {
                match self {
                    $(Column::$variant => stringify!($field),)+
                }
            }

            pub fn from_key(key: &str) -> Option<Column> {
                Column::ALL.iter().copied().find(|c| c.key() == key)
            }

            pub const fn name(self) -> &'static str {
                match self {
                    $(Column::$variant => $name,)+
                }
            }

            const fn shown_by_default(self) -> bool {
                match self {
                    $(Column::$variant => $default,)+
                }
            }
        }
    };
}

impl InterpretorConfig {
    pub fn is_visible(&self, column: Column) -> bool {
        self.visible.contains(&column)
    }

    pub fn toggle(&mut self, column: Column) {
        if let Some(index) = self.visible.iter().position(|&c| c == column) {
            self.visible.remove(index);
            return;
        }
        let rank = |c: Column| Column::ALL.iter().position(|&k| k == c).unwrap_or(0);
        let at = self
            .visible
            .iter()
            .position(|&c| rank(c) > rank(column))
            .unwrap_or(self.visible.len());
        self.visible.insert(at, column);
    }
}

interpretation_columns! {
    Address => address : "address" = true,
    Time => time : "time" = true,
    U16 => u16 : "u16" = true,
    I16 => i16 : "i16" = true,
    U8 => u8 : "u8" = false,
    I8 => i8 : "i8" = false,
    Hex => hex : "hex" = true,
    Hex32 => hex32 : "hex32" = false,
    F16 => f16 : "f16" = false,
    Bcd => bcd : "bcd" = false,
    Bcd32 => bcd32 : "bcd32" = false,
    U32 => u32 : "u32" = false,
    I32 => i32 : "i32" = false,
    U32M10K => u32_m10k : "u32 m10k" = false,
    I32M10K => i32_m10k : "i32 m10k" = false,
    U64 => u64 : "u64" = false,
    I64 => i64 : "i64" = false,
    F32 => f32 : "f32" = false,
    F64 => f64 : "f64" = false,
    Ascii => ascii : "ascii" = true,
    Bits => bits : "bits" = true,
    Custom => custom : "custom" = true,
    Label => label : "label" = true,
}

#[cfg(test)]
mod tests {
    use super::{CONFIG_VERSION, Config};

    #[test]
    fn the_version_is_written_first_and_assumed_when_missing() {
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.starts_with(&format!("{{\"version\":{CONFIG_VERSION},")));
        let loaded: Config = serde_json::from_str("{}").unwrap();
        assert_eq!(loaded.version, CONFIG_VERSION);
    }
}
