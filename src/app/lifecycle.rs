use super::{
    bits_to_words, default_config_path, fetch_config_or_exit, reconnect_backoff, App,
    BackgroundTask, CommStats, ConnectTaskResult, DeviceIdTaskResult, LoadConfigTaskResult,
    RawTaskResult, ReconnectState, RefreshTaskResult, SweepState, WriteOutcome,
};
use crate::compat::{self, Instant, TaskPoll};
use crate::config::{Config, InterpretorConfig};
use crate::interpretator::Interpretor;
use crate::modbus::{Interface, ModbusDevice, WordOrder};
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    ConnectionStatus, Popup, PopupKind, PopupPayload, ReadPanel, ReadParams, State, StatusMessage,
};
use crate::writes_log::WritesLogState;
use chrono::Utc;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8};
use std::sync::{Arc, Mutex};

impl App {
    pub async fn new(config_path: Option<String>, make_config_if_none: bool) -> Self {
        let create_if_missing = make_config_if_none || config_path.is_none();
        let config_path = config_path.unwrap_or_else(default_config_path);
        let config = fetch_config_or_exit(&config_path, create_if_missing);
        Self::boot(config, config_path).await
    }

    pub async fn boot(config: Config, config_path: String) -> Self {
        let device = ModbusDevice::new(&config.device)
            .await
            .inspect_err(|e| println!("Could not initialize device: {e}"))
            .ok();

        let mut app = Self {
            config_path,
            interpreter: Interpretor::new(InterpretorConfig::default(), WordOrder::default()),
            pinned_registers: Vec::new(),
            labels: BTreeMap::new(),
            custom_rules: BTreeMap::new(),
            state: State::Read(ReadParams::default()),
            config: Config::default(),
            device: None,
            running: true,
            connection: ConnectionStatus::Unknown,
            frame: 0,
            last_frame: std::time::Duration::ZERO,
            paused: false,
            headless: false,
            dirty: false,
            sweep: SweepState::default(),
            stats: CommStats::default(),
            reconnect: ReconnectState::default(),
            visible_rows: Cell::new(1),
            h_max_offset: Cell::new(0),
            previous_position: None,
            background_task: None,
            network_scan: None,
            #[cfg(not(target_arch = "wasm32"))]
            network_scan_task: None,
            previous_values: BTreeMap::new(),
            changed: BTreeMap::new(),
            read_log: BTreeMap::new(),
            value_history: BTreeMap::new(),
            pending_write: None,
            pending_import: None,
            logged_connection: ConnectionStatus::Unknown,
            api_device: Arc::new(Mutex::new(None)),
            api_bound_port: Arc::new(AtomicU16::new(0)),
            api_read_only: Arc::new(AtomicBool::new(false)),
            api_allow_slave_id: Arc::new(AtomicBool::new(false)),
            api_status: Arc::new(AtomicU8::new(0)),
            api_bind: Arc::new(AtomicU8::new(0)),
            writes_log: Arc::new(Mutex::new(WritesLogState::default())),
            #[cfg(not(target_arch = "wasm32"))]
            api_server: None,
            #[cfg(not(target_arch = "wasm32"))]
            api_server_port: None,
            #[cfg(not(target_arch = "wasm32"))]
            api_pending_port: None,
            #[cfg(not(target_arch = "wasm32"))]
            clipboard: None,
        };

        app.apply_config(config, device);
        app.visible_rows.set(app.config.registers_batch.max(1));

        if app.device.is_some() {
            app.state = State::Read(app.startup_read_params());
            log::info!("Started \u{b7} {}", app.config.display_device());
            if app.config.cycle_types.enabled_count() == 0 {
                app.notify_no_cycle_types();
            }
        } else {
            let mut read = app.startup_read_params();
            read.popup = Some(Popup::Discovery(Self::discovery_params(&app.config)));
            app.state = State::Read(read);
            log::warn!("Started \u{b7} no device, opened Discovery");
        }

        #[cfg(not(target_arch = "wasm32"))]
        app.reconcile_api_server();

        app
    }

    pub(super) fn apply_config(&mut self, config: Config, device: Option<ModbusDevice>) {
        self.device = device;
        self.interpreter =
            Interpretor::new(config.interpretations.clone(), config.device.word_order);
        self.pinned_registers = config.pinned_registers.clone().into();
        self.labels = config.labels.clone().into();
        self.custom_rules = config.custom_rules.clone().into();
        self.config = config;
        self.sweep.active = false;

        self.sync_api_device();
        self.sync_api_read_only();
        self.sync_api_allow_slave_id();
        self.refresh_writes_log_state();

        self.clear_read_accumulation();
        self.previous_position = None;
        self.connection = ConnectionStatus::Unknown;
        self.logged_connection = ConnectionStatus::Unknown;
        self.reconnect = ReconnectState::default();
    }

    pub(super) fn clear_read_accumulation(&mut self) {
        self.previous_values.clear();
        self.changed.clear();
        self.read_log.clear();
        self.value_history.clear();
        self.stats = CommStats::default();
    }

    pub(super) fn startup_read_params(&self) -> ReadParams {
        ReadParams {
            position: self.config.startup.address,
            window_start: self.config.startup.address,
            register_type: self.config.startup.register_type,
            panel: self.config.startup.panel,
            ..Default::default()
        }
    }

    pub fn read(&self) -> &ReadParams {
        match &self.state {
            State::Read(p) => p,
            _ => unreachable!("read() called outside the Read state"),
        }
    }

    pub fn read_mut(&mut self) -> &mut ReadParams {
        match &mut self.state {
            State::Read(p) => p,
            _ => unreachable!("read_mut() called outside the Read state"),
        }
    }

    pub(super) fn is_reading(&self) -> bool {
        matches!(self.state, State::Read(_))
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn available_ports() -> Vec<String> {
        Vec::new()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn available_ports() -> Vec<String> {
        tokio_serial::available_ports()
            .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default()
    }

    pub fn popup_kind(&self) -> Option<PopupKind> {
        self.read().popup.as_ref().map(Popup::kind)
    }

    pub fn popup_as<T: PopupPayload>(&self) -> Option<&T> {
        match &self.state {
            State::Read(p) => p.popup.as_ref().and_then(T::from_popup),
            _ => None,
        }
    }

    pub fn popup_as_mut<T: PopupPayload>(&mut self) -> Option<&mut T> {
        match &mut self.state {
            State::Read(p) => p.popup.as_mut().and_then(T::from_popup_mut),
            _ => None,
        }
    }

    pub fn close_popup(&mut self) {
        self.read_mut().popup = None;
    }

    pub fn set_read_status(&mut self, message: StatusMessage) {
        let p = self.read_mut();
        p.status = Some(message);
        p.status_at = Instant::now();
    }

    pub fn request_quit(&mut self) {
        if self.dirty && !self.config.ignore_dirty {
            self.read_mut().popup = Some(Popup::Quit);
        } else {
            self.running = false;
        }
    }

    pub async fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.sync_api_status();
        #[cfg(not(target_arch = "wasm32"))]
        self.reconcile_api_server();
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        if self.is_reading() {
            let rows = self.visible_rows.get();
            let cols = self.config.matrix_cols;
            self.read_mut().scroll_to_cursor(rows, cols);
        }

        if self.maybe_reconnect() {
            return;
        }

        if self.sweep.active {
            if self.is_reading() {
                let rows = self.visible_rows.get();
                let cols = self.config.matrix_cols;
                let current = self.sweep.current;
                {
                    let p = self.read_mut();
                    p.position = current;
                    p.scroll_to_cursor(rows, cols);
                }
                self.refresh().await;
            }
            return;
        }

        let should_refresh = !self.paused
            && !self.headless
            && matches!(
                &self.state,
                State::Read(p)
                    if self.config.update_interval_ms
                        .is_some_and(|ms| p.refresh_timer.elapsed().as_millis() >= ms as u128)
            );

        if should_refresh {
            self.refresh().await;
        }
    }

    fn maybe_reconnect(&mut self) -> bool {
        if self.device.is_none() || !self.is_reading() {
            return false;
        }

        if !matches!(self.config.device.interface, Interface::Network(_)) {
            return false;
        }
        if !matches!(
            self.connection,
            ConnectionStatus::Error(_) | ConnectionStatus::Reconnecting
        ) {
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
            log::warn!("Connection lost \u{b7} reconnecting\u{2026}");
        }
        let config = self.config.device.clone();
        self.background_task = Some(BackgroundTask::Reconnect(compat::spawn(async move {
            ModbusDevice::new(&config).await.map_err(|e| e.to_string())
        })));
    }

    fn apply_reconnect_result(&mut self, result: Option<Result<ModbusDevice, String>>) {
        match result {
            Some(Ok(device)) => {
                self.device = Some(device);
                self.sync_api_device();
                self.reconnect = ReconnectState::default();
                self.connection = ConnectionStatus::Unknown;
                self.logged_connection = ConnectionStatus::Unknown;
                log::info!("Reconnected \u{b7} {}", self.config.display_device());
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
                    "Reconnect attempt {} failed \u{b7} {error}; retrying in {}s",
                    self.reconnect.attempts,
                    delay.as_secs()
                );
                self.connection = ConnectionStatus::Error(error);
            }
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if self.paused {
            log::info!("Auto-refresh paused");
        } else {
            log::info!("Auto-refresh resumed");
        }
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    async fn read_words(
        device: &ModbusDevice,
        register_type: RegisterType,
        position: u16,
        amount: u16,
    ) -> Result<Vec<u16>, anyhow::Error> {
        Ok(match register_type {
            RegisterType::Holding => device.holdings(position, amount).await?,
            RegisterType::Input => device.inputs(position, amount).await?,
            RegisterType::Coil => bits_to_words(device.coils(position, amount).await?),
            RegisterType::Discrete => bits_to_words(device.discretes(position, amount).await?),
        })
    }

    async fn aquire_data_with(
        device: &ModbusDevice,
        amount: u16,
        position: u16,
        register_type: RegisterType,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let values = Self::read_words(device, register_type, position, amount).await?;

        Ok(values
            .into_iter()
            .enumerate()
            .map(|(i, v)| ((register_type, position + i as u16), v))
            .collect())
    }

    async fn aquire_pinned_data_with(
        device: &ModbusDevice,
        regs: &[RegisterCell],
        batch: u16,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let batch = batch.max(1) as usize;
        let mut collection = Vec::with_capacity(regs.len());

        let mut i = 0usize;
        while i < regs.len() {
            let (kind, start_addr) = regs[i];

            let mut run_len = 1usize;
            while i + run_len < regs.len() && run_len < batch {
                let (next_kind, next_addr) = regs[i + run_len];

                if next_kind == kind && start_addr.checked_add(run_len as u16) == Some(next_addr) {
                    run_len += 1;
                } else {
                    break;
                }
            }

            let values = Self::read_words(device, kind, start_addr, run_len as u16).await?;
            anyhow::ensure!(
                values.len() == run_len,
                "Expected {run_len} value(s) at {start_addr}, got {}",
                values.len()
            );

            collection.extend(regs[i..i + run_len].iter().copied().zip(values));

            i += run_len;
        }

        Ok(collection)
    }

    pub async fn refresh(&mut self) {
        if self.background_task.is_some() || !self.is_reading() {
            return;
        }
        let Some(device) = self.device.clone() else {
            return;
        };

        let sweeping = self.sweep.active;
        let amount = if sweeping && self.sweep.errored {
            1
        } else {
            self.config.registers_batch.max(1)
        };
        let visible = self.visible_rows.get().max(1);
        let cols = self.config.matrix_cols;
        let (panel, position, register_type) = {
            let p = self.read_mut();
            p.refresh_timer = Instant::now();
            p.loading = true;
            p.scroll_to_cursor(visible, cols);
            (p.panel, p.position, p.register_type)
        };
        let max_read_start = u16::MAX - (amount - 1);

        let read_start = if sweeping {
            position.min(max_read_start)
        } else {
            position.saturating_sub(amount / 2).min(max_read_start)
        };

        let read_main = sweeping || matches!(panel, ReadPanel::Main | ReadPanel::Matrix);
        self.connection = ConnectionStatus::Reading;

        let panel_registers = if read_main {
            Vec::new()
        } else {
            self.panel_refresh_window(amount as usize)
        };

        self.background_task = Some(BackgroundTask::Refresh(compat::spawn(async move {
            let read_began = Instant::now();
            let (main_data, pinned_data) = if read_main {
                let main = Self::aquire_data_with(&device, amount, read_start, register_type)
                    .await
                    .map_err(|e| e.to_string());
                (Some(main), None)
            } else {
                let pinned = Self::aquire_pinned_data_with(&device, &panel_registers, amount)
                    .await
                    .map_err(|e| e.to_string());
                (None, Some(pinned))
            };
            let read_duration = read_began.elapsed();

            RefreshTaskResult {
                register_type,
                main_data,
                pinned_data,
                read_duration,
            }
        })));
    }

    fn apply_refresh_result(&mut self, result: RefreshTaskResult) {
        match (&result.main_data, &result.pinned_data) {
            (Some(Ok(_)), _) | (_, Some(Ok(_))) => self.stats.record_read_ok(result.read_duration),
            (Some(Err(e)), _) | (_, Some(Err(e))) => self.stats.record_read_error(e),
            _ => {}
        }
        if !self.is_reading() {
            return;
        }
        if result.main_data.is_some()
            && !matches!(
                &self.state,
                State::Read(params) if params.register_type == result.register_type
            )
        {
            self.read_mut().loading = false;
            return;
        }

        let read_at = Utc::now();
        let history_cap = (self.config.graph_history_cap as usize).max(1);

        for data in [result.main_data.as_ref(), result.pinned_data.as_ref()]
            .into_iter()
            .flatten()
            .flatten()
        {
            for &(cell, value) in data {
                let did_change =
                    matches!(self.previous_values.get(&cell), Some(&prev) if prev != value);
                self.changed.insert(cell, did_change);
                self.previous_values.insert(cell, value);
                self.read_log.insert(cell, (value, read_at));

                let history = self.value_history.entry(cell).or_default();
                history.push_back(value);
                while history.len() > history_cap {
                    history.pop_front();
                }
            }
        }

        let connection = match (&result.main_data, &result.pinned_data) {
            (Some(Ok(_)), _) | (_, Some(Ok(_))) => ConnectionStatus::Connected,
            (Some(Err(e)), _) | (_, Some(Err(e))) => ConnectionStatus::Error(e.clone()),
            _ => self.connection.clone(),
        };
        if matches!(connection, ConnectionStatus::Connected) {
            self.reconnect = ReconnectState::default();
        }
        {
            let params = self.read_mut();
            params.read_duration = Some(result.read_duration);
            params.loading = false;
            match &result.main_data {
                Some(Err(e)) => params.read_error = Some(e.clone()),
                Some(Ok(_)) => params.read_error = None,
                None => {}
            }
        }

        if connection != self.logged_connection {
            match &connection {
                ConnectionStatus::Connected => log::info!("Connected"),
                ConnectionStatus::Error(e) => log::error!("Read error \u{b7} {e}"),
                _ => {}
            }
            self.logged_connection = connection.clone();
        }
        self.connection = connection;

        if self.sweep.active && result.main_data.is_some() {
            self.advance_sweep(matches!(&result.main_data, Some(Err(_))));
        }
    }

    pub(super) fn free_background_slot(&mut self) -> bool {
        match &self.background_task {
            None => true,
            Some(BackgroundTask::Refresh(_)) => {
                self.background_task = None;
                if self.is_reading() {
                    self.read_mut().loading = false;
                }
                true
            }
            Some(_) => false,
        }
    }

    pub async fn complete_background_task(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_network_scan();

        enum Done {
            Refresh(Option<RefreshTaskResult>),
            Write(Option<WriteOutcome>),
            Reconnect(Option<Result<ModbusDevice, String>>),
            Connect(Option<ConnectTaskResult>),
            DeviceId(Option<DeviceIdTaskResult>),
            Raw(Option<RawTaskResult>),
            LoadConfig(Option<LoadConfigTaskResult>),
        }

        macro_rules! poll_task {
            ($handle:expr, $variant:path) => {
                match $handle.poll_result() {
                    TaskPoll::Pending => return,
                    TaskPoll::Finished(value) => $variant(Some(value)),
                    TaskPoll::Gone => $variant(None),
                }
            };
        }

        let done = match self.background_task.as_mut() {
            None => return,
            Some(BackgroundTask::Refresh(handle)) => poll_task!(handle, Done::Refresh),
            Some(BackgroundTask::Write(handle)) => poll_task!(handle, Done::Write),
            Some(BackgroundTask::Reconnect(handle)) => poll_task!(handle, Done::Reconnect),
            Some(BackgroundTask::Connect(handle)) => poll_task!(handle, Done::Connect),
            Some(BackgroundTask::DeviceId(handle)) => poll_task!(handle, Done::DeviceId),
            Some(BackgroundTask::Raw(handle)) => poll_task!(handle, Done::Raw),
            Some(BackgroundTask::LoadConfig(handle)) => poll_task!(handle, Done::LoadConfig),
        };
        self.background_task = None;

        match done {
            Done::Reconnect(result) => self.apply_reconnect_result(result),
            Done::Connect(result) => self.apply_connect_result(result),
            Done::DeviceId(result) => self.apply_device_id_result(result),
            Done::Raw(result) => self.apply_raw_result(result),
            Done::LoadConfig(result) => self.apply_load_config_result(result),
            Done::Refresh(Some(result)) => self.apply_refresh_result(result),
            Done::Refresh(None) => {
                let message = "read task stopped unexpectedly".to_string();
                self.stats.record_read_error(&message);
                if self.is_reading() {
                    let params = self.read_mut();
                    params.read_error = Some(message.clone());
                    params.loading = false;
                }
                log::error!("Read task failed \u{b7} {message}");
                self.connection = ConnectionStatus::Error(message);
            }
            Done::Write(outcome) => {
                let outcome = outcome.unwrap_or_else(|| WriteOutcome {
                    ok: false,
                    message: "write task stopped unexpectedly".to_string(),
                });
                self.stats.record_write(outcome.ok, &outcome.message);
                if let Some(pending) = &self.pending_write {
                    let detail = format!(
                        "@{} = {} (was {})",
                        pending.address,
                        pending.new_value,
                        pending
                            .previous
                            .map_or_else(|| "?".to_string(), |v| v.to_string()),
                    );
                    if outcome.ok {
                        self.log_write();
                        log::info!("Write {detail}");
                    } else {
                        log::error!("Write failed \u{b7} {detail} \u{b7} {}", outcome.message);
                    }
                }
                self.pending_write = None;
                if self.is_reading() {
                    if let Some(w) = self.write_mut() {
                        w.result = Some(if outcome.ok {
                            StatusMessage::ok(outcome.message)
                        } else {
                            StatusMessage::err(outcome.message)
                        });
                    }
                }
            }
        }
    }
}
