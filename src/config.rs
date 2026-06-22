use crate::app::PinnedRegisters;
use crate::custom::{CustomOp, CustomRepr, CustomRule, EnumEntry, OpKind};
use crate::input::KeyCode;
use crate::modbus::{DeviceConfig, Interface};
use crate::register::RegisterType;
use crate::state::ReadPanel;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub name: String,
    pub device: DeviceConfig,
    pub startup: Startup,
    pub interpretations: InterpretorConfig,
    pub registers_batch: u16,
    pub update_interval_ms: Option<u64>,
    pub graph_history_cap: u16,
    pub matrix_cols: u16,
    pub read_only: bool,
    pub log_writes: bool,
    pub ignore_dirty: bool,
    pub cycle_types: CycleTypes,
    pub port: Option<u16>,
    pub pinned_registers: PinnedRegisters,
    pub labels: Labels,
    pub custom_rules: CustomRules,
    pub keybinds: Keybinds,
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
    Exit => exit : "Quit" = EXIT,
    Pin => pin : "Add/remove pin" = PIN,
    Dump => dump : "Dump read data" = DUMP,
    Help => help : "Help" = HELP,
    Refresh => refresh : "Refresh" = REFRESH,
    Toggle => toggle : "Switch register type" = TOGGLE,
    Write => write : "Write register" = WRITE,
    Jump => jump : "Go to address/label" = JUMP,
    Label => label : "Label register" = LABEL,
    Custom => custom : "Custom rule" = CUSTOM,
    Columns => columns : "Toggle columns" = COLUMNS,
    Pause => pause : "Pause/resume" = PAUSE,
    WordOrder => word_order : "Cycle word order" = WORD_ORDER,
    Slave => slave : "Set slave id" = SLAVE,
    CyclePosition => cycle_position : "Previous position" = CYCLE_POSITION,
    Inspect => inspect : "Inspect register" = INSPECT,
    DeviceId => device_id : "Device identification" = DEVICE_ID,
    Raw => raw : "Raw function call" = RAW,
    Graph => graph : "Value graph" = GRAPH,
    Discovery => discovery : "Switch device" = DISCOVERY,
    Settings => settings : "Settings" = SETTINGS,
    CopyAddress => copy_address : "Copy address" = COPY_ADDRESS,
    Logs => logs : "View write log" = LOGS,
    AppLogs => app_logs : "App log" = APP_LOGS,
    Sweep => sweep : "Sweep" = SWEEP,
    Clear => clear : "Clear session data" = CLEAR,
    SwitchView => switch_view : "Cycle panel" = SWITCH_VIEW,
    Action => action : "Read / confirm" = ACTION,
    MoveUp => move_up : "Move up" = MOVE_UP,
    MoveDown => move_down : "Move down" = MOVE_DOWN,
    PageUp => page_up : "Page up" = PAGE_UP,
    PageDown => page_down : "Page down" = PAGE_DOWN,
}

impl Keybinds {
    pub fn action_for(&self, code: KeyCode) -> Option<KeybindAction> {
        KeybindAction::ALL
            .iter()
            .copied()
            .find(|&action| self.get(action) == code)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CustomRules {
    pub holdings: Vec<CustomRule>,
    pub inputs: Vec<CustomRule>,
    pub coils: Vec<CustomRule>,
    pub discretes: Vec<CustomRule>,
    pub show_continuation: bool,
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
pub struct Labels {
    pub holdings: Vec<Label>,
    pub inputs: Vec<Label>,
    pub coils: Vec<Label>,
    pub discretes: Vec<Label>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Label {
    #[serde(rename = "i")]
    pub address: u16,
    #[serde(rename = "t")]
    pub text: String,
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

impl Config {
    pub fn display_device(&self) -> String {
        match &self.device.interface {
            Interface::Mock => "Mock".to_string(),
            Interface::Wired(p) => format!("Wired {} ({})", p.path, p.baud_rate),
            Interface::Network(p) => format!("Network: {}:{}", p.ip, p.port),
        }
    }
}

fn label(address: u16, text: &str) -> Label {
    Label {
        address,
        text: text.to_string(),
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

fn demo_labels() -> Labels {
    Labels {
        holdings: vec![
            label(0, "model (ascii)"),
            label(8, "fw version (bcd)"),
            label(9, "serial (u32)"),
            label(11, "slave id"),
            label(12, "uptime (u32 s)"),
            label(50, "set: voltage"),
            label(51, "set: current"),
            label(52, "set: ripple"),
            label(53, "set: noise"),
            label(54, "set: time scale"),
            label(1000, "energy (u32 Wh)"),
            label(1002, "on-time (u32 s)"),
            label(1004, "write count"),
            label(1005, "energy (m10k)"),
            label(1100, "status bits"),
            label(1101, "alarm count"),
        ],
        inputs: vec![
            label(0, "voltage L1"),
            label(1, "voltage L2"),
            label(2, "voltage L3"),
            label(3, "current L1"),
            label(4, "current L2"),
            label(5, "current L3"),
            label(6, "frequency"),
            label(7, "temperature"),
            label(8, "active power (f32)"),
            label(10, "power factor (f32)"),
            label(12, "apparent (u32 VA)"),
            label(14, "reactive (i32 var)"),
            label(16, "energy (m10k)"),
            label(20, "energy (f64 kWh)"),
            label(30, "seconds"),
            label(31, "sawtooth"),
            label(32, "square"),
            label(33, "noise"),
            label(34, "random walk"),
        ],
        coils: vec![
            label(0, "main breaker"),
            label(1, "phase L1 enable"),
            label(2, "phase L2 enable"),
            label(3, "phase L3 enable"),
            label(4, "maintenance bypass"),
            label(5, "auto mode"),
        ],
        discretes: vec![
            label(0, "device ready"),
            label(1, "grid present"),
            label(2, "warning"),
            label(3, "heartbeat"),
            label(7, "noise enabled"),
            label(8, "fault"),
        ],
    }
}

fn demo_rules() -> CustomRules {
    CustomRules {
        holdings: vec![
            scaled(50, CustomRepr::U16, 10.0, 1, " V"),
            scaled(51, CustomRepr::U16, 100.0, 2, " A"),
            switch(53, &[(0, "off"), (1, "on")]),
            plain(54, CustomRepr::U16, None, " %"),
            scaled(1000, CustomRepr::U32, 1000.0, 2, " kWh"),
        ],
        inputs: vec![
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
        coils: vec![
            switch(0, &[(0, "open"), (1, "closed")]),
            switch(4, &[(0, "normal"), (1, "bypass")]),
        ],
        discretes: vec![
            switch(0, &[(0, "no"), (1, "yes")]),
            switch(8, &[(0, "ok"), (1, "FAULT")]),
        ],
        show_continuation: true,
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            name: "demo".to_string(),
            device: DeviceConfig::default(),
            startup: Startup {
                address: 5,
                register_type: RegisterType::Input,
                panel: ReadPanel::Main,
            },
            interpretations: InterpretorConfig::default(),
            registers_batch: 10,
            update_interval_ms: Some(1000),
            graph_history_cap: 180,
            matrix_cols: 10,
            read_only: false,
            log_writes: false,
            ignore_dirty: false,
            cycle_types: CycleTypes::default(),
            port: None,
            pinned_registers: Default::default(),
            labels: demo_labels(),
            custom_rules: demo_rules(),
            keybinds: Keybinds::default(),
        }
    }
}

macro_rules! interpretation_columns {
    ($($variant:ident => $field:ident : $name:literal = $default:literal),+ $(,)?) => {
        #[derive(Clone, Debug, Deserialize, Serialize)]
        #[serde(default)]
        pub struct InterpretorConfig {
            $(pub $field: bool,)+
        }

        impl Default for InterpretorConfig {
            fn default() -> Self {
                Self { $($field: $default,)+ }
            }
        }

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Column {
            $($variant,)+
        }

        impl Column {
            pub const ALL: &'static [Column] = &[$(Column::$variant),+];

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
    IndexHex => index_hex : "index (hex)" = false,
    U8s => u8s : "u8s" = false,
    I8s => i8s : "i8s" = false,
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
    Time => time : "time (read at)" = true,
    Ago => ago : "ago (read)" = false,
    Label => label : "label" = true,
}
