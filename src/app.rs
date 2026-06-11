use crate::compat::{self, Instant, TaskHandle, TaskPoll};
use crate::config::{Column, Config, CustomRules, InterpretorConfig, Label, Labels, Startup};
use crate::constants::CONFIG_PATH;
use crate::custom::{parse_enum, parse_op, CustomRepr, CustomRule};
use crate::interpretator::{format_ago, Interpretor};
use crate::modbus::{
    DeviceConfig, Interface, InterfaceNetworkParams, InterfaceWiredParams, ModbusDevice, WordOrder,
};
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    ConnectionStatus, CustomField, CustomParams, DiscoveryParams, DumpParams, InterfaceKind,
    LabelParams, LogViewParams, LogsParams, Popup, PopupKind, ReadPanel, ReadParams, SearchParams,
    SettingsField, SettingsParams, State, WriteParams,
};
use crate::writes_log::{SharedWritesLog, WriteKind, WritesLogState};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU16};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{error, fs};

pub type ApiDevice = Arc<Mutex<Option<ModbusDevice>>>;
pub type BoundPort = Arc<AtomicU16>;
pub type ReadOnlyFlag = Arc<AtomicBool>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord,
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
enum BackgroundTask {
    Refresh(TaskHandle<RefreshTaskResult>),
    Write(TaskHandle<WriteOutcome>),
}

#[derive(Debug)]
struct WriteOutcome {
    ok: bool,
    message: String,
}

#[derive(Debug)]
struct PendingWrite {
    address: u16,
    write_type: WriteType,
    previous: Option<u64>,
    new_value: u64,
}

#[derive(Debug)]
struct RefreshTaskResult {
    register_type: RegisterType,
    main_data: Option<Result<Vec<RegisterCellValue>, String>>,
    pinned_data: Option<Result<Vec<RegisterCellValue>, String>>,
    read_duration: Duration,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    config_path: String,
    pub running: bool,
    pub state: State,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: Option<ModbusDevice>,
    pub interpreter: Interpretor,
    pub connection: ConnectionStatus,
    pub frame: u64,
    pub paused: bool,
    pub dirty: bool,
    pub visible_rows: Cell<u16>,
    previous_position: Option<RegisterCell>,
    background_task: Option<BackgroundTask>,
    previous_values: BTreeMap<RegisterCell, u16>,
    changed: BTreeMap<RegisterCell, bool>,
    read_log: BTreeMap<RegisterCell, (u16, DateTime<Utc>)>,
    value_history: BTreeMap<RegisterCell, VecDeque<u16>>,
    labels: BTreeMap<RegisterCell, String>,
    custom_rules: BTreeMap<RegisterCell, CustomRule>,
    pending_write: Option<PendingWrite>,
    logged_connection: ConnectionStatus,
    api_device: ApiDevice,
    api_bound_port: BoundPort,
    api_read_only: ReadOnlyFlag,
    writes_log: SharedWritesLog,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PinnedRegisters {
    pub holdings: Vec<u16>,
    pub inputs: Vec<u16>,
}

impl From<PinnedRegisters> for Vec<RegisterCell> {
    fn from(value: PinnedRegisters) -> Self {
        let mut collection = Vec::new();

        for holding in value.holdings {
            collection.push((RegisterType::Holding, holding));
        }

        for input in value.inputs {
            collection.push((RegisterType::Input, input));
        }

        collection
    }
}

impl From<Labels> for BTreeMap<RegisterCell, String> {
    fn from(value: Labels) -> Self {
        let mut map = BTreeMap::new();

        for label in value.holdings {
            map.insert((RegisterType::Holding, label.address), label.text);
        }

        for label in value.inputs {
            map.insert((RegisterType::Input, label.address), label.text);
        }

        map
    }
}

impl From<&BTreeMap<RegisterCell, String>> for Labels {
    fn from(map: &BTreeMap<RegisterCell, String>) -> Self {
        let mut holdings = Vec::new();
        let mut inputs = Vec::new();

        for ((kind, address), text) in map {
            let label = Label {
                address: *address,
                text: text.clone(),
            };
            match kind {
                RegisterType::Holding => holdings.push(label),
                RegisterType::Input => inputs.push(label),
            }
        }

        Self { holdings, inputs }
    }
}

impl From<CustomRules> for BTreeMap<RegisterCell, CustomRule> {
    fn from(value: CustomRules) -> Self {
        let mut map = BTreeMap::new();

        for rule in value.holdings {
            map.insert((RegisterType::Holding, rule.address), rule);
        }

        for rule in value.inputs {
            map.insert((RegisterType::Input, rule.address), rule);
        }

        map
    }
}

impl From<&BTreeMap<RegisterCell, CustomRule>> for CustomRules {
    fn from(map: &BTreeMap<RegisterCell, CustomRule>) -> Self {
        let mut holdings = Vec::new();
        let mut inputs = Vec::new();

        for ((kind, address), rule) in map {
            let mut rule = rule.clone();
            rule.address = *address;
            match kind {
                RegisterType::Holding => holdings.push(rule),
                RegisterType::Input => inputs.push(rule),
            }
        }

        Self {
            holdings,
            inputs,
            ..Default::default()
        }
    }
}

fn save_config(path: &str, config: &Config) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| e.to_string())
}

fn build_custom_rule(c: &CustomParams) -> Result<(RegisterCell, CustomRule), String> {
    let decimals = match c.decimals.trim() {
        "" => None,
        s => Some(
            s.parse::<u8>()
                .map_err(|_| "decimals: invalid".to_string())?
                .min(10),
        ),
    };

    let rule = CustomRule {
        address: c.address,
        repr: c.repr,
        ops: c.ops.clone(),
        enum_map: c.enum_map.clone(),
        decimals,
        prefix: c.prefix.clone(),
        suffix: c.suffix.clone(),
    };
    Ok(((c.register_type, c.address), rule))
}

fn dump_example_config_and_exit(path: &str) {
    let example_config = Config::default();
    let config_string = serde_json::to_string_pretty(&example_config).unwrap();

    fs::write(path, config_string).unwrap();
    println!("No config file found, dumped example to {path}.");
    std::process::exit(1)
}

fn fetch_config_or_exit(path: &str, dump_example_if_missing: bool) -> Config {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            if dump_example_if_missing {
                dump_example_config_and_exit(path);
            }
            println!("Could not read config {path}: {e}");
            std::process::exit(2)
        }
    };
    serde_json::from_str(&content).unwrap_or_else(|e| {
        println!("Could not parse config {path}: {e}");
        std::process::exit(3)
    })
}

impl App {
    pub async fn new(config_path: Option<String>) -> Self {
        // A missing file is only auto-created at the default path; an
        // explicitly requested config that does not exist is an error.
        let dump_example_if_missing = config_path.is_none();
        let config_path = config_path.unwrap_or_else(|| CONFIG_PATH.to_string());
        let config = fetch_config_or_exit(&config_path, dump_example_if_missing);
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
            paused: false,
            dirty: false,
            visible_rows: Cell::new(1),
            previous_position: None,
            background_task: None,
            previous_values: BTreeMap::new(),
            changed: BTreeMap::new(),
            read_log: BTreeMap::new(),
            value_history: BTreeMap::new(),
            pending_write: None,
            logged_connection: ConnectionStatus::Unknown,
            api_device: Arc::new(Mutex::new(None)),
            api_bound_port: Arc::new(std::sync::atomic::AtomicU16::new(0)),
            api_read_only: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            writes_log: Arc::new(Mutex::new(WritesLogState::default())),
        };

        app.apply_config(config, device);
        app.visible_rows.set(app.config.registers_batch.max(1));

        if app.device.is_some() {
            app.state = State::Read(app.startup_read_params());
            log::info!("Started \u{b7} {}", app.config.display_device());
        } else {
            app.state = State::Discovery(Self::discovery_params(&app.config));
            log::warn!("Started \u{b7} no device, opened Discovery");
        }

        app
    }

    fn apply_config(&mut self, config: Config, device: Option<ModbusDevice>) {
        self.device = device;
        self.interpreter =
            Interpretor::new(config.interpretations.clone(), config.device.word_order);
        self.pinned_registers = config.pinned_registers.clone().into();
        self.labels = config.labels.clone().into();
        self.custom_rules = config.custom_rules.clone().into();
        self.config = config;

        self.sync_api_device();
        self.sync_api_read_only();
        self.refresh_writes_log_state();

        self.previous_values.clear();
        self.changed.clear();
        self.read_log.clear();
        self.value_history.clear();
        self.previous_position = None;
        self.connection = ConnectionStatus::Unknown;
        self.logged_connection = ConnectionStatus::Unknown;
    }

    fn startup_read_params(&self) -> ReadParams {
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

    pub fn discovery(&self) -> Option<&DiscoveryParams> {
        match &self.state {
            State::Discovery(d) => Some(d),
            _ => None,
        }
    }

    pub fn discovery_mut(&mut self) -> Option<&mut DiscoveryParams> {
        match &mut self.state {
            State::Discovery(d) => Some(d),
            _ => None,
        }
    }

    pub fn settings(&self) -> Option<&SettingsParams> {
        match &self.state {
            State::Settings(s) => Some(s),
            _ => None,
        }
    }

    pub fn settings_mut(&mut self) -> Option<&mut SettingsParams> {
        match &mut self.state {
            State::Settings(s) => Some(s),
            _ => None,
        }
    }

    pub fn open_settings(&mut self) {
        let previous = std::mem::take(self.read_mut());
        self.state = State::Settings(SettingsParams {
            previous,
            ..Default::default()
        });
    }

    pub fn close_settings(&mut self) {
        let mut previous = match &mut self.state {
            State::Settings(s) => std::mem::take(&mut s.previous),
            _ => return,
        };
        previous.loading = false;
        self.state = State::Read(previous);
    }

    pub fn api_device(&self) -> ApiDevice {
        self.api_device.clone()
    }

    pub fn api_bound_port_handle(&self) -> BoundPort {
        self.api_bound_port.clone()
    }

    pub fn api_read_only_handle(&self) -> ReadOnlyFlag {
        self.api_read_only.clone()
    }

    fn sync_api_read_only(&self) {
        self.api_read_only
            .store(self.config.read_only, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn api_bound_port(&self) -> Option<u16> {
        match self
            .api_bound_port
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0 => None,
            port => Some(port),
        }
    }

    fn sync_api_device(&self) {
        if let Ok(mut slot) = self.api_device.lock() {
            *slot = self.device.clone();
        }
    }

    fn is_reading(&self) -> bool {
        matches!(self.state, State::Read(_))
    }

    #[cfg(target_arch = "wasm32")]
    fn available_ports() -> Vec<String> {
        Vec::new()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn available_ports() -> Vec<String> {
        tokio_serial::available_ports()
            .map(|ports| ports.into_iter().map(|p| p.port_name).collect())
            .unwrap_or_default()
    }

    fn discovery_params(config: &Config) -> DiscoveryParams {
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
        d
    }

    pub fn open_discovery(&mut self) {
        self.background_task = None;
        self.state = State::Discovery(Self::discovery_params(&self.config));
    }

    pub fn return_to_read(&mut self) {
        if self.device.is_none() {
            return;
        }
        self.connection = ConnectionStatus::Unknown;
        self.logged_connection = ConnectionStatus::Unknown;
        self.state = State::Read(self.startup_read_params());
    }

    pub async fn discovery_connect(&mut self) {
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

        if let Some(d) = self.discovery_mut() {
            d.status = Some("Connecting\u{2026}".to_string());
        }

        match ModbusDevice::new(&device_config).await {
            Ok(device) => {
                self.device = Some(device);
                self.sync_api_device();
                self.refresh_writes_log_state();
                self.interpreter.set_word_order(device_config.word_order);
                self.config.device = device_config;
                self.dirty = true;
                self.previous_values.clear();
                self.changed.clear();
                self.read_log.clear();
                self.value_history.clear();
                self.connection = ConnectionStatus::Unknown;
                self.logged_connection = ConnectionStatus::Unknown;
                let device = self.config.display_device();
                log::info!("Switched device \u{b7} {device}");
                self.state = State::Read(self.startup_read_params());
            }
            Err(e) => {
                log::error!("Connect failed \u{b7} {e}");
                if let Some(d) = self.discovery_mut() {
                    d.status = Some(format!("Connection failed: {e}"));
                }
            }
        }
    }

    pub fn popup_kind(&self) -> Option<PopupKind> {
        self.read().popup.as_ref().map(Popup::kind)
    }

    pub fn close_popup(&mut self) {
        self.read_mut().popup = None;
    }

    pub fn open_help(&mut self) {
        self.read_mut().popup = Some(Popup::Help);
    }

    pub fn open_dump(&mut self) {
        self.read_mut().popup = Some(Popup::Dump(DumpParams::default()));
    }

    pub fn open_columns(&mut self) {
        self.read_mut().popup = Some(Popup::Columns(0));
    }

    pub fn open_inspect(&mut self) {
        self.read_mut().popup = Some(Popup::Inspect);
    }

    pub fn panel_cell_at(&self, index: usize) -> Option<RegisterCell> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => {
                self.pinned_registers.get(index).copied()
            }
            ReadPanel::Labeled => self.labels.keys().nth(index).copied(),
            ReadPanel::Custom => self.custom_rules.keys().nth(index).copied(),
        }
    }

    pub fn panel_window(&self, start: usize, count: usize) -> Vec<RegisterCell> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => self
                .pinned_registers
                .iter()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
            ReadPanel::Labeled => self
                .labels
                .keys()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
            ReadPanel::Custom => self
                .custom_rules
                .keys()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
        }
    }

    pub fn panel_len(&self) -> u16 {
        let len = match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => self.pinned_registers.len(),
            ReadPanel::Labeled => self.labels.len(),
            ReadPanel::Custom => self.custom_rules.len(),
        };
        len as u16
    }

    pub fn cursor_cell(&self) -> RegisterCell {
        let (panel, register_type, position, index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        match panel {
            ReadPanel::Main | ReadPanel::Matrix => (register_type, position),
            _ => self
                .panel_cell_at(index as usize)
                .unwrap_or((register_type, position)),
        }
    }

    pub fn cell_value(&self, cell: RegisterCell) -> Option<u16> {
        self.read_log.get(&cell).map(|&(value, _)| value)
    }

    pub fn cell_changed(&self, cell: RegisterCell) -> bool {
        self.changed.get(&cell).copied().unwrap_or(false)
    }

    pub fn inspect_lines(&self) -> (RegisterCell, Vec<(&'static str, String)>) {
        let cell = self.cursor_cell();
        let (kind, addr) = cell;
        let Some(&(value, time)) = self.read_log.get(&cell) else {
            return (cell, Vec::new());
        };
        let neighbor = |offset: u16| {
            self.read_log
                .get(&(kind, addr.saturating_add(offset)))
                .map(|&(v, _)| v)
        };
        let custom = self.custom_value(cell, value, self.config.device.word_order, &neighbor);
        let label = self.labels.get(&cell).map(String::as_str);
        let mut lines = vec![
            (
                "read at",
                time.with_timezone(&Local)
                    .format("%H:%M:%S.%3f")
                    .to_string(),
            ),
            ("ago", format_ago(Utc::now().signed_duration_since(time))),
        ];
        lines.extend(self.interpreter.interpret_all(
            value,
            [neighbor(1), neighbor(2), neighbor(3)],
            custom.as_deref(),
            label,
        ));
        (cell, lines)
    }

    pub fn clear_pins(&mut self) {
        let n = self.pinned_registers.len();
        self.pinned_registers.clear();
        self.dirty = true;
        log::info!("Cleared {n} pinned register(s)");
    }

    pub fn clear_labels(&mut self) {
        let n = self.labels.len();
        self.labels.clear();
        self.dirty = true;
        log::info!("Cleared {n} label(s)");
    }

    pub fn clear_custom(&mut self) {
        let n = self.custom_rules.len();
        self.custom_rules.clear();
        self.dirty = true;
        log::info!("Cleared {n} custom rule(s)");
    }

    pub fn custom_count(&self) -> usize {
        self.custom_rules.len()
    }

    pub fn writes_log_path(&self) -> std::path::PathBuf {
        let kind = match &self.config.device.interface {
            Interface::Mock => "mock",
            Interface::Wired(_) => "wired",
            Interface::Network(_) => "network",
        };
        let name = format!("writes_{kind}_{}.txt", self.config.device.slave_id);
        #[cfg(not(target_arch = "wasm32"))]
        let dir = std::env::temp_dir();
        #[cfg(target_arch = "wasm32")]
        let dir = std::path::PathBuf::new();
        dir.join(name)
    }

    pub fn writes_log_path_string(&self) -> String {
        self.writes_log_path().display().to_string()
    }

    pub fn open_logs(&mut self) {
        let path = self.writes_log_path();
        let lines: Vec<String> = match fs::read_to_string(&path) {
            Ok(content) if !content.trim().is_empty() => {
                content.lines().map(str::to_string).collect()
            }
            Ok(_) => vec!["(no writes logged yet)".to_string()],
            Err(_) => vec!["(log file not found — enable \"Log writes\" in settings)".to_string()],
        };
        let mut params = LogsParams {
            path: path.display().to_string(),
            lines,
            top: 0,
        };
        params.scroll_to_bottom();
        self.read_mut().popup = Some(Popup::Logs(params));
    }

    pub fn logs_scroll(&mut self, delta: i32) {
        if let Some(Popup::Logs(l)) = &mut self.read_mut().popup {
            l.scroll(delta);
        }
    }

    pub fn log_view(&self) -> Option<&LogViewParams> {
        match &self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub fn log_view_mut(&mut self) -> Option<&mut LogViewParams> {
        match &mut self.state {
            State::Logs(l) => Some(l),
            _ => None,
        }
    }

    pub fn open_log_view(&mut self) {
        let previous = std::mem::take(self.read_mut());
        self.state = State::Logs(LogViewParams {
            top: 0,
            follow: true,
            previous,
        });
        self.log_view_scroll(i32::MAX);
    }

    pub fn close_log_view(&mut self) {
        let previous = match &mut self.state {
            State::Logs(l) => std::mem::take(&mut l.previous),
            _ => return,
        };
        self.state = State::Read(previous);
    }

    pub fn log_view_scroll(&mut self, delta: i32) {
        let len = crate::logger::count() as i32;
        let visible = self.visible_rows.get().max(1) as i32;
        let max_top = (len - visible).max(0);
        if let Some(l) = self.log_view_mut() {
            let new = (l.top as i32 + delta).clamp(0, max_top);
            l.top = new as u16;
            l.follow = new >= max_top;
        }
    }

    pub fn writes_log_handle(&self) -> SharedWritesLog {
        self.writes_log.clone()
    }

    fn refresh_writes_log_state(&self) {
        if let Ok(mut state) = self.writes_log.lock() {
            state.enabled = self.config.log_writes;
            state.path = Some(self.writes_log_path());
        }
    }

    fn log_write(&self) {
        let Some(pending) = self.pending_write.as_ref() else {
            return;
        };
        let kind = match pending.write_type {
            WriteType::Word => WriteKind::Word(pending.new_value as u16),
            WriteType::DWord => WriteKind::DWord(pending.new_value as u32),
        };
        crate::writes_log::append(&self.writes_log, pending.address, kind, pending.previous);
    }

    fn numeric_spec(field: SettingsField) -> Option<(i64, i64, i64)> {
        match field {
            SettingsField::RegistersBatch
            | SettingsField::HistoryCap
            | SettingsField::MatrixCols => Some((1, u16::MAX as i64, 1)),
            SettingsField::AutoUpdate => Some((0, u32::MAX as i64, 100)),
            SettingsField::ApiPort => Some((-1, u16::MAX as i64, 1)),
            _ => None,
        }
    }

    fn numeric_get(&self, field: SettingsField) -> i64 {
        match field {
            SettingsField::RegistersBatch => self.config.registers_batch as i64,
            SettingsField::AutoUpdate => self.config.update_interval_ms.map_or(0, |n| n as i64),
            SettingsField::HistoryCap => self.config.graph_history_cap as i64,
            SettingsField::MatrixCols => self.config.matrix_cols as i64,
            SettingsField::ApiPort => self.config.port.map_or(-1, |p| p as i64),
            _ => 0,
        }
    }

    fn numeric_set(&mut self, field: SettingsField, value: i64) {
        match field {
            SettingsField::RegistersBatch => self.config.registers_batch = value as u16,
            SettingsField::AutoUpdate => {
                self.config.update_interval_ms = (value > 0).then_some(value as u64)
            }
            SettingsField::HistoryCap => self.config.graph_history_cap = value as u16,
            SettingsField::MatrixCols => self.config.matrix_cols = value as u16,
            SettingsField::ApiPort => self.config.port = (value >= 0).then_some(value as u16),
            _ => {}
        }
    }

    pub fn settings_adjust(&mut self, field: SettingsField, delta: i64) {
        match field {
            SettingsField::IgnoreDirty => self.config.ignore_dirty = !self.config.ignore_dirty,
            SettingsField::ReadOnly => self.config.read_only = !self.config.read_only,
            SettingsField::LogWrites => self.config.log_writes = !self.config.log_writes,
            SettingsField::ShowContinuation => {
                self.config.custom_rules.show_continuation =
                    !self.config.custom_rules.show_continuation
            }
            SettingsField::StartupPanel => {
                let panels = ReadPanel::ALL;
                let current = panels
                    .iter()
                    .position(|&p| p == self.config.startup.panel)
                    .unwrap_or(0) as i64;
                let next = (current + delta).rem_euclid(panels.len() as i64);
                self.config.startup.panel = panels[next as usize];
            }
            _ => {
                let Some((min, max, step)) = Self::numeric_spec(field) else {
                    return;
                };
                let value = (self.numeric_get(field) + delta * step).clamp(min, max);
                self.numeric_set(field, value);
            }
        }
        self.refresh_writes_log_state();
        self.sync_api_read_only();
        self.dirty = true;
    }

    pub fn settings_digit(&mut self, field: SettingsField, digit: u8) {
        let Some((min, max, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = (self.numeric_get(field).max(0) * 10 + digit as i64).clamp(min, max);
        self.numeric_set(field, value);
        self.dirty = true;
    }

    pub fn settings_backspace(&mut self, field: SettingsField) {
        if field == SettingsField::LoadConfig {
            if let Some(s) = self.settings_mut() {
                s.load_path.pop();
            }
            return;
        }
        let Some((min, _, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = self.numeric_get(field);
        self.numeric_set(field, if value >= 10 { value / 10 } else { min });
        self.dirty = true;
    }

    pub fn toggle_graph(&mut self) {
        let p = self.read_mut();
        p.graph = !p.graph;
    }

    pub fn copy_address(&mut self) {
        let (_, address) = self.cursor_cell();
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(address.to_string());
        }
        #[cfg(target_arch = "wasm32")]
        let _ = address;
    }

    pub fn toggle_graph_width(&mut self) {
        let p = self.read_mut();
        p.graph_dword = !p.graph_dword;
    }

    pub fn value_history(&self, cell: RegisterCell) -> Option<&VecDeque<u16>> {
        self.value_history.get(&cell)
    }

    pub fn open_write(&mut self) {
        if self.config.read_only {
            return;
        }

        let (write_type, write_pos) = self.cursor_cell();

        if write_type == RegisterType::Input {
            return;
        }

        let value = self
            .previous_values
            .get(&(write_type, write_pos))
            .map(|&v| v as i64);

        self.read_mut().popup = Some(Popup::Write(WriteParams {
            position: write_pos,
            value,
            ..Default::default()
        }));
    }

    pub fn open_slave(&mut self) {
        let current = self.config.device.slave_id as u16;
        self.read_mut().popup = Some(Popup::Slave(current));
    }

    pub async fn commit_slave(&mut self) {
        let id = match &self.read().popup {
            Some(Popup::Slave(value)) => Some((*value).min(u8::MAX as u16) as u8),
            _ => None,
        };
        if let Some(id) = id {
            if let Some(device) = &self.device {
                device.set_slave(id).await;
            }
            self.config.device.slave_id = id;
            self.refresh_writes_log_state();
            log::info!("Slave id set to {id}");
            self.read_mut().popup = None;
            self.refresh().await;
        }
    }

    pub fn toggle_word_order(&mut self) {
        let next = self.config.device.word_order.next();
        self.config.device.word_order = next;
        self.interpreter.set_word_order(next);
        if let Some(device) = &mut self.device {
            device.set_word_order(next);
        }
    }

    pub fn request_quit(&mut self) {
        if self.dirty && !self.config.ignore_dirty {
            self.read_mut().popup = Some(Popup::Quit);
        } else {
            self.running = false;
        }
    }

    pub fn open_search(&mut self) {
        self.read_mut().popup = Some(Popup::Search(SearchParams::default()));
        self.recompute_search();
    }

    pub fn open_label(&mut self) {
        let (label_type, label_pos) = self.cursor_cell();
        let text = self
            .labels
            .get(&(label_type, label_pos))
            .cloned()
            .unwrap_or_default();
        self.read_mut().popup = Some(Popup::Label(LabelParams {
            position: label_pos,
            register_type: label_type,
            text,
            result: None,
        }));
    }

    pub fn search_input(&mut self, c: char) {
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.query.push(c);
        }
        self.recompute_search();
    }

    pub fn search_backspace(&mut self) {
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.query.pop();
        }
        self.recompute_search();
    }

    pub fn search_move(&mut self, down: bool) {
        let rows = self.visible_rows.get();
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.selected = if down {
                s.selected.saturating_add(1)
            } else {
                s.selected.saturating_sub(1)
            };
            s.scroll(rows);
        }
    }

    pub fn search_commit(&mut self) -> bool {
        let target = match &self.read().popup {
            Some(Popup::Search(s)) => s.matches.get(s.selected as usize).map(|(cell, _)| *cell),
            _ => None,
        };
        let Some((register_type, position)) = target else {
            return false;
        };

        let from = {
            let p = self.read();
            (p.register_type, p.position)
        };
        self.previous_position = Some(from);

        let rows = self.visible_rows.get();
        let cols = self.config.matrix_cols;
        let p = self.read_mut();
        if p.panel != ReadPanel::Matrix {
            p.panel = ReadPanel::Main;
        }
        p.position = position;
        p.register_type = register_type;
        p.scroll_to_cursor(rows, cols);
        p.popup = None;
        true
    }

    pub fn cycle_position(&mut self) {
        let Some((register_type, position)) = self.previous_position else {
            return;
        };
        let current = {
            let p = self.read();
            (p.register_type, p.position)
        };
        self.previous_position = Some(current);

        let rows = self.visible_rows.get();
        let cols = self.config.matrix_cols;
        let p = self.read_mut();
        if p.panel != ReadPanel::Matrix {
            p.panel = ReadPanel::Main;
        }
        p.register_type = register_type;
        p.position = position;
        p.scroll_to_cursor(rows, cols);
    }

    fn recompute_search(&mut self) {
        let read = self.read();
        let query = match &read.popup {
            Some(Popup::Search(s)) => s.query.clone(),
            _ => return,
        };

        let (register_type, has_explicit_type) = match query.chars().next() {
            Some('h') | Some('H') => (RegisterType::Holding, true),
            Some('i') | Some('I') => (RegisterType::Input, true),
            _ => (read.register_type, false),
        };

        let mut matches: Vec<(RegisterCell, String)> = Vec::new();

        let numeric_query = if has_explicit_type {
            query.chars().skip(1).collect()
        } else {
            query.clone()
        };

        if let Ok(parsed_address) = numeric_query.trim().parse::<u32>() {
            let address = if parsed_address > u16::MAX as u32 {
                u16::MAX
            } else {
                parsed_address as u16
            };

            matches.push(((register_type, address), "jump to this address".to_string()));
        }

        let lower = query.to_lowercase();
        matches.extend(
            self.labels
                .iter()
                .filter(|(_, text)| lower.is_empty() || text.to_lowercase().contains(&lower))
                .map(|(&cell, text)| (cell, text.clone())),
        );

        let rows = self.visible_rows.get();
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.matches = matches;
            s.selected = 0;
            s.top = 0;
            s.scroll(rows);
        }
    }

    pub fn label_input(&mut self, c: char) {
        if let Some(Popup::Label(l)) = &mut self.read_mut().popup {
            l.result = None;
            l.text.push(c);
        }
    }

    pub fn label_backspace(&mut self) {
        if let Some(Popup::Label(l)) = &mut self.read_mut().popup {
            l.result = None;
            l.text.pop();
        }
    }

    pub fn commit_label(&mut self) {
        let (position, register_type, text) = match &self.read().popup {
            Some(Popup::Label(l)) => (l.position, l.register_type, l.text.clone()),
            _ => return,
        };

        let key = (register_type, position);
        if text.is_empty() {
            self.labels.remove(&key);
        } else {
            self.labels.insert(key, text);
        }
        self.dirty = true;

        self.read_mut().popup = None;
    }

    pub fn open_custom(&mut self) {
        let (register_type, address) = self.cursor_cell();

        let params = match self.custom_rules.get(&(register_type, address)) {
            Some(rule) => CustomParams {
                address,
                register_type,
                repr: rule.repr,
                ops: rule.ops.clone(),
                enum_map: rule.enum_map.clone(),
                decimals: rule.decimals.map(|d| d.to_string()).unwrap_or_default(),
                prefix: rule.prefix.clone(),
                suffix: rule.suffix.clone(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                selected: 0,
                existed: true,
                error: None,
            },
            None => CustomParams {
                address,
                register_type,
                repr: CustomRepr::default(),
                ops: Vec::new(),
                enum_map: Vec::new(),
                decimals: String::new(),
                prefix: String::new(),
                suffix: String::new(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                selected: 0,
                existed: false,
                error: None,
            },
        };
        self.read_mut().popup = Some(Popup::Custom(params));
    }

    fn with_custom(&mut self, f: impl FnOnce(&mut CustomParams)) {
        if let Some(Popup::Custom(c)) = &mut self.read_mut().popup {
            f(c);
        }
    }

    pub fn custom_move(&mut self, down: bool) {
        let n = CustomField::ALL.len() as u16;
        self.with_custom(|c| {
            c.error = None;
            c.selected = if down {
                (c.selected + 1) % n
            } else if c.selected == 0 {
                n - 1
            } else {
                c.selected - 1
            };
        });
    }

    pub fn custom_cycle(&mut self, field: CustomField, forward: bool) {
        self.with_custom(|c| {
            c.error = None;
            if field == CustomField::Repr {
                let all = CustomRepr::ALL;
                let i = all.iter().position(|&r| r == c.repr).unwrap_or(0);
                let n = all.len();
                c.repr = if forward {
                    all[(i + 1) % n]
                } else {
                    all[(i + n - 1) % n]
                };
            }
        });
    }

    pub fn custom_char(&mut self, field: CustomField, ch: char) {
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Ops => c.op_buffer.push(ch),
                CustomField::Enum => c.enum_buffer.push(ch),
                CustomField::Decimals => {
                    if ch.is_ascii_digit() && c.decimals.len() < 2 {
                        c.decimals.push(ch);
                    }
                }
                CustomField::Prefix => c.prefix.push(ch),
                CustomField::Suffix => c.suffix.push(ch),
                _ => {}
            }
        });
    }

    pub fn custom_backspace(&mut self, field: CustomField) {
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Ops => {
                    if c.op_buffer.pop().is_none() {
                        c.ops.pop();
                    }
                }
                CustomField::Enum => {
                    if c.enum_buffer.pop().is_none() {
                        c.enum_map.pop();
                    }
                }
                CustomField::Decimals => {
                    c.decimals.pop();
                }
                CustomField::Prefix => {
                    c.prefix.pop();
                }
                CustomField::Suffix => {
                    c.suffix.pop();
                }
                _ => {}
            }
        });
    }

    pub fn custom_enter(&mut self, field: CustomField) {
        match field {
            CustomField::Ops => self.with_custom(|c| {
                if c.op_buffer.trim().is_empty() {
                    return;
                }
                match parse_op(&c.op_buffer) {
                    Ok(op) => {
                        c.ops.push(op);
                        c.op_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("op: {e}")),
                }
            }),
            CustomField::Enum => self.with_custom(|c| {
                if c.enum_buffer.trim().is_empty() {
                    return;
                }
                match parse_enum(&c.enum_buffer) {
                    Ok(entry) => {
                        c.enum_map.push(entry);
                        c.enum_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("enum: {e}")),
                }
            }),
            CustomField::Save => self.commit_custom(),
            CustomField::Remove => self.remove_custom(),
            _ => {}
        }
    }

    pub fn commit_custom(&mut self) {
        let built = match &self.read().popup {
            Some(Popup::Custom(c)) => build_custom_rule(c),
            _ => return,
        };
        match built {
            Ok((cell, rule)) => {
                self.custom_rules.insert(cell, rule);
                self.dirty = true;
                self.read_mut().popup = None;
                log::info!("Custom rule set \u{b7} {:?}@{}", cell.0, cell.1);
            }
            Err(e) => self.with_custom(|c| c.error = Some(e)),
        }
    }

    pub fn remove_custom(&mut self) {
        let cell = match &self.read().popup {
            Some(Popup::Custom(c)) => (c.register_type, c.address),
            _ => return,
        };
        if self.custom_rules.remove(&cell).is_some() {
            self.dirty = true;
            log::info!("Custom rule removed \u{b7} {:?}@{}", cell.0, cell.1);
        }
        self.read_mut().popup = None;
    }

    fn persist_config(&mut self) -> String {
        self.config.labels = (&self.labels).into();

        let rebuilt: CustomRules = (&self.custom_rules).into();
        self.config.custom_rules.holdings = rebuilt.holdings;
        self.config.custom_rules.inputs = rebuilt.inputs;

        let mut pinned = PinnedRegisters::default();
        for (kind, address) in &self.pinned_registers {
            match kind {
                RegisterType::Holding => pinned.holdings.push(*address),
                RegisterType::Input => pinned.inputs.push(*address),
            }
        }
        self.config.pinned_registers = pinned;

        self.config.interpretations = self.interpreter.config();
        if let State::Read(p) = &self.state {
            self.config.startup = Startup {
                address: p.position,
                register_type: p.register_type,
                panel: p.panel,
            };
        }

        match save_config(&self.config_path, &self.config) {
            Ok(()) => format!("Saved to {}", self.config_path),
            Err(e) => format!("Save failed: {e}"),
        }
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    pub fn read_count(&self) -> usize {
        self.read_log.len()
    }

    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    fn dump_read_log(&self) -> String {
        if self.read_log.is_empty() {
            return "Nothing read yet to dump.".to_string();
        }

        let filename = format!("dump_{}.txt", Local::now().format("%Y%m%d_%H%M%S"));

        let mut out = String::from("read_at\ttype\taddress\thex\tdecimal\tlabel\n");
        for (&(kind, address), &(value, read_at)) in &self.read_log {
            let label = self
                .labels
                .get(&(kind, address))
                .cloned()
                .unwrap_or_default();
            out.push_str(&format!(
                "{}\t{kind:?}\t{address}\t{value:04X}\t{value}\t{label}\n",
                read_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            ));
        }

        match fs::write(&filename, out) {
            Ok(()) => format!("Dumped {} registers to {filename}", self.read_log.len()),
            Err(e) => format!("Dump failed: {e}"),
        }
    }

    pub fn pin(&mut self) {
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };

        let selection = match panel {
            ReadPanel::Main => (register_type, position),
            _ => match self.panel_cell_at(pinned_index as usize) {
                Some(cell) => cell,
                None => return,
            },
        };

        if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
        } else {
            self.pinned_registers.push(selection);
        }

        self.pinned_registers.sort();
        self.dirty = true;

        let rows = self.visible_rows.get();
        let len = self.panel_len();
        self.read_mut().scroll_pinned(rows, len);
    }

    pub fn settings_save(&mut self) {
        let result = self.persist_config();
        if result.starts_with("Saved") {
            self.dirty = false;
            log::info!("Configuration saved");
        } else {
            log::error!("Save failed \u{b7} {result}");
        }
        if let Some(s) = self.settings_mut() {
            s.status = Some(result);
        }
    }

    pub async fn settings_load(&mut self) {
        let Some(path) = self.settings().map(|s| s.load_path.trim().to_string()) else {
            return;
        };
        let result = self.load_config_from(&path).await;
        match &result {
            Ok(message) => log::info!("{message}"),
            Err(error) => log::error!("{error}"),
        }
        if let Some(s) = self.settings_mut() {
            s.status = Some(result.unwrap_or_else(|e| e));
        }
    }

    async fn load_config_from(&mut self, path: &str) -> Result<String, String> {
        if path.is_empty() {
            return Err("Load failed: enter a file name".to_string());
        }
        let content = fs::read_to_string(path).map_err(|e| format!("Load failed: {e}"))?;
        let config: Config =
            serde_json::from_str(&content).map_err(|e| format!("Load failed: {e}"))?;

        let device = ModbusDevice::new(&config.device)
            .await
            .map_err(|e| format!("Load failed: device: {e}"))?;

        self.apply_config(config, Some(device));
        self.dirty = true;

        let read = self.startup_read_params();
        if let Some(s) = self.settings_mut() {
            s.previous = read;
        }

        Ok(format!("Loaded {path}"))
    }

    pub fn commit_dump(&mut self) {
        let result = self.dump_read_log();
        if let Some(Popup::Dump(d)) = &mut self.read_mut().popup {
            d.result = Some(result);
        }
    }

    pub async fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        if matches!(self.state, State::Read(_)) {
            let rows = self.visible_rows.get();
            let cols = self.config.matrix_cols;
            self.read_mut().scroll_to_cursor(rows, cols);
        }

        let should_refresh = !self.paused
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

    async fn aquire_data_with(
        device: &ModbusDevice,
        amount: u16,
        position: u16,
        register_type: RegisterType,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let values = if register_type == RegisterType::Holding {
            device.holdings(position, amount).await?
        } else {
            device.inputs(position, amount).await?
        };

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

            let values = match kind {
                RegisterType::Holding => device.holdings(start_addr, run_len as u16).await?,
                RegisterType::Input => device.inputs(start_addr, run_len as u16).await?,
            };
            anyhow::ensure!(
                values.len() == run_len,
                "Expected {run_len} value(s) at {start_addr}, got {}",
                values.len()
            );

            for j in 0..run_len {
                let cell = regs[i + j];
                let value = values[j];

                collection.push((cell, value));
            }

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

        let amount = self.config.registers_batch.max(1);
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
        let read_start = position.saturating_sub(amount / 2).min(max_read_start);
        self.connection = ConnectionStatus::Reading;

        let panel_registers = {
            let total = self.panel_len() as usize;
            if total == 0 {
                Vec::new()
            } else {
                let batch = (amount as usize).min(total);
                let idx = (self.read().pinned_index as usize).min(total - 1);
                let start = idx.saturating_sub(batch / 2).min(total - batch);
                self.panel_window(start, batch)
            }
        };

        self.background_task = Some(BackgroundTask::Refresh(compat::spawn(async move {
            let read_began = Instant::now();
            let (main_data, pinned_data) = match panel {
                ReadPanel::Main | ReadPanel::Matrix => {
                    let main = Self::aquire_data_with(&device, amount, read_start, register_type)
                        .await
                        .map_err(|e| e.to_string());
                    (Some(main), None)
                }
                _ => {
                    let pinned = Self::aquire_pinned_data_with(&device, &panel_registers, amount)
                        .await
                        .map_err(|e| e.to_string());
                    (None, Some(pinned))
                }
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
    }

    fn custom_value(
        &self,
        cell: RegisterCell,
        value: u16,
        word_order: WordOrder,
        neighbor: &impl Fn(u16) -> Option<u16>,
    ) -> Option<String> {
        let (kind, address) = cell;
        let Some(rule) = self.custom_rules.get(&cell) else {
            if !self.config.custom_rules.show_continuation {
                return None;
            }
            let prev = address.checked_sub(1)?;
            let prev_rule = self.custom_rules.get(&(kind, prev))?;
            return (prev_rule.repr.register_count() == 2).then(|| "part of \u{2191}".to_string());
        };
        let mut words = vec![value];
        if rule.repr.register_count() == 2 {
            if let Some(n) = neighbor(1) {
                words.push(n);
            }
        }
        let formatted = rule.evaluate(&words, word_order);
        (!formatted.is_empty()).then_some(formatted)
    }

    pub fn custom_preview(&self, c: &CustomParams) -> Result<(String, String), String> {
        let (cell, rule) = build_custom_rule(c)?;
        let Some(&(value, _)) = self.read_log.get(&cell) else {
            return Err("no value read yet".to_string());
        };
        let mut words = vec![value];
        if rule.repr.register_count() == 2 {
            match self.read_log.get(&(cell.0, cell.1.saturating_add(1))) {
                Some(&(n, _)) => words.push(n),
                None => return Err("waiting for second register".to_string()),
            }
        }
        let output = rule.evaluate(&words, self.config.device.word_order);
        if output.is_empty() {
            return Err("no output".to_string());
        }
        let input = words
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Ok((input, output))
    }

    pub fn cell_row(&self, cell: RegisterCell, now: DateTime<Local>) -> Option<(String, bool)> {
        let (kind, addr) = cell;
        let &(value, time) = self.read_log.get(&cell)?;
        let neighbor = |offset: u16| {
            self.read_log
                .get(&(kind, addr.saturating_add(offset)))
                .map(|&(v, _)| v)
        };
        let custom = self.custom_value(cell, value, self.config.device.word_order, &neighbor);
        let label = self.labels.get(&cell).map(String::as_str);
        let row = self.interpreter.format_row(
            addr,
            value,
            [neighbor(1), neighbor(2), neighbor(3)],
            time.with_timezone(&Local),
            now,
            custom.as_deref(),
            label,
        );
        Some((row, self.cell_changed(cell)))
    }

    pub fn ascii_string_for(&self, cells: impl Iterator<Item = RegisterCell>) -> String {
        let values: Vec<RegisterCellValue> = cells
            .filter_map(|cell| self.read_log.get(&cell).map(|&(value, _)| (cell, value)))
            .collect();
        self.interpreter.ascii_string(&values)
    }

    pub fn toggle_column(&mut self, column: Column) {
        self.interpreter.toggle(column);
    }

    pub fn label_text(&self, register_type: RegisterType, address: u16) -> Option<String> {
        self.labels.get(&(register_type, address)).cloned()
    }

    pub fn commit_write(&mut self) {
        if self.config.read_only {
            if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
                w.result = Some("Read-only mode.".to_string());
            }
            return;
        }
        if self.background_task.is_some() {
            if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
                w.result = Some("Device is busy.".to_string());
            }
            return;
        }

        let (position, number, write_type) = {
            let Some(Popup::Write(w)) = &mut self.read_mut().popup else {
                return;
            };
            let Some(number) = w.value else {
                w.result = Some("Enter a value first.".to_string());
                return;
            };
            w.result = Some("Writing...".to_string());
            (w.position, number, w.write_type)
        };

        let Some(device) = self.device.clone() else {
            return;
        };

        let cell = (RegisterType::Holding, position);
        let (previous, new_value) = match write_type {
            WriteType::Word => (
                self.previous_values.get(&cell).map(|&v| v as u64),
                (number as u16) as u64,
            ),
            WriteType::DWord => {
                let order = self.config.device.word_order;
                let lo = self.previous_values.get(&cell);
                let hi = self
                    .previous_values
                    .get(&(RegisterType::Holding, position.wrapping_add(1)));
                let previous = match (lo, hi) {
                    (Some(&a), Some(&b)) => Some(order.make_word(a, b) as u64),
                    _ => None,
                };
                (previous, (number as u32) as u64)
            }
        };
        self.pending_write = Some(PendingWrite {
            address: position,
            write_type,
            previous,
            new_value,
        });

        self.background_task = Some(BackgroundTask::Write(compat::spawn(async move {
            let result = match write_type {
                WriteType::Word => device.write_register(position, number as u16).await,
                WriteType::DWord => device.write_register_word(position, number as i32).await,
            };
            match result {
                Ok(()) => WriteOutcome {
                    ok: true,
                    message: "Write OK".to_string(),
                },
                Err(e) => WriteOutcome {
                    ok: false,
                    message: format!("Write failed: {e}"),
                },
            }
        })));
    }

    fn write_bit_count(&self) -> u16 {
        match &self.read().popup {
            Some(Popup::Write(w)) => match w.write_type {
                WriteType::Word => 16,
                WriteType::DWord => 32,
            },
            _ => 16,
        }
    }

    pub fn write_toggle_type(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            w.write_type = match w.write_type {
                WriteType::Word => WriteType::DWord,
                WriteType::DWord => WriteType::Word,
            };
            let bits = match w.write_type {
                WriteType::Word => 16,
                WriteType::DWord => 32,
            };
            w.bit_cursor = w.bit_cursor.min(bits - 1);
        }
        self.clamp_write_value();
    }

    pub fn clamp_write_value(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            if let Some(value) = w.value {
                let (lo, hi) = match w.write_type {
                    WriteType::Word => (i16::MIN as i64, u16::MAX as i64),
                    WriteType::DWord => (i32::MIN as i64, u32::MAX as i64),
                };
                w.value = Some(value.clamp(lo, hi));
            }
        }
    }

    pub fn write_move_bit(&mut self, left: bool) {
        let bits = self.write_bit_count();
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            w.bit_cursor = if left {
                (w.bit_cursor + 1).min(bits - 1)
            } else {
                w.bit_cursor.saturating_sub(1)
            };
        }
    }

    pub fn write_toggle_bit(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            let mask = 1u32 << w.bit_cursor;
            let current = w.value.unwrap_or(0) as u32;
            w.value = Some((current ^ mask) as i64);
        }
    }

    pub async fn complete_background_task(&mut self) {
        enum Done {
            Refresh(Option<RefreshTaskResult>),
            Write(Option<WriteOutcome>),
        }

        let done = match self.background_task.as_mut() {
            None => return,
            Some(BackgroundTask::Refresh(handle)) => match handle.poll_result() {
                TaskPoll::Pending => return,
                TaskPoll::Finished(result) => Done::Refresh(Some(result)),
                TaskPoll::Gone => Done::Refresh(None),
            },
            Some(BackgroundTask::Write(handle)) => match handle.poll_result() {
                TaskPoll::Pending => return,
                TaskPoll::Finished(outcome) => Done::Write(Some(outcome)),
                TaskPoll::Gone => Done::Write(None),
            },
        };
        self.background_task = None;

        match done {
            Done::Refresh(Some(result)) => self.apply_refresh_result(result),
            Done::Refresh(None) => {
                let message = "read task stopped unexpectedly".to_string();
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
                    if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
                        w.result = Some(outcome.message);
                    }
                }
            }
        }
    }

    pub fn toggle_type(&mut self) {
        let p = self.read_mut();
        p.read_duration = None;
        p.read_error = None;
        p.register_type.toggle();
    }
}
