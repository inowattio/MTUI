use crate::config::{Column, Config, Label, Labels, Startup};
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::{
    DeviceConfig, Interface, InterfaceNetworkParams, InterfaceWiredParams, ModbusDevice,
};
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    ConnectionStatus, DiscoveryParams, DumpParams, InterfaceKind, LabelParams, LogViewParams,
    LogsParams, Popup, PopupKind, ReadPanel, ReadParams, SearchParams, SettingsField,
    SettingsParams, State, WriteParams,
};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};
use std::{error, fs};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord,
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
enum BackgroundTask {
    Refresh(JoinHandle<RefreshTaskResult>),
    Write(JoinHandle<WriteOutcome>),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: DateTime<Local>,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug)]
struct RefreshTaskResult {
    window_start: u16,
    register_type: RegisterType,
    main_data: Option<Result<Vec<RegisterCellValue>, String>>,
    pinned_data: Option<Result<Vec<RegisterCellValue>, String>>,
    read_duration: Duration,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
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
    pending_write: Option<PendingWrite>,
    logs: VecDeque<LogEntry>,
    logged_connection: ConnectionStatus,
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

fn save_config(config: &Config) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(CONFIG_PATH, content).map_err(|e| e.to_string())
}

fn dump_example_config_and_exit() {
    let example_config = Config::default();
    let config_string = serde_json::to_string_pretty(&example_config).unwrap();

    fs::write(CONFIG_PATH, config_string).unwrap();
    println!("No config file found, dumped example.");
    std::process::exit(0)
}

fn fetch_config_or_exit() -> Config {
    let content = fs::read_to_string(CONFIG_PATH)
        .inspect_err(|_| dump_example_config_and_exit())
        .unwrap();
    serde_json::from_str(&content)
        .inspect_err(|e| println!("Could not parse config: {e}"))
        .unwrap()
}

impl App {
    pub async fn new() -> Self {
        let config = fetch_config_or_exit();
        let device = ModbusDevice::new(&config.device)
            .await
            .inspect_err(|e| println!("Could not initialize device: {e}"))
            .ok();
        let initial_rows = config.registers_batch.max(1);

        let state = if device.is_some() {
            State::Read(ReadParams {
                position: config.startup.address,
                window_start: config.startup.address,
                register_type: config.startup.register_type,
                ..Default::default()
            })
        } else {
            State::Discovery(Self::discovery_params(&config))
        };

        let mut app = Self {
            interpreter: Interpretor::new(config.interpretations.clone(), config.device.word_order),
            pinned_registers: config.pinned_registers.clone().into(),
            labels: config.labels.clone().into(),
            state,
            config,
            device,
            running: true,
            connection: ConnectionStatus::Unknown,
            frame: 0,
            paused: false,
            dirty: false,
            visible_rows: Cell::new(initial_rows),
            previous_position: None,
            background_task: None,
            previous_values: BTreeMap::new(),
            changed: BTreeMap::new(),
            read_log: BTreeMap::new(),
            value_history: BTreeMap::new(),
            pending_write: None,
            logs: VecDeque::new(),
            logged_connection: ConnectionStatus::Unknown,
        };

        if app.device.is_some() {
            let device = app.config.display_device();
            app.log_info(format!("Started \u{b7} {device}"));
        } else {
            app.log_warn("Started \u{b7} no device, opened Discovery");
        }

        app
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
        self.state = State::Settings(SettingsParams::default());
    }

    fn is_reading(&self) -> bool {
        matches!(self.state, State::Read(_))
    }

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
        self.state = State::Read(ReadParams {
            position: self.config.startup.address,
            window_start: self.config.startup.address,
            register_type: self.config.startup.register_type,
            ..Default::default()
        });
    }

    pub async fn discovery_connect(&mut self) {
        let device_config = {
            let Some(d) = self.discovery() else {
                return;
            };
            let interface = match d.interface {
                InterfaceKind::Mock => Interface::Mock,
                InterfaceKind::Wired => Interface::Wired(InterfaceWiredParams {
                    path: d.ports.get(d.port_index as usize).cloned().unwrap_or_default(),
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
                self.log_info(format!("Switched device \u{b7} {device}"));
                self.state = State::Read(ReadParams {
                    position: self.config.startup.address,
                    window_start: self.config.startup.address,
                    register_type: self.config.startup.register_type,
                    ..Default::default()
                });
            }
            Err(e) => {
                self.log_error(format!("Connect failed \u{b7} {e}"));
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

    pub fn clear_pins(&mut self) {
        let n = self.pinned_registers.len();
        self.pinned_registers.clear();
        self.dirty = true;
        self.log_info(format!("Cleared {n} pinned register(s)"));
    }

    pub fn clear_labels(&mut self) {
        let n = self.labels.len();
        self.labels.clear();
        self.dirty = true;
        self.log_info(format!("Cleared {n} label(s)"));
    }

    pub fn writes_log_path(&self) -> std::path::PathBuf {
        let kind = match &self.config.device.interface {
            Interface::Mock => "mock",
            Interface::Wired(_) => "wired",
            Interface::Network(_) => "network",
        };
        let name = format!("writes_{kind}_{}.txt", self.config.device.slave_id);
        std::env::temp_dir().join(name)
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

    fn push_log(&mut self, level: LogLevel, message: impl Into<String>) {
        const LOG_CAP: usize = 1000;
        self.logs.push_back(LogEntry {
            time: Local::now(),
            level,
            message: message.into(),
        });
        while self.logs.len() > LOG_CAP {
            self.logs.pop_front();
        }
    }

    fn log_info(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Info, message);
    }

    fn log_warn(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Warn, message);
    }

    fn log_error(&mut self, message: impl Into<String>) {
        self.push_log(LogLevel::Error, message);
    }

    pub fn activity_logs(&self) -> &VecDeque<LogEntry> {
        &self.logs
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
        let len = self.logs.len() as i32;
        let visible = self.visible_rows.get().max(1) as i32;
        let max_top = (len - visible).max(0);
        if let Some(l) = self.log_view_mut() {
            let new = (l.top as i32 + delta).clamp(0, max_top);
            l.top = new as u16;
            l.follow = new >= max_top;
        }
    }

    fn log_write(&self) {
        use std::io::Write as _;

        if !self.config.log_writes {
            return;
        }
        let Some(pending) = self.pending_write.as_ref() else {
            return;
        };

        let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
        let kind = match pending.write_type {
            WriteType::Word => "word",
            WriteType::DWord => "dword",
        };
        let previous = pending
            .previous
            .map_or_else(|| "?".to_string(), |v| v.to_string());
        let line = format!(
            "{timestamp} | {} | {kind} | {previous} | {}\n",
            pending.address, pending.new_value
        );

        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.writes_log_path())
        {
            let _ = file.write_all(line.as_bytes());
        }
    }

    pub fn settings_adjust(&mut self, field: SettingsField, delta: i64) {
        match field {
            SettingsField::RegistersBatch => {
                let n = (self.config.registers_batch as i64 + delta).clamp(1, u16::MAX as i64);
                self.config.registers_batch = n as u16;
            }
            SettingsField::AutoUpdate => {
                let cur = self.config.auto_update_interval_seconds.unwrap_or(0) as i64;
                let n = (cur + delta).max(0);
                self.config.auto_update_interval_seconds = (n > 0).then_some(n as u64);
            }
            SettingsField::HistoryCap => {
                let n = (self.config.graph_history_cap as i64 + delta).clamp(1, u16::MAX as i64);
                self.config.graph_history_cap = n as u16;
            }
            SettingsField::ReadOnly => self.config.read_only = !self.config.read_only,
            SettingsField::LogWrites => self.config.log_writes = !self.config.log_writes,
            SettingsField::ClearPins | SettingsField::ClearLabels | SettingsField::Save => return,
        }
        self.dirty = true;
    }

    pub fn settings_digit(&mut self, field: SettingsField, digit: u8) {
        let digit = digit as u64;
        match field {
            SettingsField::RegistersBatch => {
                let n = (self.config.registers_batch as u64).saturating_mul(10) + digit;
                self.config.registers_batch = n.min(u16::MAX as u64) as u16;
            }
            SettingsField::AutoUpdate => {
                let cur = self.config.auto_update_interval_seconds.unwrap_or(0);
                let n = cur.saturating_mul(10) + digit;
                self.config.auto_update_interval_seconds = (n > 0).then_some(n);
            }
            SettingsField::HistoryCap => {
                let n = (self.config.graph_history_cap as u64).saturating_mul(10) + digit;
                self.config.graph_history_cap = n.min(u16::MAX as u64) as u16;
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub fn settings_backspace(&mut self, field: SettingsField) {
        match field {
            SettingsField::RegistersBatch => {
                self.config.registers_batch = (self.config.registers_batch / 10).max(1);
            }
            SettingsField::AutoUpdate => {
                let n = self.config.auto_update_interval_seconds.unwrap_or(0) / 10;
                self.config.auto_update_interval_seconds = (n > 0).then_some(n);
            }
            SettingsField::HistoryCap => {
                self.config.graph_history_cap = (self.config.graph_history_cap / 10).max(1);
            }
            _ => return,
        }
        self.dirty = true;
    }

    pub fn toggle_graph(&mut self) {
        let p = self.read_mut();
        p.graph = !p.graph;
    }

    pub fn copy_address(&mut self) {
        let p = self.read();
        let address = if p.panel == ReadPanel::Pinned {
            self.pinned_registers
                .get(p.pinned_index as usize)
                .map(|&(_, addr)| addr)
                .unwrap_or(p.position)
        } else {
            p.position
        };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(address.to_string());
        }
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

        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        let (write_type, write_pos) = if panel == ReadPanel::Pinned {
            self.pinned_registers
                .get(pinned_index as usize)
                .map(|&(kind, address)| (kind, address))
                .unwrap_or((register_type, position))
        } else {
            (register_type, position)
        };

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
            self.log_info(format!("Slave id set to {id}"));
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
        self.rebuild_read_rows();
    }

    pub fn request_quit(&mut self) {
        if self.dirty {
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
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        let (label_type, label_pos) = if panel == ReadPanel::Pinned {
            self.pinned_registers
                .get(pinned_index as usize)
                .map(|&(kind, address)| (kind, address))
                .unwrap_or((register_type, position))
        } else {
            (register_type, position)
        };
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
        let p = self.read_mut();
        let type_changed = register_type != p.register_type;
        p.panel = ReadPanel::Main;
        p.position = position;
        p.register_type = register_type;
        p.scroll_to_cursor(rows);
        if type_changed {
            p.main_rows = Vec::new();
            p.main_changed = Vec::new();
        }
        p.popup = None;
        self.rebuild_read_rows();
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
        let p = self.read_mut();
        p.panel = ReadPanel::Main;
        p.register_type = register_type;
        p.position = position;
        p.scroll_to_cursor(rows);
        self.rebuild_read_rows();
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
        self.rebuild_read_rows();
    }

    fn persist_config(&mut self) -> String {
        self.config.labels = (&self.labels).into();

        let mut pinned = PinnedRegisters::default();
        for (kind, address) in &self.pinned_registers {
            match kind {
                RegisterType::Holding => pinned.holdings.push(*address),
                RegisterType::Input => pinned.inputs.push(*address),
            }
        }
        self.config.pinned_registers = pinned;

        self.config.interpretations = self.interpreter.config();
        // Capture the live cursor as the new startup point, but only when a read
        // view exists (Save is reachable from the Settings state, where it does not).
        if let State::Read(p) = &self.state {
            self.config.startup = Startup {
                address: p.position,
                register_type: p.register_type,
            };
        }

        match save_config(&self.config) {
            Ok(()) => format!("Saved to {CONFIG_PATH}"),
            Err(e) => format!("Save failed: {e}"),
        }
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
            let label = self.labels.get(&(kind, address)).cloned().unwrap_or_default();
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
            ReadPanel::Pinned => match self.pinned_registers.get(pinned_index as usize) {
                Some(&cell) => cell,
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
        let len = self.pinned_registers.len() as u16;
        self.read_mut().scroll_pinned(rows, len);
    }

    pub fn settings_save(&mut self) {
        let result = self.persist_config();
        if result.starts_with("Saved") {
            self.dirty = false;
            self.log_info("Configuration saved");
        } else {
            self.log_error(format!("Save failed \u{b7} {result}"));
        }
        if let Some(s) = self.settings_mut() {
            s.status = Some(result);
        }
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
            let p = self.read_mut();
            let before = p.window_start;
            p.scroll_to_cursor(rows);
            if p.window_start != before {
                self.rebuild_read_rows();
            }
        }

        let should_refresh = !self.paused
            && matches!(
                &self.state,
                State::Read(p)
                    if self.config.auto_update_interval_seconds
                        .is_some_and(|seconds| p.refresh_timer.elapsed().as_secs() > seconds)
            );

        if should_refresh {
            self.refresh().await;
        } else if self.interpreter.is_enabled(Column::Ago) {
            self.rebuild_read_rows();
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        self.log_info(if self.paused {
            "Auto-refresh paused"
        } else {
            "Auto-refresh resumed"
        });
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

            for j in 0..run_len {
                let cell = regs[i + j];
                let value = values.get(j).cloned().unwrap();

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

        // The display window follows the viewport (cursor stays centered), but
        // the read fetches exactly `registers_batch` registers centered on the
        // cursor so the focused register always has fresh data.
        let amount = self.config.registers_batch.max(1);
        let visible = self.visible_rows.get().max(1);
        let (panel, window_start, position, register_type) = {
            let p = self.read_mut();
            p.refresh_timer = Instant::now();
            p.loading = true;
            p.scroll_to_cursor(visible);
            (p.panel, p.window_start, p.position, p.register_type)
        };
        let max_read_start = u16::MAX - (amount - 1);
        let read_start = position.saturating_sub(amount / 2).min(max_read_start);
        self.connection = ConnectionStatus::Reading;

        // On the pinned panel, refresh only the `registers_batch` pins centered
        // on the pinned cursor (like the main window) instead of every pin.
        let pinned_registers = {
            let pins = &self.pinned_registers;
            let total = pins.len();
            if total == 0 {
                Vec::new()
            } else {
                let batch = (amount as usize).min(total);
                let idx = (self.read().pinned_index as usize).min(total - 1);
                let start = idx.saturating_sub(batch / 2).min(total - batch);
                pins[start..start + batch].to_vec()
            }
        };

        self.background_task = Some(BackgroundTask::Refresh(tokio::spawn(async move {
            let read_began = Instant::now();
            let (main_data, pinned_data) = match panel {
                ReadPanel::Main => {
                    let main = Self::aquire_data_with(&device, amount, read_start, register_type)
                        .await
                        .map_err(|e| e.to_string());
                    (Some(main), None)
                }
                ReadPanel::Pinned => {
                    let pinned = Self::aquire_pinned_data_with(&device, &pinned_registers, amount)
                        .await
                        .map_err(|e| e.to_string());
                    (None, Some(pinned))
                }
            };
            let read_duration = read_began.elapsed();

            RefreshTaskResult {
                window_start,
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
                let did_change = matches!(self.previous_values.get(&cell), Some(&prev) if prev != value);
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
        let read_ok =
            matches!(&result.main_data, Some(Ok(_))) || matches!(&result.pinned_data, Some(Ok(_)));

        {
            let params = self.read_mut();
            params.read_duration = Some(result.read_duration);
            params.loading = false;
            if let Some(Err(e)) = &result.main_data {
                params.main_rows = vec![e.clone()];
                params.main_changed = Vec::new();
                params.data_start = result.window_start;
            }
        }

        if read_ok {
            self.rebuild_read_rows();
        }

        if connection != self.logged_connection {
            match &connection {
                ConnectionStatus::Connected => self.log_info("Connected"),
                ConnectionStatus::Error(e) => self.log_error(format!("Read error \u{b7} {e}")),
                _ => {}
            }
            self.logged_connection = connection.clone();
        }
        self.connection = connection;
    }

    pub fn rebuild_read_rows(&mut self) {
        if !self.is_reading() {
            return;
        }
        let visible = self.visible_rows.get().max(1);
        let (window_start, register_type) = {
            let p = self.read();
            (p.window_start, p.register_type)
        };
        let now = Local::now();

        let mut main_rows = Vec::with_capacity(visible as usize);
        let mut main_changed = Vec::with_capacity(visible as usize);
        let mut window_values: Vec<RegisterCellValue> = Vec::new();
        for i in 0..visible {
            let addr = window_start.saturating_add(i);
            let cell = (register_type, addr);
            let label = self.labels.get(&cell).map(String::as_str);
            match self.read_log.get(&cell) {
                Some(&(value, time)) => {
                    let neighbor = |offset: u16| {
                        self.read_log
                            .get(&(register_type, addr.saturating_add(offset)))
                            .map(|&(v, _)| v)
                    };
                    main_rows.push(self.interpreter.format_row(
                        addr,
                        value,
                        [neighbor(1), neighbor(2), neighbor(3)],
                        time.with_timezone(&Local),
                        now,
                        label,
                    ));
                    main_changed.push(self.changed.get(&cell).copied().unwrap_or(false));
                    window_values.push((cell, value));
                }
                None => {
                    main_rows.push(self.interpreter.placeholder(addr, label));
                    main_changed.push(false);
                }
            }
        }
        let main_ascii = self.interpreter.ascii_string(&window_values);

        let pins = self.pinned_registers.clone();
        let mut pinned_rows = Vec::with_capacity(pins.len());
        let mut pinned_changed = Vec::with_capacity(pins.len());
        let mut pinned_values: Vec<RegisterCellValue> = Vec::new();
        for &(kind, addr) in &pins {
            let cell = (kind, addr);
            let label = self.labels.get(&cell).map(String::as_str);
            match self.read_log.get(&cell) {
                Some(&(value, time)) => {
                    let neighbor = |offset: u16| {
                        self.read_log
                            .get(&(kind, addr.saturating_add(offset)))
                            .map(|&(v, _)| v)
                    };
                    pinned_rows.push(self.interpreter.format_row(
                        addr,
                        value,
                        [neighbor(1), neighbor(2), neighbor(3)],
                        time.with_timezone(&Local),
                        now,
                        label,
                    ));
                    pinned_changed.push(self.changed.get(&cell).copied().unwrap_or(false));
                    pinned_values.push((cell, value));
                }
                None => {
                    pinned_rows.push(self.interpreter.placeholder(addr, label));
                    pinned_changed.push(false);
                }
            }
        }
        let pinned_ascii = self.interpreter.ascii_string(&pinned_values);

        let params = self.read_mut();
        params.main_rows = main_rows;
        params.main_changed = main_changed;
        params.ascii_string = main_ascii;
        params.pinned_rows = pinned_rows;
        params.pinned_changed = pinned_changed;
        params.pinned_ascii_string = pinned_ascii;
        params.data_start = window_start;
    }

    pub fn toggle_column(&mut self, column: Column) {
        self.interpreter.toggle(column);
        self.rebuild_read_rows();
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

        self.background_task = Some(BackgroundTask::Write(tokio::spawn(async move {
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
        let Some(task) = self.background_task.as_ref() else {
            return;
        };

        if !match task {
            BackgroundTask::Refresh(handle) => handle.is_finished(),
            BackgroundTask::Write(handle) => handle.is_finished(),
        } {
            return;
        }

        let task = self.background_task.take().unwrap();

        match task {
            BackgroundTask::Refresh(handle) => match handle.await {
                Ok(result) => self.apply_refresh_result(result),
                Err(e) => {
                    let message = e.to_string();
                    if self.is_reading() {
                        let params = self.read_mut();
                        params.main_rows = vec![message.clone()];
                        params.main_changed = Vec::new();
                        params.loading = false;
                    }
                    self.log_error(format!("Read task failed \u{b7} {message}"));
                    self.connection = ConnectionStatus::Error(message);
                }
            },
            BackgroundTask::Write(handle) => {
                let outcome = handle.await.unwrap_or_else(|e| WriteOutcome {
                    ok: false,
                    message: e.to_string(),
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
                        self.log_info(format!("Write {detail}"));
                    } else {
                        self.log_error(format!("Write failed \u{b7} {detail} \u{b7} {}", outcome.message));
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
        p.main_rows = Vec::new();
        p.read_duration = None;
        p.ascii_string = String::new();
        p.main_changed = Vec::new();
        p.register_type.toggle();
    }
}
