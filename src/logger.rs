use chrono::{DateTime, Local};
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::sync::Mutex;

const CAP: usize = 1000;
const TARGET_PREFIX: &str = "mtui";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl From<Level> for LogLevel {
    fn from(level: Level) -> Self {
        match level {
            Level::Error => LogLevel::Error,
            Level::Warn => LogLevel::Warn,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub time: DateTime<Local>,
    pub level: LogLevel,
    pub message: String,
}

static ENTRIES: Mutex<VecDeque<LogEntry>> = Mutex::new(VecDeque::new());
static LOGGER: TuiLogger = TuiLogger;

struct TuiLogger;

impl Log for TuiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.target().starts_with(TARGET_PREFIX)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut entries) = ENTRIES.lock() {
            entries.push_back(LogEntry {
                time: Local::now(),
                level: record.level().into(),
                message: record.args().to_string(),
            });
            while entries.len() > CAP {
                entries.pop_front();
            }
        }
    }

    fn flush(&self) {}
}

pub fn init() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Info);
}

pub fn count() -> usize {
    ENTRIES.lock().map(|e| e.len()).unwrap_or(0)
}

pub fn snapshot() -> Vec<LogEntry> {
    ENTRIES
        .lock()
        .map(|e| e.iter().cloned().collect())
        .unwrap_or_default()
}
