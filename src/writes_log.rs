use chrono::Local;
use std::fmt;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub enum WriteKind {
    Word(u16),
    DWord(u32),
    Coil(bool),
    Multiple(Vec<u16>),
}

impl fmt::Display for WriteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WriteKind::Word(_) => "word",
            WriteKind::DWord(_) => "dword",
            WriteKind::Coil(_) => "coil",
            WriteKind::Multiple(_) => "multiple",
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct WritesLogState {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

pub type SharedWritesLog = Arc<Mutex<WritesLogState>>;

pub fn append(shared: &SharedWritesLog, address: u16, kind: WriteKind, previous: Option<u64>) {
    let (enabled, path) = match shared.lock() {
        Ok(state) => (state.enabled, state.path.clone()),
        Err(_) => return,
    };
    if !enabled {
        return;
    }
    let Some(path) = path else {
        return;
    };

    let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f");
    let previous = previous.map_or_else(|| "?".to_string(), |v| v.to_string());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let val = match &kind {
            WriteKind::Word(w) => w.to_string(),
            WriteKind::DWord(d) => d.to_string(),
            WriteKind::Coil(c) => if *c { "on" } else { "off" }.to_string(),
            WriteKind::Multiple(v) => format!("{v:?}"),
        };

        let _ = writeln!(
            file,
            "{timestamp} | {address} | {kind} | {previous} | {val}"
        );
    }
}
