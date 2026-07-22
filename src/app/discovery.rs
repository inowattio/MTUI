#[cfg(not(target_arch = "wasm32"))]
use super::{scan_subnet, subnet_prefix_from, ScanProgress};
use super::{App, BackgroundTask, ConnectTaskResult, ReconnectState};
use crate::compat;
#[cfg(not(target_arch = "wasm32"))]
use crate::compat::TaskPoll;
use crate::config::Config;
use crate::modbus::{
    DeviceConfig, Interface, InterfaceNetworkParams, InterfaceWiredParams, ModbusDevice,
};
use crate::state::{
    ConnectionStatus, DiscoveryField, DiscoveryParams, InterfaceKind, Popup, StatusMessage,
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

impl App {
    pub fn discovery(&self) -> Option<&DiscoveryParams> {
        self.popup_as()
    }

    pub fn discovery_mut(&mut self) -> Option<&mut DiscoveryParams> {
        self.popup_as_mut()
    }

    pub(super) fn discovery_params(config: &Config) -> DiscoveryParams {
        let device = &config.device;
        let mut d = DiscoveryParams {
            ports: Self::available_ports(),
            slave_id: device.slave_id,
            connect_timeout_ms: device.timeout_connect_ms,
            command_timeout_ms: device.timeout_command_ms,
            between_commands_ms: device.time_between_commands_ms,
            word_order: device.word_order,
            ..Default::default()
        };
        match &device.interface {
            Interface::Wired(w) => {
                d.interface = InterfaceKind::Wired;
                d.baud_rate = w.baud_rate;
                d.data_bits = w.data_bits;
                d.parity = w.parity;
                d.stop_bits = w.stop_bits;
                if let Some(i) = d.ports.iter().position(|p| p == &w.path) {
                    d.port_index = i as u16;
                }
            }
            Interface::Network(n) => {
                d.interface = InterfaceKind::Network;
                d.ip = n.ip.clone();
                d.net_port = n.port;
            }
            Interface::Mock => d.interface = InterfaceKind::Mock,
        }

        if !config.show_mock && d.interface == InterfaceKind::Mock {
            d.interface = InterfaceKind::Wired;
        }
        d
    }

    pub fn open_discovery(&mut self) {
        self.background_task = None;
        let params = Self::discovery_params(&self.config);
        self.read_mut().popup = Some(Popup::Discovery(params));
    }

    pub fn discovery_connect(&mut self) {
        if !self.free_background_slot() {
            self.set_discovery_status(StatusMessage::info("Device is busy."));
            return;
        }
        let device_config = {
            let Some(d) = self.discovery() else {
                return;
            };
            let interface = match d.interface {
                InterfaceKind::Mock => Interface::Mock,
                InterfaceKind::Wired => Interface::Wired(InterfaceWiredParams {
                    path: d
                        .ports
                        .get(d.port_index as usize)
                        .cloned()
                        .unwrap_or_default(),
                    baud_rate: d.baud_rate,
                    data_bits: d.data_bits,
                    parity: d.parity,
                    stop_bits: d.stop_bits,
                }),
                InterfaceKind::Network => Interface::Network(InterfaceNetworkParams {
                    ip: d.ip.clone(),
                    port: d.net_port,
                }),
            };
            DeviceConfig {
                interface,
                slave_id: d.slave_id,
                timeout_connect_ms: d.connect_timeout_ms,
                timeout_command_ms: d.command_timeout_ms,
                time_between_commands_ms: d.between_commands_ms,
                word_order: d.word_order,
            }
        };

        self.set_discovery_status(StatusMessage::warn("Connecting\u{2026}"));

        self.background_task = Some(BackgroundTask::Connect(compat::spawn(async move {
            let result = ModbusDevice::new(&device_config)
                .await
                .map_err(|e| e.to_string());
            ConnectTaskResult {
                config: device_config,
                result,
            }
        })));
    }

    pub(super) fn apply_connect_result(&mut self, result: Option<ConnectTaskResult>) {
        let Some(ConnectTaskResult { config, result }) = result else {
            log::error!("Connect task stopped unexpectedly");
            self.set_discovery_status(StatusMessage::err(
                "Connection failed: task stopped unexpectedly",
            ));
            return;
        };
        match result {
            Ok(device) => {
                self.device = Some(device);
                self.sync_api_device();
                self.refresh_writes_log_state();
                self.interpreter.set_word_order(config.word_order);
                self.config.device = config;
                self.refresh_dirty();
                self.clear_read_accumulation();
                self.connection = ConnectionStatus::Unknown;
                self.logged_connection = ConnectionStatus::Unknown;
                self.reconnect = ReconnectState::default();
                let device = self.config.display_device();
                log::info!("Switched device \u{b7} {device}");
                if self.discovery().is_some() {
                    self.close_popup();
                }
            }
            Err(e) => {
                log::error!("Connect failed \u{b7} {e}");
                self.set_discovery_status(StatusMessage::err(format!("Connection failed: {e}")));
            }
        }
    }

    pub fn scan_progress(&self) -> Option<(usize, usize)> {
        self.network_scan
            .as_ref()
            .map(|s| (s.done.load(Ordering::Relaxed), s.total))
    }

    pub fn use_found_ip(&mut self, index: u16) {
        if let Some(d) = self.discovery_mut() {
            let Some(ip) = d.found.get(index as usize).cloned() else {
                return;
            };
            d.ip = ip;
            d.scan_open = false;
            if let Some(pos) = d
                .fields()
                .iter()
                .position(|f| *f == DiscoveryField::Connect)
            {
                d.selected = pos as u16;
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_network_scan(&mut self) {
        if self.network_scan.is_some() {
            return;
        }
        let Some(d) = self.discovery() else {
            return;
        };
        if d.interface != InterfaceKind::Network {
            return;
        }
        let Some(prefix) = subnet_prefix_from(&d.ip).or_else(crate::state::local_subnet_prefix)
        else {
            self.set_discovery_status(StatusMessage::err("Couldn't determine a subnet to scan"));
            return;
        };
        let port = d.net_port;
        let per_host = Duration::from_millis(d.connect_timeout_ms.clamp(100, 2_000));
        let total = 254;
        let done = Arc::new(AtomicUsize::new(0));
        self.network_scan = Some(ScanProgress {
            done: done.clone(),
            total,
        });
        if let Some(d) = self.discovery_mut() {
            d.found.clear();
            d.scan_open = true;
            d.scan_selected = 0;
            d.status = Some(StatusMessage::warn(format!(
                "Scanning {prefix}0/24\u{2026}"
            )));
        }
        self.network_scan_task = Some(compat::spawn(scan_subnet(prefix, port, per_host, done)));
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start_network_scan(&mut self) {
        self.set_discovery_status(StatusMessage::warn(
            "Network scan isn't available in the web demo",
        ));
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn poll_network_scan(&mut self) {
        let Some(handle) = self.network_scan_task.as_mut() else {
            return;
        };
        match handle.poll_result() {
            TaskPoll::Pending => {}
            TaskPoll::Finished(found) => self.finish_network_scan(found),
            TaskPoll::Gone => self.finish_network_scan(Vec::new()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn finish_network_scan(&mut self, found: Vec<String>) {
        self.network_scan_task = None;
        self.network_scan = None;
        let count = found.len();
        if let Some(d) = self.discovery_mut() {
            d.found = found;
            d.status = Some(if count == 0 {
                StatusMessage::warn("No devices found on this subnet")
            } else {
                StatusMessage::ok(format!("Found {count} device(s)"))
            });
        }
    }

    fn set_discovery_status(&mut self, msg: StatusMessage) {
        if let Some(d) = self.discovery_mut() {
            d.status = Some(msg);
        }
    }
}
