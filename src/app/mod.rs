#[cfg(not(target_arch = "wasm32"))]
use crate::compat;
use crate::compat::{Instant, TaskHandle};
use crate::config::{Config, CustomRules, Label, Labels};
use crate::constants::CONFIG_PATH;
use crate::custom::CustomRule;
use crate::interpretator::Interpretor;
use crate::modbus::{DeviceConfig, DeviceIdAccess, ModbusDevice};
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{ConnectionStatus, CustomParams, State};
use crate::writes_log::SharedWritesLog;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type ApiDevice = Arc<Mutex<Option<ModbusDevice>>>;
pub type BoundPort = Arc<AtomicU16>;
pub type ReadOnlyFlag = Arc<AtomicBool>;
pub type AllowSlaveFlag = Arc<AtomicBool>;
pub type StatusFlag = Arc<AtomicU8>;
pub type BindStateFlag = Arc<AtomicU8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiBindState {
    Pending,
    Bound,
    Failed,
}

impl ApiBindState {
    pub fn code(self) -> u8 {
        match self {
            ApiBindState::Pending => 0,
            ApiBindState::Bound => 1,
            ApiBindState::Failed => 2,
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            1 => ApiBindState::Bound,
            2 => ApiBindState::Failed,
            _ => ApiBindState::Pending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord,
    Coil,
}

impl WriteType {
    fn bits(self) -> u16 {
        match self {
            WriteType::Coil => 1,
            WriteType::Word => 16,
            WriteType::DWord => 32,
        }
    }
}

pub type AppResult<T> = anyhow::Result<T>;

#[derive(Debug)]
enum BackgroundTask {
    Refresh(TaskHandle<RefreshTaskResult>),
    Write(TaskHandle<WriteOutcome>),
    Reconnect(TaskHandle<Result<ModbusDevice, String>>),
    Connect(TaskHandle<ConnectTaskResult>),
    DeviceId(TaskHandle<DeviceIdTaskResult>),
    Raw(TaskHandle<RawTaskResult>),
    LoadConfig(TaskHandle<LoadConfigTaskResult>),
}

#[derive(Debug)]
struct ConnectTaskResult {
    config: DeviceConfig,
    result: Result<ModbusDevice, String>,
}

#[derive(Debug)]
struct DeviceIdTaskResult {
    access: DeviceIdAccess,
    result: Result<Vec<(u8, String)>, String>,
}

#[derive(Debug)]
struct RawTaskResult {
    code: u8,
    sent: usize,
    result: Result<Vec<u8>, String>,
}

#[derive(Debug)]
struct LoadConfigTaskResult {
    path: String,
    config: Box<Config>,
    result: Result<ModbusDevice, String>,
}

#[derive(Debug)]
struct ScanProgress {
    done: Arc<AtomicUsize>,
    total: usize,
}

#[cfg(not(target_arch = "wasm32"))]
fn subnet_prefix_from(ip: &str) -> Option<String> {
    let octets: Vec<&str> = ip.split('.').take(3).collect();
    if octets.len() == 3 && octets.iter().all(|o| o.parse::<u8>().is_ok()) {
        Some(format!("{}.{}.{}.", octets[0], octets[1], octets[2]))
    } else {
        None
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn scan_subnet(
    prefix: String,
    port: u16,
    per_host: Duration,
    done: Arc<AtomicUsize>,
) -> Vec<String> {
    use futures::stream::{self, StreamExt};

    let mut found: Vec<(u16, String)> = stream::iter(1u16..=254)
        .map(|host| {
            let ip = format!("{prefix}{host}");
            let done = done.clone();
            async move {
                let connected = matches!(
                    compat::timeout(
                        per_host,
                        tokio::net::TcpStream::connect((ip.as_str(), port))
                    )
                    .await,
                    Ok(Ok(_))
                );
                done.fetch_add(1, Ordering::Relaxed);
                connected.then_some((host, ip))
            }
        })
        .buffer_unordered(256)
        .filter_map(|hit| async move { hit })
        .collect()
        .await;

    found.sort_by_key(|(host, _)| *host);
    found.into_iter().map(|(_, ip)| ip).collect()
}

const RECONNECT_BASE_MS: u64 = 1_000;
const RECONNECT_CAP_MS: u64 = 30_000;

fn reconnect_backoff(attempts: u32) -> Duration {
    let shift = attempts.saturating_sub(1).min(5);
    Duration::from_millis((RECONNECT_BASE_MS << shift).min(RECONNECT_CAP_MS))
}

fn fuzzy_rank<T: Copy>(query: &str, items: &[T], label: impl Fn(T) -> &'static str) -> Vec<T> {
    let mut scored: Vec<(i32, usize, T)> = items
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(i, item)| fuzzy_score(query, label(item)).map(|score| (score, i, item)))
        .collect();

    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, item)| item).collect()
}

fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let text = text.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();

    if let Some(pos) = text.find(&query) {
        return Some(1000 - pos as i32);
    }

    let mut chars = text.chars();
    for qc in query.chars() {
        loop {
            match chars.next() {
                Some(tc) if tc == qc => break,
                Some(_) => continue,
                None => return None,
            }
        }
    }
    Some(0)
}

#[derive(Debug, Default)]
struct ReconnectState {
    attempts: u32,
    next_at: Option<Instant>,
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

#[derive(Clone, Debug)]
pub struct SweepState {
    pub active: bool,
    pub from: u16,
    pub to: u16,
    pub continuous: bool,
    pub current: u16,
    pub errored: bool,
}

impl Default for SweepState {
    fn default() -> Self {
        Self {
            active: false,
            from: 0,
            to: 100,
            continuous: true,
            current: 0,
            errored: false,
        }
    }
}

#[derive(Debug, Default)]
pub struct CommStats {
    pub reads_ok: u64,
    pub read_errors: u64,
    pub writes_ok: u64,
    pub write_errors: u64,
    latency_sum: Duration,
    latency_min: Option<Duration>,
    latency_max: Option<Duration>,
    last_error: Option<(String, DateTime<Utc>)>,
}

impl CommStats {
    pub fn record_read_ok(&mut self, latency: Duration) {
        self.reads_ok += 1;
        self.latency_sum += latency;
        self.latency_min = Some(self.latency_min.map_or(latency, |m| m.min(latency)));
        self.latency_max = Some(self.latency_max.map_or(latency, |m| m.max(latency)));
    }

    pub fn record_read_error(&mut self, message: &str) {
        self.read_errors += 1;
        self.last_error = Some((message.to_string(), Utc::now()));
    }

    pub fn record_write(&mut self, ok: bool, message: &str) {
        if ok {
            self.writes_ok += 1;
        } else {
            self.write_errors += 1;
            self.last_error = Some((message.to_string(), Utc::now()));
        }
    }

    pub fn latency(&self) -> Option<(Duration, Duration, Duration)> {
        let (min, max) = (self.latency_min?, self.latency_max?);
        let avg = self.latency_sum / self.reads_ok.min(u32::MAX as u64) as u32;
        Some((min, avg, max))
    }

    pub fn last_error(&self) -> Option<(&str, DateTime<Utc>)> {
        self.last_error.as_ref().map(|(m, at)| (m.as_str(), *at))
    }
}

#[derive(Debug)]
struct RefreshTaskResult {
    register_type: RegisterType,
    main_data: Option<Result<Vec<RegisterCellValue>, String>>,
    pinned_data: Option<Result<Vec<RegisterCellValue>, String>>,
    read_duration: Duration,
}

#[cfg(not(target_arch = "wasm32"))]
struct ClipboardHandle(arboard::Clipboard);

#[cfg(not(target_arch = "wasm32"))]
impl std::fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipboardHandle")
    }
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    config_path: String,
    origin_config_path: String,
    pub running: bool,
    pub state: State,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: Option<ModbusDevice>,
    pub interpreter: Interpretor,
    pub connection: ConnectionStatus,
    pub frame: u64,
    pub last_frame: Duration,
    pub paused: bool,
    pub headless: bool,
    pub dirty: bool,
    saved_config: String,
    pub sweep: SweepState,
    pub stats: CommStats,
    reconnect: ReconnectState,
    pub visible_rows: Cell<u16>,
    pub h_max_offset: Cell<u16>,
    previous_position: Option<RegisterCell>,
    background_task: Option<BackgroundTask>,
    network_scan: Option<ScanProgress>,
    #[cfg(not(target_arch = "wasm32"))]
    network_scan_task: Option<TaskHandle<Vec<String>>>,
    previous_values: BTreeMap<RegisterCell, u16>,
    changed: BTreeMap<RegisterCell, DateTime<Utc>>,
    read_log: BTreeMap<RegisterCell, (u16, DateTime<Utc>)>,
    value_history: BTreeMap<RegisterCell, VecDeque<(u16, DateTime<Utc>)>>,
    labels: BTreeMap<RegisterCell, String>,
    custom_rules: BTreeMap<RegisterCell, CustomRule>,
    pending_write: Option<PendingWrite>,
    pending_import: Option<ImportPayload>,
    logged_connection: ConnectionStatus,
    api_device: ApiDevice,
    api_bound_port: BoundPort,
    api_read_only: ReadOnlyFlag,
    api_allow_slave_id: AllowSlaveFlag,
    api_status: StatusFlag,
    api_bind: BindStateFlag,
    writes_log: SharedWritesLog,
    #[cfg(not(target_arch = "wasm32"))]
    api_server: Option<tokio::task::JoinHandle<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    api_server_port: Option<u16>,
    #[cfg(not(target_arch = "wasm32"))]
    api_pending_port: Option<u16>,
    #[cfg(not(target_arch = "wasm32"))]
    clipboard: Option<ClipboardHandle>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct PinnedRegisters {
    pub holdings: Vec<u16>,
    pub inputs: Vec<u16>,
    pub coils: Vec<u16>,
    pub discretes: Vec<u16>,
}

#[derive(Debug, Default, Deserialize)]
struct ImportPayload {
    pinned_registers: Option<PinnedRegisters>,
    labels: Option<Labels>,
    custom_rules: Option<CustomRules>,
}

fn section_count<T>(section: &[T], rest: [&[T]; 3]) -> usize {
    section.len() + rest.iter().map(|s| s.len()).sum::<usize>()
}

impl ImportPayload {
    fn pins(&self) -> usize {
        self.pinned_registers.as_ref().map_or(0, |p| {
            section_count(&p.holdings, [&p.inputs, &p.coils, &p.discretes])
        })
    }

    fn labels(&self) -> usize {
        self.labels.as_ref().map_or(0, |l| {
            section_count(&l.holdings, [&l.inputs, &l.coils, &l.discretes])
        })
    }

    fn rules(&self) -> usize {
        self.custom_rules.as_ref().map_or(0, |r| {
            section_count(&r.holdings, [&r.inputs, &r.coils, &r.discretes])
        })
    }

    fn total(&self) -> usize {
        self.pins() + self.labels() + self.rules()
    }
}

macro_rules! impl_section_mut {
    ($ty:ty, $elem:ty) => {
        impl $ty {
            fn section_mut(&mut self, kind: RegisterType) -> &mut Vec<$elem> {
                match kind {
                    RegisterType::Holding => &mut self.holdings,
                    RegisterType::Input => &mut self.inputs,
                    RegisterType::Coil => &mut self.coils,
                    RegisterType::Discrete => &mut self.discretes,
                }
            }
        }
    };
}

impl_section_mut!(PinnedRegisters, u16);
impl_section_mut!(Labels, Label);
impl_section_mut!(CustomRules, CustomRule);

impl From<PinnedRegisters> for Vec<RegisterCell> {
    fn from(mut value: PinnedRegisters) -> Self {
        let mut collection = Vec::new();
        for kind in RegisterType::ALL {
            for address in std::mem::take(value.section_mut(kind)) {
                collection.push((kind, address));
            }
        }
        collection
    }
}

impl From<&[RegisterCell]> for PinnedRegisters {
    fn from(cells: &[RegisterCell]) -> Self {
        let mut pinned = PinnedRegisters::default();
        for &(kind, address) in cells {
            pinned.section_mut(kind).push(address);
        }
        pinned
    }
}

impl From<Labels> for BTreeMap<RegisterCell, String> {
    fn from(mut value: Labels) -> Self {
        let mut map = BTreeMap::new();
        for kind in RegisterType::ALL {
            for label in std::mem::take(value.section_mut(kind)) {
                map.insert((kind, label.address), label.text);
            }
        }
        map
    }
}

impl From<&BTreeMap<RegisterCell, String>> for Labels {
    fn from(map: &BTreeMap<RegisterCell, String>) -> Self {
        let mut labels = Labels::default();
        for (&(kind, address), text) in map {
            labels.section_mut(kind).push(Label {
                address,
                text: text.clone(),
            });
        }
        labels
    }
}

impl From<CustomRules> for BTreeMap<RegisterCell, CustomRule> {
    fn from(mut value: CustomRules) -> Self {
        let mut map = BTreeMap::new();
        for kind in RegisterType::ALL {
            for rule in std::mem::take(value.section_mut(kind)) {
                map.insert((kind, rule.address), rule);
            }
        }
        map
    }
}

impl From<&BTreeMap<RegisterCell, CustomRule>> for CustomRules {
    fn from(map: &BTreeMap<RegisterCell, CustomRule>) -> Self {
        let mut rules = CustomRules::default();
        for (&(kind, address), rule) in map {
            let mut rule = rule.clone();
            rule.address = address;
            rules.section_mut(kind).push(rule);
        }
        rules
    }
}

fn ensure_parent_dir(path: &str) -> Result<(), String> {
    match std::path::Path::new(path).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|e| e.to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_config_path() -> String {
    if std::path::Path::new(CONFIG_PATH).exists() {
        return CONFIG_PATH.to_string();
    }
    directories::ProjectDirs::from("io", "inowattio", "mtui")
        .map(|dirs| {
            dirs.config_dir()
                .join(CONFIG_PATH)
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_else(|| CONFIG_PATH.to_string())
}

#[cfg(target_arch = "wasm32")]
fn default_config_path() -> String {
    CONFIG_PATH.to_string()
}

fn save_config(path: &str, config: &Config) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    ensure_parent_dir(path)?;
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

    let width = c.repr.register_count() as u8 * 16;
    if let Some(entry) = c.bits.iter().find(|e| e.bit >= width) {
        return Err(format!(
            "bit: {} exceeds {} ({} bits)",
            entry.bit,
            c.repr.label(),
            width
        ));
    }

    if c.next.len() > c.repr.register_count() - 1 {
        return Err(format!(
            "next: {} uses {} register(s)",
            c.repr.label(),
            c.repr.register_count()
        ));
    }

    let rule = CustomRule {
        address: c.address,
        repr: c.repr,
        ops: c.ops.clone(),
        enum_map: c.enum_map.clone(),
        bits: c.bits.clone(),
        next: c.next.clone(),
        decimals,
        prefix: c.prefix.clone(),
        suffix: c.suffix.clone(),
        word_order: c.word_order,
    };
    Ok(((c.register_type, c.address), rule))
}

fn parse_hex_bytes(input: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    if !cleaned.len().is_multiple_of(2) {
        return Err("hex data needs an even number of digits".to_string());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| format!("invalid hex byte '{}'", &cleaned[i..i + 2]))
        })
        .collect()
}

fn create_default_config(path: &str) -> Config {
    let config = Config::demo();
    let serialized = serde_json::to_string_pretty(&config).expect("serialize default config");

    let _ = ensure_parent_dir(path);
    match fs::write(path, serialized) {
        Ok(()) => log::info!("No config found; created a default one at {path}"),
        Err(e) => log::warn!("No config found and could not write {path}: {e}; using defaults"),
    }
    config
}

fn fetch_config_or_exit(path: &str, dump_example_if_missing: bool) -> Config {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            if dump_example_if_missing {
                return create_default_config(path);
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

mod api;
mod columns;
mod config_io;
mod custom;
mod discovery;
mod help;
mod lifecycle;
mod logs;
mod panel;
mod search;
mod settings;
mod slave;
mod sweep;
mod write;
