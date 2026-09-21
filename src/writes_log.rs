use chrono::Local;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const HEADER: &str = "timestamp,unit,address,type,previous,value";

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

impl WriteKind {
    fn csv_value(&self) -> String {
        match self {
            WriteKind::Word(w) => w.to_string(),
            WriteKind::DWord(d) => d.to_string(),
            WriteKind::Coil(c) => u8::from(*c).to_string(),
            WriteKind::Multiple(v) => v.iter().map(u16::to_string).collect::<Vec<_>>().join(" "),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteEntry {
    pub timestamp: String,
    pub unit: u8,
    pub address: u16,
    pub kind: String,
    pub previous: Option<u64>,
    pub value: String,
}

impl WriteEntry {
    pub fn display_value(&self) -> String {
        match (self.kind.as_str(), self.value.as_str()) {
            ("coil", "1") => "on".to_string(),
            ("coil", "0") => "off".to_string(),
            _ => self.value.clone(),
        }
    }

    pub fn display_previous(&self) -> String {
        match (self.kind.as_str(), self.previous) {
            (_, None) => "?".to_string(),
            ("coil", Some(1)) => "on".to_string(),
            ("coil", Some(0)) => "off".to_string(),
            (_, Some(v)) => v.to_string(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WritesLogState {
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

pub type SharedWritesLog = Arc<Mutex<WritesLogState>>;

pub fn append(
    shared: &SharedWritesLog,
    unit: u8,
    address: u16,
    kind: WriteKind,
    previous: Option<u64>,
) {
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

    let timestamp = Local::now().format("%Y-%m-%dT%H:%M:%S%.3f").to_string();
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    if file.metadata().is_ok_and(|m| m.len() == 0) {
        let _ = writeln!(file, "{HEADER}");
    }
    let _ = writeln!(
        file,
        "{}",
        record(&timestamp, unit, address, &kind, previous)
    );
}

pub fn read_entries(path: &Path) -> std::io::Result<Vec<WriteEntry>> {
    let content = std::fs::read_to_string(path)?;
    Ok(content.lines().filter_map(parse_record).collect())
}

fn record(
    timestamp: &str,
    unit: u8,
    address: u16,
    kind: &WriteKind,
    previous: Option<u64>,
) -> String {
    let previous = previous.map(|v| v.to_string()).unwrap_or_default();
    format!(
        "{timestamp},{unit},{address},{kind},{previous},{}",
        kind.csv_value()
    )
}

fn parse_record(line: &str) -> Option<WriteEntry> {
    let mut fields = line.trim_end().splitn(6, ',');
    let timestamp = fields.next()?;
    let unit = fields.next()?.parse().ok()?;
    let address = fields.next()?.parse().ok()?;
    let kind = fields.next()?;
    let previous = match fields.next()? {
        "" => None,
        p => Some(p.parse().ok()?),
    };
    let value = fields.next()?;
    Some(WriteEntry {
        timestamp: timestamp.to_string(),
        unit,
        address,
        kind: kind.to_string(),
        previous,
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::{HEADER, WriteEntry, WriteKind, parse_record, record};

    #[test]
    fn records_are_comma_separated_with_an_empty_unknown_previous() {
        assert_eq!(
            record("t", 7, 40, &WriteKind::Word(3), Some(2)),
            "t,7,40,word,2,3"
        );
        assert_eq!(
            record("t", 1, 5, &WriteKind::Coil(true), None),
            "t,1,5,coil,,1"
        );
        assert_eq!(
            record("t", 250, 0, &WriteKind::Multiple(vec![1, 2]), None),
            "t,250,0,multiple,,1 2"
        );
    }

    #[test]
    fn records_parse_back_and_the_header_does_not() {
        let entry = parse_record(&record("t", 7, 40, &WriteKind::DWord(70000), None)).unwrap();
        assert_eq!(
            entry,
            WriteEntry {
                timestamp: "t".to_string(),
                unit: 7,
                address: 40,
                kind: "dword".to_string(),
                previous: None,
                value: "70000".to_string(),
            }
        );
        assert_eq!(parse_record(HEADER), None);
        assert_eq!(parse_record(""), None);
        assert_eq!(parse_record("t,7,40,word,x,3"), None);
    }

    #[test]
    fn coils_display_as_on_and_off() {
        let entry = parse_record("t,1,5,coil,0,1").unwrap();
        assert_eq!(entry.display_previous(), "off");
        assert_eq!(entry.display_value(), "on");
        let entry = parse_record("t,1,5,word,,9").unwrap();
        assert_eq!(entry.display_previous(), "?");
        assert_eq!(entry.display_value(), "9");
    }
}
