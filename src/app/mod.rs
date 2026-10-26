use crate::compat;
use crate::compat::{Instant, TaskHandle};
use crate::config::{Config, Registers};
use crate::constants::CONFIG_PATH;
#[cfg(not(target_arch = "wasm32"))]
use crate::constants::SCAN_PREFIX_RANGE;
use crate::custom::{ASCII_REGISTERS_MAX, CustomRule};
use crate::interpretator::Interpretor;
#[cfg(not(target_arch = "wasm32"))]
use crate::modbus::Interface;
use crate::modbus::{DeviceConfig, DeviceIdAccess, ModbusDevice};
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{ConnectionStatus, CustomParams, ScreenLayout, State, UnitParams};
#[cfg(not(target_arch = "wasm32"))]
use crate::state::{FoundHost, ScanMethod};
use crate::writes_log::SharedWritesLog;
use chrono::{DateTime, Utc};
use std::collections::{BTreeMap, VecDeque};
use std::io::Write as _;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{fmt, fs, io};
use tokio_modbus::ExceptionCode;

pub type ApiDevice = Arc<Mutex<Option<ModbusDevice>>>;
pub type BoundPort = Arc<AtomicU16>;
pub type ReadOnlyFlag = Arc<AtomicBool>;
pub type AllowUnitFlag = Arc<AtomicBool>;
pub type AllowRawFlag = Arc<AtomicBool>;
pub type StatusFlag = Arc<AtomicU8>;
pub type BindStateFlag = Arc<AtomicU8>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiBindState {
    Pending,
    Bound,
    Failed,
}

impl ApiBindState {
    pub const fn code(self) -> u8 {
        match self {
            Self::Pending => 0,
            Self::Bound => 1,
            Self::Failed => 2,
        }
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Bound,
            2 => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriteType {
    #[default]
    Word,
    DWord,
    Coil,
}

impl WriteType {
    pub const fn bits(self) -> u16 {
        match self {
            Self::Coil => 1,
            Self::Word => 16,
            Self::DWord => 32,
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
    UnitScan(TaskHandle<UnitScanTaskResult>),
    LoadConfig(TaskHandle<LoadConfigTaskResult>),
}

#[derive(Debug)]
enum UnitProbeOutcome {
    Response(Vec<u16>),
    Exception(String),
    Silent,
}

#[derive(Debug)]
struct UnitScanTaskResult {
    unit_id: u8,
    outcome: UnitProbeOutcome,
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
    result: Result<(Vec<u8>, Duration), String>,
}

#[derive(Debug)]
struct LoadConfigTaskResult {
    path: PathBuf,
    config: Box<Config>,
    result: Result<ModbusDevice, String>,
}

#[derive(Debug)]
struct ScanProgress {
    done: Arc<AtomicUsize>,
    total: usize,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScanRange {
    network: Ipv4Addr,
    prefix: u8,
}

#[cfg(not(target_arch = "wasm32"))]
impl ScanRange {
    fn parse(ip: &str, prefix: u8) -> Option<Self> {
        if !SCAN_PREFIX_RANGE.contains(&prefix) {
            return None;
        }
        let parts: Vec<&str> = ip.split('.').collect();
        if parts.len() != 4 {
            return None;
        }
        let mut octets = [0u8; 4];
        for (i, (slot, part)) in octets.iter_mut().zip(parts).enumerate() {
            *slot = match part {
                "" if i == 3 => 0,
                part => part.parse().ok()?,
            };
        }
        let mask = u32::MAX << (32 - prefix);
        Some(Self {
            network: Ipv4Addr::from(u32::from(Ipv4Addr::from(octets)) & mask),
            prefix,
        })
    }

    const fn host_count(self) -> usize {
        (1usize << (32 - self.prefix)) - 2
    }

    fn hosts(self) -> impl Iterator<Item = Ipv4Addr> {
        let base = u32::from(self.network);
        (1..=self.host_count() as u32).map(move |offset| Ipv4Addr::from(base + offset))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl fmt::Display for ScanRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.network, self.prefix)
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
struct ModbusProbe {
    device: DeviceConfig,
    register_type: RegisterType,
    address: u16,
    amount: u16,
}

#[cfg(not(target_arch = "wasm32"))]
async fn scan_subnet(
    range: ScanRange,
    port: u16,
    method: ScanMethod,
    per_host: Duration,
    probe: ModbusProbe,
    done: Arc<AtomicUsize>,
) -> Result<Vec<FoundHost>, String> {
    use futures::stream::{self, StreamExt};

    let pinger = match method {
        ScanMethod::Ping => Some(
            surge_ping::Client::new(&surge_ping::Config::default())
                .map_err(|error| format!("ICMP unavailable: {error}"))?,
        ),
        ScanMethod::Port | ScanMethod::Modbus => None,
    };
    let ident = surge_ping::PingIdentifier(std::process::id() as u16);

    let mut found: Vec<(usize, FoundHost)> = stream::iter(range.hosts().enumerate())
        .map(|(index, host)| {
            let done = done.clone();
            let pinger = pinger.clone();
            let probe = &probe;
            async move {
                let reply = match (&pinger, method) {
                    (Some(client), _) => ping_host(client, host, ident, index as u16, per_host)
                        .await
                        .then_some(None),
                    (None, ScanMethod::Modbus) => {
                        probe_modbus(probe, host, per_host).await.map(Some)
                    }
                    (None, _) => probe_port(host, port, per_host).await.then_some(None),
                };
                done.fetch_add(1, Ordering::Relaxed);
                reply.map(|reply| {
                    (
                        index,
                        FoundHost {
                            ip: host.to_string(),
                            reply,
                        },
                    )
                })
            }
        })
        .buffer_unordered(256)
        .filter_map(|hit| async move { hit })
        .collect()
        .await;

    found.sort_by_key(|(index, _)| *index);
    Ok(found.into_iter().map(|(_, host)| host).collect())
}

#[cfg(not(target_arch = "wasm32"))]
async fn probe_modbus(
    probe: &ModbusProbe,
    ip: Ipv4Addr,
    per_host: Duration,
) -> Option<Result<Vec<u16>, String>> {
    let mut config = probe.device.clone();
    config.connect_timeout_ms = per_host.as_millis() as u64;
    match &mut config.interface {
        Interface::Tcp(n) | Interface::RtuOverTcp(n) => n.ip = ip.to_string(),
        Interface::Serial(_) | Interface::Mock => return None,
    }
    let result =
        ModbusDevice::read_once(&config, probe.register_type, probe.address, probe.amount).await;
    match result {
        Ok(values) => {
            log::info!("Network scan | {ip} responded | {values:?}");
            Some(Ok(values))
        }
        Err(e) if e.downcast_ref::<ExceptionCode>().is_some() => Some(Err(e.to_string())),
        Err(_) => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn ping_host(
    client: &surge_ping::Client,
    ip: Ipv4Addr,
    ident: surge_ping::PingIdentifier,
    seq: u16,
    per_host: Duration,
) -> bool {
    let mut pinger = client.pinger(IpAddr::V4(ip), ident).await;
    pinger.timeout(per_host);
    pinger
        .ping(surge_ping::PingSequence(seq), &[0u8; 8])
        .await
        .is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
async fn probe_port(ip: Ipv4Addr, port: u16, per_host: Duration) -> bool {
    matches!(
        compat::timeout(per_host, tokio::net::TcpStream::connect((ip, port))).await,
        Ok(Ok(_))
    )
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
                Some(_) => {}
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
    link_lost: bool,
}

#[derive(Debug)]
struct WriteOutcome {
    ok: bool,
    message: String,
    latency: Duration,
    read_back: Vec<(RegisterCell, u16)>,
}

#[derive(Debug)]
struct QueuedWrite {
    position: u16,
    number: i64,
    write_type: WriteType,
    func: crate::state::WriteFunc,
    raw: bool,
}

#[derive(Debug)]
struct PendingWrite {
    unit: u8,
    address: u16,
    write_type: WriteType,
    func: crate::state::WriteFunc,
    previous: Option<u64>,
    new_value: u64,
}

impl PendingWrite {
    const fn target(&self) -> StatTarget {
        match self.write_type {
            WriteType::Coil => StatTarget::Write(RegisterType::Coil, self.address, 1),
            WriteType::Word => StatTarget::Write(RegisterType::Holding, self.address, 1),
            WriteType::DWord => StatTarget::Write(RegisterType::Holding, self.address, 2),
        }
    }
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
struct RequestTrace {
    latencies: Vec<Duration>,
    attempt: Option<StatTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadFailure {
    Exception,
    Timeout,
    Transport,
}

#[derive(Debug)]
struct ReadError {
    message: String,
    kind: ReadFailure,
}

impl From<anyhow::Error> for ReadError {
    fn from(error: anyhow::Error) -> Self {
        let kind = if error.downcast_ref::<ExceptionCode>().is_some() {
            ReadFailure::Exception
        } else if error.downcast_ref::<compat::Elapsed>().is_some() {
            ReadFailure::Timeout
        } else {
            ReadFailure::Transport
        };
        Self {
            message: error.to_string(),
            kind,
        }
    }
}

#[derive(Debug)]
struct RefreshTaskResult {
    register_type: RegisterType,
    main_window: bool,
    values: Vec<RegisterCellValue>,
    trace: RequestTrace,
    error: Option<ReadError>,
    read_duration: Duration,
}

#[cfg(not(target_arch = "wasm32"))]
struct ClipboardHandle(arboard::Clipboard);

#[cfg(not(target_arch = "wasm32"))]
impl fmt::Debug for ClipboardHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClipboardHandle")
    }
}

#[derive(Clone, Debug)]
pub struct ReadEntry {
    pub value: u16,
    pub at: DateTime<Utc>,
    pub time_text: Arc<str>,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    config_path: PathBuf,
    origin_config_path: PathBuf,
    pub running: bool,
    pub state: State,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: Option<ModbusDevice>,
    pub interpreter: Interpretor,
    pub connection: ConnectionStatus,
    pub frame: u64,
    pub last_frame: Duration,
    pub ram_bytes: Option<usize>,
    pub paused: bool,
    pub headless: bool,
    pub dirty: bool,
    saved_config: String,
    pub sweep: SweepState,
    pub stats: CommStats,
    reconnect: ReconnectState,
    pub layout: ScreenLayout,
    pub viewport_width: u16,
    background_task: Option<BackgroundTask>,
    network_scan: Option<ScanProgress>,
    unit_scan: Option<(String, UnitParams)>,
    #[cfg(not(target_arch = "wasm32"))]
    network_scan_task: Option<TaskHandle<Result<Vec<FoundHost>, String>>>,
    #[cfg(not(target_arch = "wasm32"))]
    ports_task: Option<TaskHandle<Vec<String>>>,
    changed: BTreeMap<RegisterCell, DateTime<Utc>>,
    read_log: BTreeMap<RegisterCell, ReadEntry>,
    value_history: BTreeMap<RegisterCell, VecDeque<(u16, DateTime<Utc>)>>,
    labels: BTreeMap<RegisterCell, String>,
    custom_rules: BTreeMap<RegisterCell, CustomRule>,
    pending_write: Option<PendingWrite>,
    queued_write: Option<QueuedWrite>,
    pending_import: Option<Registers>,
    logged_connection: ConnectionStatus,
    api_device: ApiDevice,
    api_bound_port: BoundPort,
    api_read_only: ReadOnlyFlag,
    api_allow_unit_id: AllowUnitFlag,
    api_allow_raw: AllowRawFlag,
    api_status: StatusFlag,
    api_bind: BindStateFlag,
    writes_log: SharedWritesLog,
    #[cfg(not(target_arch = "wasm32"))]
    api_server: Option<tokio::task::JoinHandle<()>>,
    #[cfg(not(target_arch = "wasm32"))]
    api_server_port: Option<u16>,
    #[cfg(not(target_arch = "wasm32"))]
    clipboard: Option<ClipboardHandle>,
}

pub(super) fn file_name(parts: &[&str], extension: &str) -> String {
    let stem: Vec<String> = parts
        .iter()
        .map(|part| {
            part.trim()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .trim_matches('_')
                .to_string()
        })
        .filter(|part| !part.is_empty())
        .collect();
    format!("{}.{extension}", stem.join("_"))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::create_dir_all(parent).map_err(|e| e.to_string())
        }
        _ => Ok(()),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_config_path() -> PathBuf {
    if Path::new(CONFIG_PATH).exists() {
        return PathBuf::from(CONFIG_PATH);
    }
    directories::ProjectDirs::from("io", "inowattio", "mtui")
        .map(|dirs| dirs.config_dir().join(CONFIG_PATH))
        .unwrap_or_else(|| PathBuf::from(CONFIG_PATH))
}

#[cfg(target_arch = "wasm32")]
fn default_config_path() -> PathBuf {
    PathBuf::from(CONFIG_PATH)
}

fn stage_file(path: &Path, content: &str) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()
}

fn write_atomically(path: &Path, content: &str) -> Result<(), String> {
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);

    stage_file(&temporary, content)
        .and_then(|()| fs::rename(&temporary, path))
        .map_err(|error| {
            let _ = fs::remove_file(&temporary);
            error.to_string()
        })
}

fn save_config(path: &Path, config: &Config) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    ensure_parent_dir(path)?;
    write_atomically(path, &content)
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

    let ascii = c.is_ascii();
    let width = c.register_count() * 16;
    if !ascii && let Some(entry) = c.bits.iter().find(|e| usize::from(e.bit) >= width) {
        return Err(format!(
            "bit: {} exceeds {} ({} bits)",
            entry.bit,
            c.repr.label(),
            width
        ));
    }

    if c.next.len() > c.register_count() - 1 {
        return Err(format!(
            "next: {} uses {} register(s)",
            c.repr.label(),
            c.register_count()
        ));
    }

    if ascii && !(1..=ASCII_REGISTERS_MAX).contains(&c.registers) {
        return Err(format!("registers: 1 to {ASCII_REGISTERS_MAX}"));
    }

    let rule = CustomRule {
        address: c.address,
        repr: c.repr,
        registers: ascii.then_some(c.registers),
        ops: if ascii { Vec::new() } else { c.ops.clone() },
        enum_map: if ascii {
            Vec::new()
        } else {
            c.enum_map.clone()
        },
        bits: if ascii { Vec::new() } else { c.bits.clone() },
        next: c.next.clone(),
        decimals: decimals.filter(|_| !ascii),
        prefix: c.prefix.clone(),
        suffix: c.suffix.clone(),
        word_order: c.word_order,
    };
    Ok(((c.register_type, c.address), rule))
}

fn create_default_config(path: &Path) -> Config {
    let config = Config::demo();
    let serialized = serde_json::to_string_pretty(&config).expect("serialize default config");

    let _ = ensure_parent_dir(path);
    match write_atomically(path, &serialized) {
        Ok(()) => log::info!(
            "No config found; created a default one at {}",
            path.display()
        ),
        Err(e) => log::warn!(
            "No config found and could not write {}: {e}; using defaults",
            path.display()
        ),
    }
    config
}

#[derive(Debug)]
pub enum ConfigError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl ConfigError {
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Read { .. } => 2,
            Self::Parse { .. } => 3,
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, .. } => write!(f, "Could not read config {}", path.display()),
            Self::Parse { path, .. } => {
                write!(f, "Could not parse config {}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

fn load_config(path: &Path, create_if_missing: bool) -> Result<Config, ConfigError> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(source) if create_if_missing && source.kind() == io::ErrorKind::NotFound => {
            return Ok(create_default_config(path));
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let parse = |source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    };
    let value: serde_json::Value = serde_json::from_str(&content).map_err(parse)?;
    let legacy = value
        .as_object()
        .is_some_and(|fields| !fields.is_empty() && !fields.contains_key("version"));
    let mut config: Config = serde_json::from_value(value).map_err(parse)?;
    config.legacy = legacy;
    Ok(config)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) async fn settle_until(app: &mut App, done: impl Fn(&App) -> bool, what: &str) {
    for _ in 0..500 {
        app.complete_background_task();
        if done(app) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("{what} never finished");
}

#[cfg(test)]
mod tests {
    #[test]
    fn ascii_rules_drop_numeric_settings_and_need_a_register_count() {
        use crate::custom::{CustomOp, CustomRepr, OpKind};
        use crate::state::CustomParams;

        let mut c = CustomParams {
            address: 200,
            repr: CustomRepr::Ascii,
            registers: 16,
            ops: vec![CustomOp {
                op: OpKind::Div,
                v: 10.0,
            }],
            decimals: "2".to_string(),
            suffix: " fw".to_string(),
            ..Default::default()
        };
        let (_, rule) = super::build_custom_rule(&c).unwrap();
        assert_eq!(rule.registers, Some(16));
        assert_eq!(rule.register_count(), 16);
        assert!(rule.ops.is_empty());
        assert_eq!(rule.decimals, None);
        assert_eq!(rule.suffix, " fw");

        c.registers = 0;
        assert!(super::build_custom_rule(&c).is_err());

        c.repr = CustomRepr::U16;
        let (_, rule) = super::build_custom_rule(&c).unwrap();
        assert_eq!(rule.registers, None);
        assert_eq!(rule.ops.len(), 1);
    }

    #[test]
    fn file_names_skip_empty_parts_and_sanitize_the_rest() {
        assert_eq!(
            super::file_name(&["dump", "", "20260921"], "txt"),
            "dump_20260921.txt"
        );
        assert_eq!(
            super::file_name(&["writes", "Plant A / line 2", "tcp", "1"], "csv"),
            "writes_Plant_A___line_2_tcp_1.csv"
        );
        assert_eq!(super::file_name(&["logs", "  ", "x"], "txt"), "logs_x.txt");
    }

    use super::{Config, ConfigError, load_config, save_config};
    use crate::scratch::ScratchDir;
    use std::fs;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn scan_ranges_mask_the_address_and_skip_network_and_broadcast() {
        use super::ScanRange;
        use std::net::Ipv4Addr;

        let range = ScanRange::parse("192.168.1.37", 22).unwrap();
        assert_eq!(range.to_string(), "192.168.0.0/22");
        assert_eq!(range.host_count(), 1022);
        let hosts: Vec<Ipv4Addr> = range.hosts().collect();
        assert_eq!(hosts.len(), 1022);
        assert_eq!(hosts.first().copied(), Some(Ipv4Addr::new(192, 168, 0, 1)));
        assert_eq!(hosts.last().copied(), Some(Ipv4Addr::new(192, 168, 3, 254)));

        let prefill = ScanRange::parse("10.0.0.", 24).unwrap();
        assert_eq!(prefill.to_string(), "10.0.0.0/24");
        assert_eq!(prefill.host_count(), 254);

        assert!(ScanRange::parse("10.0.0", 24).is_none());
        assert!(ScanRange::parse("10..0.1", 24).is_none());
        assert!(ScanRange::parse("300.0.0.1", 24).is_none());
        assert!(ScanRange::parse("10.0.0.1", 8).is_none());
        assert!(ScanRange::parse("10.0.0.1", 31).is_none());
    }

    #[test]
    fn read_failures_are_classified_by_cause() {
        use super::{ReadError, ReadFailure};
        use tokio_modbus::ExceptionCode;

        let kind = |error: anyhow::Error| ReadError::from(error).kind;
        assert_eq!(
            kind(anyhow::Error::from(ExceptionCode::IllegalDataAddress)),
            ReadFailure::Exception
        );
        assert_eq!(
            kind(crate::modbus::exception_error(ExceptionCode::Custom(0x20))),
            ReadFailure::Exception
        );
        assert_eq!(
            kind(anyhow::Error::from(crate::compat::Elapsed)),
            ReadFailure::Timeout
        );
        assert_eq!(
            kind(anyhow::Error::from(std::io::Error::from(
                std::io::ErrorKind::BrokenPipe
            ))),
            ReadFailure::Transport
        );
        assert_eq!(
            kind(anyhow::anyhow!("Expected 2 value(s), got 1")),
            ReadFailure::Transport
        );
    }

    #[test]
    fn missing_file_is_a_read_error_unless_creation_is_allowed() {
        let scratch = ScratchDir::new("missing");
        let path = scratch.path("config.json");

        let error = load_config(&path, false).expect_err("nothing to read");
        assert!(matches!(error, ConfigError::Read { .. }));
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().starts_with("Could not read config"));
        assert!(std::error::Error::source(&error).is_some());
        assert!(!fs::exists(&path).unwrap(), "must not create anything");

        let config = load_config(&path, true).expect("demo config written");
        assert_eq!(config.name, "demo");
        let reloaded = load_config(&path, false).expect("the written file parses");
        assert_eq!(reloaded.name, config.name);
    }

    #[test]
    fn a_file_without_a_version_is_flagged_as_legacy() {
        let scratch = ScratchDir::new("legacy");
        let path = scratch.path("config.json");
        fs::write(&path, r#"{"name": "old"}"#).unwrap();
        assert!(load_config(&path, false).unwrap().legacy);

        fs::write(&path, r#"{"version": 1, "name": "new"}"#).unwrap();
        assert!(!load_config(&path, false).unwrap().legacy);

        fs::write(&path, "{}").unwrap();
        assert!(!load_config(&path, false).unwrap().legacy);
    }

    #[test]
    fn garbage_is_a_parse_error() {
        let scratch = ScratchDir::new("garbage");
        let path = scratch.path("config.json");
        fs::write(&path, "{ not json").unwrap();

        let error = load_config(&path, true).expect_err("unparsable");
        assert!(matches!(error, ConfigError::Parse { .. }));
        assert_eq!(error.exit_code(), 3);
        assert!(error.to_string().starts_with("Could not parse config"));
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{ not json",
            "a broken file must never be overwritten"
        );
    }

    #[test]
    fn unreadable_existing_path_is_not_replaced() {
        let scratch = ScratchDir::new("dir");
        let path = scratch.path("config.json");
        fs::create_dir(&path).unwrap();

        let error = load_config(&path, true).expect_err("a directory cannot be read");
        assert!(matches!(error, ConfigError::Read { .. }));
    }

    #[test]
    fn saving_replaces_the_file_and_leaves_no_temporary_behind() {
        let scratch = ScratchDir::new("atomic");
        let path = scratch.path("config.json");
        let temporary = scratch.path("config.json.tmp");

        let first = Config {
            name: "first".to_string(),
            ..Config::default()
        };
        save_config(&path, &first).expect("the config is written");
        assert!(!fs::exists(&temporary).unwrap(), "no leftover temporary");
        assert_eq!(load_config(&path, false).unwrap().name, "first");

        let second = Config {
            name: "second".to_string(),
            ..Config::default()
        };
        save_config(&path, &second).expect("the config is replaced");
        assert!(!fs::exists(&temporary).unwrap(), "no leftover temporary");
        assert_eq!(load_config(&path, false).unwrap().name, "second");
    }

    #[test]
    fn a_stale_temporary_does_not_block_the_next_save() {
        let scratch = ScratchDir::new("stale");
        let path = scratch.path("config.json");
        let temporary = scratch.path("config.json.tmp");
        fs::write(&temporary, "half a config").unwrap();

        save_config(&path, &Config::default()).expect("the stale temporary is reused");

        assert!(!fs::exists(&temporary).unwrap());
        assert_eq!(load_config(&path, false).unwrap().name, "demo");
    }

    #[test]
    fn a_failed_save_leaves_the_previous_config_untouched() {
        let scratch = ScratchDir::new("kept");
        let path = scratch.path("config.json");
        save_config(&path, &Config::default()).expect("the config is written");
        let saved = fs::read_to_string(&path).unwrap();

        fs::create_dir(scratch.path("config.json.tmp")).unwrap();
        let error = save_config(&path, &Config::default())
            .expect_err("staging cannot write over a directory");

        assert!(!error.is_empty(), "the failure is reported");
        assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    }
}

mod api;
mod columns;
mod config_io;
mod custom;
mod device_id;
mod diff;
mod discovery;
#[cfg(not(target_arch = "wasm32"))]
mod dropped;
mod dump;
mod help;
mod label;
mod lifecycle;
mod logs;
mod panel;
mod pin;
mod raw;
mod search;
mod settings;
mod stats;
mod sweep;
mod unit;
mod unit_scan;
mod write;

pub use stats::{CommStats, StatTarget};
