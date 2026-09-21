use crate::custom::{BitEntry, CustomOp, CustomRepr, CustomRule, EnumEntry, OpKind};
use crate::input::KeyCode;
use crate::modbus::{DeviceConfig, Interface};
use crate::register::{RegisterCell, RegisterType};
use crate::state::ReadPanel;
use crate::tui::theme::Theme;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub name: String,
    pub next_config: String,
    pub device: DeviceConfig,
    pub startup: Startup,
    pub save_position_on_exit: bool,
    pub columns: InterpretorConfig,
    pub batch: BatchConfig,
    pub filter_panels_by_type: bool,
    pub refresh_interval_ms: Option<u64>,
    pub reconnect_on_timeout: bool,
    pub changed_expiry_ms: Option<u64>,
    pub graph: GraphConfig,
    pub matrix: MatrixConfig,
    pub read_only: bool,
    pub log_writes: bool,
    pub skip_unsaved_warning: bool,
    pub show_mock_device: bool,
    pub show_clock: bool,
    pub show_frame_time: bool,
    pub show_ram: bool,
    pub show_connection_label: bool,
    pub show_ascii_strip: bool,
    pub show_inactive_tabs: bool,
    pub show_read_window: bool,
    pub show_rule_continuation: bool,
    pub padding: Padding,
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

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Padding {
    pub horizontal: u16,
    pub vertical: u16,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ApiConfig {
    pub port: Option<u16>,
    pub unit_id_override: bool,
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
    pub const ALL: [BatchAnchor; 3] = [BatchAnchor::Start, BatchAnchor::Middle, BatchAnchor::End];

    pub fn label(self) -> &'static str {
        match self {
            BatchAnchor::Start => "start",
            BatchAnchor::Middle => "middle",
            BatchAnchor::End => "end",
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
    pub const ALL: [TimeMode; 2] = [TimeMode::ReadAt, TimeMode::Ago];

    pub fn label(self) -> &'static str {
        match self {
            TimeMode::ReadAt => "read at",
            TimeMode::Ago => "ago",
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
    pub const ALL: [AddressMode; 2] = [AddressMode::Dec, AddressMode::Hex];

    pub fn label(self) -> &'static str {
        match self {
            AddressMode::Dec => "decimal",
            AddressMode::Hex => "hex",
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
            pub fn get(&self, action: KeybindAction) -> KeyCode {
                match action {
                    $(KeybindAction::$action => self.$field,)+
                }
            }

            pub fn set(&mut self, action: KeybindAction, key: KeyCode) {
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

            pub fn label(self) -> &'static str {
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

    fn section_mut(&mut self, kind: RegisterType) -> &mut Vec<RegisterEntry> {
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
        let mut registers = Registers::default();
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
    pub fn enabled(&self, register_type: RegisterType) -> bool {
        match register_type {
            RegisterType::Holding => self.holdings,
            RegisterType::Input => self.inputs,
            RegisterType::Coil => self.coils,
            RegisterType::Discrete => self.discretes,
        }
    }

    pub fn toggle(&mut self, register_type: RegisterType) {
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
    pub fn enabled(&self, panel: ReadPanel) -> bool {
        match panel {
            ReadPanel::Main => true,
            ReadPanel::Pinned => self.pinned,
            ReadPanel::Labeled => self.labeled,
            ReadPanel::Custom => self.custom,
            ReadPanel::Matrix => self.matrix,
        }
    }

    pub fn toggle(&mut self, panel: ReadPanel) {
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
            refresh_interval_ms: Some(1000),
            reconnect_on_timeout: true,
            changed_expiry_ms: Some(1000),
            graph: GraphConfig::default(),
            matrix: MatrixConfig::default(),
            read_only: false,
            log_writes: false,
            skip_unsaved_warning: false,
            show_mock_device: true,
            show_clock: true,
            show_frame_time: false,
            show_ram: false,
            show_connection_label: true,
            show_ascii_strip: true,
            show_inactive_tabs: true,
            show_read_window: true,
            show_rule_continuation: false,
            padding: Padding::default(),
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
            show_rule_continuation: true,
            ..Self::default()
        }
    }
}

fn column_order<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Column>, D::Error> {
    let keys = Vec::<String>::deserialize(deserializer)?;
    Ok(keys
        .iter()
        .filter_map(|key| Column::from_key(key))
        .collect())
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
            $(pub $field: bool,)+
            #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "column_order")]
            pub order: Vec<Column>,
            pub time_mode: TimeMode,
            pub address_mode: AddressMode,
            pub label_width: u16,
            pub custom_width: u16,
        }

        impl Default for InterpretorConfig {
            fn default() -> Self {
                Self {
                    $($field: $default,)+
                    order: Vec::new(),
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

            pub fn key(self) -> &'static str {
                match self {
                    $(Column::$variant => stringify!($field),)+
                }
            }

            pub fn from_key(key: &str) -> Option<Column> {
                Column::ALL.iter().copied().find(|c| c.key() == key)
            }

            pub fn name(self) -> &'static str {
                match self {
                    $(Column::$variant => $name,)+
                }
            }
        }

        impl InterpretorConfig {
            pub fn get(&self, column: Column) -> bool {
                match column {
                    $(Column::$variant => self.$field,)+
                }
            }

            pub fn toggle(&mut self, column: Column) {
                match column {
                    $(Column::$variant => self.$field = !self.$field,)+
                }
            }
        }
    };
}

interpretation_columns! {
    Address => address : "address" = true,
    U8 => u8 : "u8" = false,
    I8 => i8 : "i8" = false,
    U16 => u16 : "u16" = true,
    I16 => i16 : "i16" = true,
    F16 => f16 : "f16" = false,
    U32 => u32 : "u32" = false,
    I32 => i32 : "i32" = false,
    U32M10K => u32_m10k : "u32 m10k" = false,
    I32M10K => i32_m10k : "i32 m10k" = false,
    F32 => f32 : "f32" = false,
    F64 => f64 : "f64" = false,
    U64 => u64 : "u64" = false,
    I64 => i64 : "i64" = false,
    Hex => hex : "hex" = true,
    Hex32 => hex32 : "hex32" = false,
    Bcd => bcd : "bcd" = false,
    Bcd32 => bcd32 : "bcd32" = false,
    Bits => bits : "bits" = true,
    Ascii => ascii : "ascii" = true,
    Custom => custom : "custom" = true,
    Time => time : "time" = true,
    Label => label : "label" = true,
}
