use crate::app::PinnedRegisters;
use crate::modbus::{
    DataBits, DeviceConfig, Interface, InterfaceWiredParams, Parity, StopBits, WordOrder,
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
    pub dump_file: String,
    pub pinned_defaults: PinnedRegisters,
    #[serde(default)]
    pub labels: Labels,
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
                interface: Interface::Wired(InterfaceWiredParams {
                    path: "".to_string(),
                    baud_rate: 0,
                    data_bits: DataBits::Five,
                    parity: Parity::None,
                    stop_bits: StopBits::One,
                }),
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
            interpretations: InterpretorConfig {
                hex: false,
                u32: true,
                i32: true,
                f32: false,
                u64: false,
                i64: false,
                ascii: true,
                bits: false,
            },
            registers_batch: 4,
            auto_update_interval_seconds: Some(1),
            dump_file: "dump.txt".into(),
            pinned_defaults: Default::default(),
            labels: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InterpretorConfig {
    pub hex: bool,
    pub u32: bool,
    pub i32: bool,
    pub f32: bool,
    pub u64: bool,
    pub i64: bool,
    pub ascii: bool,
    pub bits: bool,
}
