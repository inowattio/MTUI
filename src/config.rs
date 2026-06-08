use crate::app::PinnedRegisters;
use crate::custom::CustomRule;
use crate::modbus::{
    DeviceConfig, Interface, WordOrder,
};
use crate::register::RegisterType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub device: DeviceConfig,
    pub startup: Startup,
    pub interpretations: InterpretorConfig,
    pub registers_batch: u16,
    pub auto_update_interval_seconds: Option<u64>,
    pub graph_history_cap: u16,
    pub read_only: bool,
    pub log_writes: bool,
    pub port: Option<u16>,
    pub pinned_registers: PinnedRegisters,
    pub labels: Labels,
    pub custom_rules: CustomRules,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CustomRules {
    pub holdings: Vec<CustomRule>,
    pub inputs: Vec<CustomRule>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Startup {
    pub address: u16,
    #[serde(rename = "type")]
    pub register_type: RegisterType,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Labels {
    pub holdings: Vec<Label>,
    pub inputs: Vec<Label>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Label {
    #[serde(rename = "i")]
    pub address: u16,
    #[serde(rename = "t")]
    pub text: String,
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

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig {
                interface: Interface::Mock,
                slave_id: 0,
                timeout_connect_ms: 1000,
                timeout_command_ms: 2000,
                time_between_commands_ms: 3,
                word_order: WordOrder::default(),
            },
            startup: Startup {
                address: 0,
                register_type: RegisterType::Holding,
            },
            interpretations: InterpretorConfig::default(),
            registers_batch: 4,
            auto_update_interval_seconds: Some(1),
            graph_history_cap: 180,
            read_only: false,
            log_writes: false,
            port: None,
            pinned_registers: Default::default(),
            labels: Default::default(),
            custom_rules: Default::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips() {
        let json = serde_json::to_string(&Config::default()).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.custom_rules.holdings.len(), 0);
    }

    #[test]
    fn config_without_custom_rules_still_loads() {
        let mut value = serde_json::to_value(Config::default()).unwrap();
        value.as_object_mut().unwrap().remove("custom_rules");
        let parsed: Config = serde_json::from_value(value).unwrap();
        assert!(parsed.custom_rules.holdings.is_empty());
        assert!(parsed.custom_rules.inputs.is_empty());
    }
}

interpretation_columns! {
    IndexHex => index_hex : "index (hex)" = false,
    U8s => u8s : "u8s" = false,
    I8s => i8s : "i8s" = false,
    U16 => u16 : "u16" = true,
    I16 => i16 : "i16" = true,
    F16 => f16 : "f16" = false,
    U32 => u32 : "u32" = true,
    I32 => i32 : "i32" = true,
    F32 => f32 : "f32" = true,
    U64 => u64 : "u64" = false,
    I64 => i64 : "i64" = false,
    Hex => hex : "hex" = true,
    Hex32 => hex32 : "hex32" = false,
    Bcd => bcd : "bcd" = false,
    Bcd32 => bcd32 : "bcd32" = false,
    Bits => bits : "bits" = true,
    Ascii => ascii : "ascii" = true,
    Custom => custom : "custom" = false,
    Time => time : "time (read at)" = true,
    Ago => ago : "ago (read)" = false,
    Label => label : "label" = true,
}
