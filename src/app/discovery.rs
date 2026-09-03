use super::{App, BackgroundTask, ConnectTaskResult, ReconnectState};
#[cfg(not(target_arch = "wasm32"))]
use super::{ScanProgress, scan_subnet, subnet_prefix_from};
use crate::compat;
#[cfg(not(target_arch = "wasm32"))]
use crate::compat::TaskPoll;
use crate::config::Config;
use crate::modbus::{Interface, ModbusDevice};
use crate::state::{ConnectionStatus, DiscoveryParams, InterfaceKind, Popup, StatusMessage};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(not(target_arch = "wasm32"))]
fn local_subnet_prefix() -> Option<String> {
    match local_ip_address::local_ip().ok()? {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => {
            let [a, b, c, _] = ip.octets();
            Some(format!("{a}.{b}.{c}."))
        }
        _ => None,
    }
}

#[cfg(target_arch = "wasm32")]
fn local_subnet_prefix() -> Option<String> {
    None
}

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
            ip: match &device.interface {
                Interface::Network(n) | Interface::RtuOverTcp(n) => n.ip.clone(),
                _ => local_subnet_prefix().unwrap_or_else(|| "127.0.0.1".to_string()),
            },
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
                match d.ports.iter().position(|p| p == &w.path) {
                    Some(i) => d.port_index = i as u16,
                    None => d.custom_path = w.path.clone(),
                }
            }
            Interface::Network(n) => {
                d.interface = InterfaceKind::Network;
                d.net_port = n.port;
            }
            Interface::RtuOverTcp(n) => {
                d.interface = InterfaceKind::RtuOverTcp;
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
        self.free_background_slot();
        let params = Self::discovery_params(&self.config);
        self.read_mut().popup = Some(Popup::Discovery(params));
    }

    pub fn discovery_connect(&mut self) {
        if !self.free_background_slot() {
            self.set_discovery_status(StatusMessage::info("Device is busy."));
            return;
        }
        let Some(device_config) = self.discovery().map(DiscoveryParams::device_config) else {
            return;
        };

        self.set_discovery_status(StatusMessage::warn("Connecting\u{2026}"));

        let previous = self.take_device();
        self.background_task = Some(BackgroundTask::Connect(compat::spawn(async move {
            let result = ModbusDevice::replace(previous, &device_config)
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
                if self.device.is_none() {
                    self.connection = ConnectionStatus::Error(e);
                    self.logged_connection = self.connection.clone();
                }
            }
        }
    }

    pub fn scan_progress(&self) -> Option<(usize, usize)> {
        self.network_scan
            .as_ref()
            .map(|s| (s.done.load(Ordering::Relaxed), s.total))
    }

    pub fn choose_port(&mut self, index: usize) {
        if let Some(d) = self.discovery_mut()
            && index < d.ports.len()
        {
            d.port_index = index as u16;
            d.custom_path.clear();
            d.focus_connect();
        }
    }

    pub fn use_found_ip(&mut self, index: usize) {
        if let Some(d) = self.discovery_mut()
            && let Some(ip) = d.found.get(index).cloned()
        {
            d.ip = ip;
            d.focus_connect();
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
        if !d.interface.uses_tcp() {
            return;
        }
        let Some(prefix) = subnet_prefix_from(&d.ip).or_else(local_subnet_prefix) else {
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
            d.set_found(Vec::new());
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
            d.set_found(found);
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

#[cfg(test)]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::modbus::{
        DataBits, Interface, InterfaceNetworkParams, InterfaceWiredParams, Parity, StopBits,
    };
    use crate::state::{DiscoveryColumn, DiscoveryField, DiscoveryParams, InterfaceKind};

    fn wired_with_ports(ports: &[&str]) -> DiscoveryParams {
        let mut d = DiscoveryParams {
            ports: ports.iter().map(ToString::to_string).collect(),
            ..DiscoveryParams::default()
        };
        d.set_interface(InterfaceKind::Wired);
        d
    }

    #[test]
    fn side_column_follows_the_interface() {
        use DiscoveryField::*;
        let mut d = DiscoveryParams::default();
        assert!(
            d.side_fields().is_empty(),
            "the mock has nothing to configure"
        );

        d.ports = vec!["/dev/ttyUSB0".to_string(), "/dev/ttyS0".to_string()];
        d.set_interface(InterfaceKind::Wired);
        assert_eq!(
            d.side_fields(),
            vec![
                Port(0),
                Port(1),
                CustomPath,
                Baud,
                DataBits,
                Parity,
                StopBits
            ]
        );

        d.set_found(vec!["10.0.0.5".to_string()]);
        d.set_interface(InterfaceKind::Network);
        assert_eq!(d.side_fields(), vec![Ip, NetPort, ScanNetwork, Found(0)]);
    }

    #[test]
    fn tab_switches_columns_only_when_there_is_a_side() {
        let mut d = DiscoveryParams::default();
        d.toggle_column();
        assert_eq!(
            d.column,
            DiscoveryColumn::Common,
            "nothing to switch to on the mock"
        );

        let mut d = wired_with_ports(&["/dev/ttyUSB0"]);
        d.toggle_column();
        assert_eq!(d.column, DiscoveryColumn::Side);
        assert_eq!(d.current_field(), DiscoveryField::Port(0));
        d.move_cursor(true);
        assert_eq!(d.current_field(), DiscoveryField::CustomPath);
        d.move_cursor(true);
        assert_eq!(d.current_field(), DiscoveryField::Baud);
        d.move_cursor(false);
        d.move_cursor(false);
        d.move_cursor(false);
        assert_eq!(
            d.current_field(),
            DiscoveryField::StopBits,
            "wraps within the column"
        );

        d.toggle_column();
        assert_eq!(d.current_field(), DiscoveryField::Interface);
        d.move_cursor(false);
        assert_eq!(
            d.current_field(),
            DiscoveryField::Connect,
            "wraps within the column"
        );
    }

    #[test]
    fn a_typed_path_wins_over_the_list() {
        let mut d = wired_with_ports(&["/dev/ttyUSB0", "/dev/ttyUSB1"]);
        d.port_index = 1;
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyUSB1"));

        d.custom_path = "  /dev/ttyAMA0 ".to_string();
        assert!(d.custom_path_active());
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyAMA0"));
        assert!(matches!(
            d.device_config().interface,
            Interface::Wired(ref w) if w.path == "/dev/ttyAMA0" && w.baud_rate == 9600
        ));

        d.custom_path = "   ".to_string();
        assert!(!d.custom_path_active(), "whitespace is not a path");
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyUSB1"));
    }

    #[test]
    fn baud_steps_to_the_nearest_preset_and_wraps() {
        let mut d = DiscoveryParams::default();
        assert_eq!(d.baud_rate, 9600);
        d.cycle_baud(true);
        assert_eq!(d.baud_rate, 19200);
        d.cycle_baud(false);
        d.cycle_baud(false);
        assert_eq!(d.baud_rate, 4800);

        d.baud_rate = 250_000;
        assert!(!d.is_preset_baud());
        d.cycle_baud(true);
        assert_eq!(d.baud_rate, 460_800);
        d.baud_rate = 250_000;
        d.cycle_baud(false);
        assert_eq!(d.baud_rate, 230_400);

        d.baud_rate = 921_600;
        d.cycle_baud(true);
        assert_eq!(d.baud_rate, 1200, "wraps past the top");
        d.cycle_baud(false);
        assert_eq!(d.baud_rate, 921_600, "wraps past the bottom");

        d.baud_rate = 0;
        d.cycle_baud(true);
        assert_eq!(d.baud_rate, 1200);
    }

    #[test]
    fn a_typed_path_needs_no_enumerated_ports() {
        let mut d = wired_with_ports(&[]);
        assert_eq!(d.serial_path(), None);
        d.custom_path = "/dev/ttyAMA0".to_string();
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyAMA0"));
    }

    #[test]
    fn an_unlisted_configured_path_shows_up_as_the_custom_path() {
        let mut config = Config::default();
        config.device.interface = Interface::Wired(InterfaceWiredParams {
            path: "/dev/serial/by-id/usb-not-enumerated-here".to_string(),
            baud_rate: 19200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        });
        let d = App::discovery_params(&config);
        assert_eq!(d.interface, InterfaceKind::Wired);
        assert_eq!(d.custom_path, "/dev/serial/by-id/usb-not-enumerated-here");
        assert_eq!(
            d.serial_path().as_deref(),
            Some("/dev/serial/by-id/usb-not-enumerated-here"),
            "the popup must not silently swap in a different port"
        );
        assert_eq!(d.baud_rate, 19200);
    }

    #[test]
    fn rtu_over_tcp_shares_the_network_side_and_maps_to_its_own_interface() {
        let mut d = DiscoveryParams {
            ip: "10.0.0.9".to_string(),
            net_port: 8899,
            ..DiscoveryParams::default()
        };
        d.set_interface(InterfaceKind::RtuOverTcp);
        assert_eq!(
            d.side_fields(),
            vec![
                DiscoveryField::Ip,
                DiscoveryField::NetPort,
                DiscoveryField::ScanNetwork
            ]
        );
        assert!(matches!(
            d.device_config().interface,
            Interface::RtuOverTcp(ref n) if n.ip == "10.0.0.9" && n.port == 8899
        ));

        let mut config = Config::default();
        config.device.interface = Interface::RtuOverTcp(InterfaceNetworkParams {
            ip: "10.0.0.9".to_string(),
            port: 8899,
        });
        let d = App::discovery_params(&config);
        assert_eq!(d.interface, InterfaceKind::RtuOverTcp);
        assert_eq!(d.ip, "10.0.0.9");
        assert_eq!(d.net_port, 8899);
    }

    #[test]
    fn leaving_for_the_mock_drops_back_to_the_common_column() {
        let mut d = wired_with_ports(&["/dev/ttyUSB0"]);
        d.toggle_column();
        d.set_interface(InterfaceKind::Mock);
        assert_eq!(d.column, DiscoveryColumn::Common);
        assert_eq!(d.current_field(), DiscoveryField::Interface);
    }

    #[test]
    fn a_shrinking_list_keeps_the_cursor_in_range() {
        let mut d = DiscoveryParams::default();
        d.set_interface(InterfaceKind::Network);
        d.set_found(vec!["10.0.0.1".to_string(), "10.0.0.2".to_string()]);
        d.toggle_column();
        d.side_selected = 4; // Found(1)
        assert_eq!(d.current_field(), DiscoveryField::Found(1));
        d.set_found(Vec::new());
        assert_eq!(d.current_field(), DiscoveryField::ScanNetwork);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn using_a_port_row_picks_it_and_lands_on_connect() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_discovery();
        *app.discovery_mut().unwrap() = wired_with_ports(&["/dev/ttyUSB0", "/dev/ttyUSB1"]);

        app.discovery_mut().unwrap().custom_path = "/dev/ttyAMA0".to_string();
        app.choose_port(1);

        let d = app.discovery().unwrap();
        assert_eq!(d.port_index, 1);
        assert!(
            d.custom_path.is_empty(),
            "picking a port supersedes the typed path"
        );
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyUSB1"));
        assert_eq!(d.current_field(), DiscoveryField::Connect);

        app.choose_port(7);
        assert_eq!(
            app.discovery().unwrap().port_index,
            1,
            "out of range is ignored"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn using_a_found_address_fills_the_ip_and_lands_on_connect() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_discovery();
        {
            let d = app.discovery_mut().unwrap();
            d.set_interface(InterfaceKind::Network);
            d.set_found(vec!["10.0.0.5".to_string()]);
        }

        app.use_found_ip(0);

        let d = app.discovery().unwrap();
        assert_eq!(d.ip, "10.0.0.5");
        assert_eq!(d.current_field(), DiscoveryField::Connect);
    }

    #[test]
    fn configured_network_address_is_shown_as_is() {
        let mut config = Config::default();
        config.device.interface = Interface::Network(InterfaceNetworkParams {
            ip: "10.1.2.3".to_string(),
            port: 1502,
        });
        let d = App::discovery_params(&config);
        assert_eq!(d.interface, InterfaceKind::Network);
        assert_eq!(d.ip, "10.1.2.3");
        assert_eq!(d.net_port, 1502);
    }

    #[test]
    fn other_interfaces_prefill_a_subnet_or_loopback() {
        let d = App::discovery_params(&Config::default());
        assert!(
            d.ip == "127.0.0.1" || d.ip.ends_with('.'),
            "unexpected prefill: {}",
            d.ip
        );
    }
}
