use super::{StatusMessage, clamp_pick};
use crate::constants::{DEFAULT_SCAN_PREFIX, SCAN_PREFIX_RANGE};
use crate::modbus::{
    DataBits, DeviceConfig, Interface, InterfaceSerialParams, InterfaceTcpParams, Parity, StopBits,
    WordOrder,
};
use crate::num_ops::wrap_index;
use crate::register::RegisterType;

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
        Modbus,
    }
}

impl ScanMethod {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ping => "ICMP ping",
            Self::Port => "TCP port",
            Self::Modbus => "Modbus read",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundHost {
    pub ip: String,
    pub reply: Option<Result<Vec<u16>, String>>,
}

impl FoundHost {
    pub fn alive(ip: impl Into<String>) -> Self {
        Self {
            ip: ip.into(),
            reply: None,
        }
    }
}

impl InterfaceKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mock => "Mock",
            Self::Serial => "Serial",
            Self::Tcp => "TCP",
            Self::RtuOverTcp => "RTU over TCP",
        }
    }

    pub const fn uses_tcp(self) -> bool {
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
    ScanRepr,
    ScanPrefix,
    ScanNetwork,
    Found(usize),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiscoveryColumn {
    #[default]
    Common,
    Side,
}

#[derive(Debug, PartialEq, Eq)]
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
    pub scan_ascii: bool,
    pub scan_prefix: u8,
    pub register_type: RegisterType,
    pub address: u16,
    pub amount: u16,
    pub unit_id: u8,
    pub connect_timeout_ms: u64,
    pub command_timeout_ms: u64,
    pub between_commands_ms: u64,
    pub word_order: WordOrder,
    pub found: Vec<FoundHost>,
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
            scan_ascii: false,
            scan_prefix: DEFAULT_SCAN_PREFIX,
            register_type: RegisterType::default(),
            address: 0,
            amount: 1,
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
            InterfaceKind::Tcp | InterfaceKind::RtuOverTcp => [Ip, NetPort, ScanMethod]
                .into_iter()
                .chain(self.reads_on_scan().then_some(ScanRepr))
                .chain([ScanPrefix, ScanNetwork])
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

    pub fn reads_on_scan(&self) -> bool {
        self.scan_method == ScanMethod::Modbus
    }

    pub fn cycle_scan_method(&mut self, forward: bool) {
        self.scan_method = crate::num_ops::cycle(&ScanMethod::ALL, self.scan_method, forward);
        self.clamp_side();
    }

    pub fn step_scan_prefix(&mut self, forward: bool) {
        let next = if forward {
            self.scan_prefix.saturating_add(1)
        } else {
            self.scan_prefix.saturating_sub(1)
        };
        self.scan_prefix = next.clamp(*SCAN_PREFIX_RANGE.start(), *SCAN_PREFIX_RANGE.end());
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

    pub fn set_found(&mut self, found: Vec<FoundHost>) {
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
