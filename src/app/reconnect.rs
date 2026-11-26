use super::{App, BackgroundTask, ReadError, ReadFailure, ReconnectState, reconnect_backoff};
use crate::compat::{self, Instant};
use crate::modbus::{Interface, ModbusDevice};
use crate::state::ConnectionStatus;

impl App {
    pub(super) fn maybe_reconnect(&mut self) -> bool {
        if !self.is_reading() {
            return false;
        }

        if matches!(self.config.device.interface, Interface::Mock) {
            return false;
        }

        if !self.reconnect.link_lost {
            return false;
        }

        if self.reconnect.next_at.is_some_and(|at| Instant::now() < at) {
            return true;
        }

        self.spawn_reconnect();
        true
    }

    fn spawn_reconnect(&mut self) {
        self.connection = ConnectionStatus::Reconnecting;
        self.reconnect.next_at = None;
        if self.reconnect.attempts == 0 {
            log::warn!("Connection lost | reconnecting...");
        }
        let config = self.config.device.clone();
        let previous = self.take_device();
        self.background_task = Some(BackgroundTask::Reconnect(compat::spawn(async move {
            ModbusDevice::replace(previous, &config)
                .await
                .map_err(|e| e.to_string())
        })));
    }

    pub(super) fn apply_reconnect_result(&mut self, result: Option<Result<ModbusDevice, String>>) {
        match result {
            Some(Ok(device)) => {
                device.set_unit(self.config.device.unit_id);
                self.device = Some(device);
                self.sync_api_device();
                self.reconnect = ReconnectState::default();
                self.connection = ConnectionStatus::Unknown;
                self.logged_connection = ConnectionStatus::Unknown;
                log::info!("Reconnected | {}", self.config.display_device());
            }
            other => {
                let error = match other {
                    Some(Err(e)) => e,
                    _ => "reconnect task stopped unexpectedly".to_string(),
                };
                self.reconnect.attempts = self.reconnect.attempts.saturating_add(1);
                let delay = reconnect_backoff(self.reconnect.attempts);
                self.reconnect.next_at = Some(Instant::now() + delay);
                log::warn!(
                    "Reconnect attempt {} failed | {error}; retrying in {}s",
                    self.reconnect.attempts,
                    delay.as_secs()
                );
                self.connection = ConnectionStatus::Error(error);
            }
        }
    }

    pub(super) fn settle_connection(&mut self, error: Option<&ReadError>) {
        let connection = match error {
            Some(e) => {
                let link_lost = match e.kind {
                    ReadFailure::Exception => false,
                    ReadFailure::Timeout => self.config.reconnect_on_timeout,
                    ReadFailure::Transport => true,
                };
                if link_lost {
                    self.reconnect.link_lost = true;
                } else {
                    self.reconnect = ReconnectState::default();
                }
                ConnectionStatus::Error(e.message.clone())
            }
            None => {
                self.reconnect = ReconnectState::default();
                ConnectionStatus::Connected
            }
        };

        if connection != self.logged_connection {
            match &connection {
                ConnectionStatus::Connected => log::info!("Connected"),
                ConnectionStatus::Error(e) => log::error!("Read error | {e}"),
                _ => {}
            }
            self.logged_connection = connection.clone();
        }
        self.connection = connection;
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use super::{App, BackgroundTask};
    #[cfg(not(target_arch = "wasm32"))]
    use crate::config::Config;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::modbus::{
        DataBits, DeviceConfig, Interface, InterfaceSerialParams, InterfaceTcpParams, ModbusDevice,
        Parity, StopBits,
    };
    use crate::register::RegisterType;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::state::ConnectionStatus;
    #[cfg(not(target_arch = "wasm32"))]
    use std::assert_matches;

    #[cfg(not(target_arch = "wasm32"))]
    fn missing_serial_port() -> Interface {
        Interface::Serial(InterfaceSerialParams {
            path: "/nonexistent/mtui-test-port".to_string(),
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn settle(app: &mut App) {
        crate::app::settle_until(app, |app| app.background_task.is_none(), "background task").await;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_lost_serial_link_is_reconnected_with_backoff() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.device.interface = missing_serial_port();
        app.reconnect.link_lost = true;
        app.connection = ConnectionStatus::Error("Input/output error".to_string());

        app.tick();
        assert_matches!(
            app.background_task,
            Some(BackgroundTask::Reconnect(_)),
            "a serial device must be reconnected like a network one"
        );
        assert_eq!(app.connection, ConnectionStatus::Reconnecting);

        settle(&mut app).await;
        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(app.reconnect.link_lost);
        assert_eq!(app.reconnect.attempts, 1);
        assert!(app.reconnect.next_at.is_some(), "retry is scheduled");
        assert!(
            app.device.is_none(),
            "a failed attempt leaves no device behind"
        );

        app.tick();
        assert!(
            app.background_task.is_none(),
            "no new attempt inside the backoff window"
        );

        app.reconnect.next_at = None;
        app.tick();
        assert_matches!(
            app.background_task,
            Some(BackgroundTask::Reconnect(_)),
            "retry after backoff with no device present"
        );
        settle(&mut app).await;
        assert_eq!(app.reconnect.attempts, 2);
        assert!(app.reconnect.next_at.is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn no_device_and_no_lost_link_means_no_reconnect() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.device.interface = missing_serial_port();
        app.device = None;
        app.connection = ConnectionStatus::Error("Connection failed".to_string());

        app.tick();
        assert!(app.background_task.is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_unit_id_set_during_a_reconnect_reaches_the_new_device() {
        use crate::modbus::ModbusDevice;
        use crate::state::UnitParams;

        let mut app = App::boot(Config::default(), String::new()).await;
        let stale = app.config.device.clone();
        let previous = app.take_device();
        let target = stale.unit_id.wrapping_add(4);

        app.open_unit();
        app.popup_as_mut::<UnitParams>().unwrap().id = target;
        app.commit_unit();
        assert_eq!(app.config.device.unit_id, target);

        let device = ModbusDevice::replace(previous, &stale).await.unwrap();
        assert_eq!(device.unit(), stale.unit_id);
        app.apply_reconnect_result(Some(Ok(device)));
        assert_eq!(app.device.as_ref().map(ModbusDevice::unit), Some(target));
    }

    #[cfg(not(target_arch = "wasm32"))]
    async fn app_with_silent_device() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        let silent = ModbusDevice::new(&DeviceConfig {
            unit_id: 200,
            request_timeout_ms: 50,
            ..DeviceConfig::default()
        })
        .await
        .unwrap();
        app.device = Some(silent);
        app.config.device.interface = missing_serial_port();
        app
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_silent_device_counts_as_a_lost_link_by_default() {
        let mut app = app_with_silent_device().await;
        assert!(app.config.reconnect_on_timeout);

        app.refresh();
        settle(&mut app).await;

        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(app.reconnect.link_lost);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn timeouts_are_left_alone_when_the_setting_is_off() {
        let mut app = app_with_silent_device().await;
        app.config.reconnect_on_timeout = false;

        app.refresh();
        settle(&mut app).await;
        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(!app.reconnect.link_lost);

        app.tick();
        assert!(
            app.background_task.is_none(),
            "a timeout must not reconnect with the setting off"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_dropped_connection_is_a_lost_link_even_with_the_setting_off() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                drop(stream);
            }
        });
        let config = DeviceConfig {
            interface: Interface::Tcp(InterfaceTcpParams {
                ip: "127.0.0.1".to_string(),
                port,
            }),
            connect_timeout_ms: 500,
            request_timeout_ms: 500,
            ..DeviceConfig::default()
        };
        let mut app = App::boot(Config::default(), String::new()).await;
        app.device = Some(ModbusDevice::new(&config).await.expect("connect"));
        app.config.device = config;
        app.config.reconnect_on_timeout = false;

        app.refresh();
        settle(&mut app).await;

        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(app.reconnect.link_lost);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_device_answering_with_an_exception_is_not_reconnected() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.device.interface = missing_serial_port();
        app.read_mut().register_type = RegisterType::Holding;
        app.read_mut().position = 600;

        app.refresh();
        settle(&mut app).await;
        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(!app.reconnect.link_lost);

        app.tick();
        assert!(
            app.background_task.is_none(),
            "an exception must not trigger a reconnect"
        );
    }
}
