use chrono::{DateTime, Local};
use log::{Level, LevelFilter, Log, Metadata, Record};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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
            Level::Error => Self::Error,
            Level::Warn => Self::Warn,
            _ => Self::Info,
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

static ECHO: AtomicBool = AtomicBool::new(false);

struct TuiLogger;

impl Log for TuiLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.target().starts_with(TARGET_PREFIX)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = Local::now();
        let message = record.args().to_string();

        if ECHO.load(Ordering::Relaxed) {
            eprintln!(
                "{} {:<5} {message}",
                now.format("%H:%M:%S%.3f"),
                record.level()
            );
        }

        if let Ok(mut entries) = ENTRIES.lock() {
            entries.push_back(LogEntry {
                time: now,
                level: record.level().into(),
                message,
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

pub fn enable_echo() {
    ECHO.store(true, Ordering::Relaxed);
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

impl LogLevel {
    pub fn tag(self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

pub fn export(entries: &[LogEntry]) -> String {
    entries
        .iter()
        .map(|e| {
            format!(
                "{} {:<5} {}\n",
                e.time.format("%Y-%m-%dT%H:%M:%S%.3f"),
                e.level.tag(),
                e.message
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{LogEntry, LogLevel, export};
    use chrono::{Local, TimeZone};

    #[test]
    fn exported_lines_carry_the_full_timestamp_and_a_padded_level() {
        let time = Local.with_ymd_and_hms(2026, 9, 18, 13, 5, 7).unwrap();
        let entries = [
            LogEntry {
                time,
                level: LogLevel::Info,
                message: "Connected".to_string(),
            },
            LogEntry {
                time,
                level: LogLevel::Error,
                message: "Read error | timeout".to_string(),
            },
        ];
        assert_eq!(
            export(&entries),
            "2026-09-18T13:05:07.000 INFO  Connected\n2026-09-18T13:05:07.000 ERROR Read error | timeout\n"
        );
        assert_eq!(export(&[]), "");
    }
}
