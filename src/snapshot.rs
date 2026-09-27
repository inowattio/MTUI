use crate::config::{AddressMode, Column};
use crate::constants::message;
use crate::interpretator::{RowSegment, is_age};
use crate::register::{RegisterCell, RegisterType};
use std::collections::BTreeMap;
use std::fmt;
use std::iter::Peekable;
use std::str::Chars;

pub const TYPE_COLUMN: &str = "type";

const BOM: char = '\u{feff}';

const RAW_COLUMNS: [Column; 4] = [Column::U16, Column::Hex, Column::I16, Column::Bits];

pub fn csv_line<'a>(fields: impl Iterator<Item = &'a str>) -> String {
    let mut line = fields.map(csv_field).collect::<Vec<_>>().join(",");
    line.push('\n');
    line
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CsvRecord {
    line: usize,
    fields: Vec<String>,
}

impl CsvRecord {
    fn is_blank(&self) -> bool {
        matches!(self.fields.as_slice(), [only] if only.trim().is_empty())
    }
}

struct CsvCursor<'a> {
    chars: Peekable<Chars<'a>>,
    line: usize,
}

impl CsvCursor<'_> {
    fn quoted(&mut self, field: &mut String) -> Option<()> {
        loop {
            match self.chars.next()? {
                '"' if self.chars.peek() == Some(&'"') => {
                    self.chars.next();
                    field.push('"');
                }
                '"' => return matches!(self.chars.peek(), None | Some(',' | '\n')).then_some(()),
                c => {
                    self.line += usize::from(c == '\n');
                    field.push(c);
                }
            }
        }
    }
}

fn read_csv(text: &str) -> Result<Vec<CsvRecord>, usize> {
    let text = text.strip_prefix(BOM).unwrap_or(text).replace("\r\n", "\n");
    let text = if text.contains('\n') {
        text
    } else {
        text.replace('\r', "\n")
    };
    let mut cursor = CsvCursor {
        chars: text.chars().peekable(),
        line: 1,
    };
    let mut records = Vec::new();
    let mut record = CsvRecord {
        line: 1,
        fields: Vec::new(),
    };
    let mut field = String::new();
    let mut fresh = true;
    while let Some(c) = cursor.chars.next() {
        match c {
            '"' if fresh => {
                cursor.quoted(&mut field).ok_or(record.line)?;
                fresh = false;
            }
            ',' | '\n' => {
                record.fields.push(std::mem::take(&mut field));
                fresh = true;
                if c == '\n' {
                    cursor.line += 1;
                    let next = CsvRecord {
                        line: cursor.line,
                        fields: Vec::new(),
                    };
                    records.push(std::mem::replace(&mut record, next));
                }
            }
            c => {
                field.push(c);
                fresh = false;
            }
        }
    }
    if !fresh || !record.fields.is_empty() {
        record.fields.push(field);
        records.push(record);
    }
    Ok(records)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotEntry {
    pub value: u16,
    pub time: Option<String>,
}

pub type Snapshot = BTreeMap<RegisterCell, SnapshotEntry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotError {
    NotADump,
    NoAddressColumn,
    NoRawColumn,
    NoRows,
    MalformedRow(usize),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADump => f.write_str(message::NOT_A_DUMP),
            Self::NoAddressColumn => f.write_str(message::DUMP_NO_ADDRESS),
            Self::NoRawColumn => f.write_str(message::DUMP_NO_RAW_COLUMN),
            Self::NoRows => f.write_str(message::DUMP_NO_ROWS),
            Self::MalformedRow(line) => write!(f, "Dump row on line {line} is malformed"),
        }
    }
}

pub fn is_dump(text: &str) -> bool {
    text.trim_start_matches(BOM)
        .trim_start()
        .split([',', '\n', '\r'])
        .next()
        .is_some_and(|first| first.trim() == TYPE_COLUMN)
}

struct DumpHeader {
    width: usize,
    address: usize,
    time: Option<usize>,
    raw: (Column, usize),
}

impl DumpHeader {
    fn parse(fields: &[String]) -> Result<Self, SnapshotError> {
        let find = |column: Column| fields.iter().position(|f| f.trim() == column.name());
        let address = find(Column::Address).ok_or(SnapshotError::NoAddressColumn)?;
        let raw = RAW_COLUMNS
            .iter()
            .find_map(|&column| find(column).map(|index| (column, index)))
            .ok_or(SnapshotError::NoRawColumn)?;
        Ok(Self {
            width: fields.len(),
            address,
            time: find(Column::Time),
            raw,
        })
    }

    fn row(&self, fields: &[String], mode: AddressMode) -> Option<(RegisterCell, SnapshotEntry)> {
        if fields.len() != self.width {
            return None;
        }
        let kind = register_type(&fields[0])?;
        let address = parse_address(&fields[self.address], mode)?;
        let (column, index) = self.raw;
        let value = raw_value(column, &fields[index])?;
        let time = self
            .time
            .map(|index| fields[index].trim())
            .filter(|time| !time.is_empty() && !is_age(time))
            .map(str::to_string);
        Some(((kind, address), SnapshotEntry { value, time }))
    }
}

fn register_type(text: &str) -> Option<RegisterType> {
    let text = text.trim();
    RegisterType::ALL
        .into_iter()
        .find(|kind| kind.name().eq_ignore_ascii_case(text))
}

fn parse_address(text: &str, mode: AddressMode) -> Option<u16> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u16::from_str_radix(hex, 16).ok();
    }
    match mode {
        AddressMode::Dec => text.parse().ok(),
        AddressMode::Hex => u16::from_str_radix(text, 16).ok(),
    }
}

fn raw_value(column: Column, text: &str) -> Option<u16> {
    let text = text.trim();
    match column {
        Column::U16 => text.parse().ok(),
        Column::Hex if text.len() == 4 && text.bytes().all(|b| b.is_ascii_hexdigit()) => {
            u16::from_str_radix(text, 16).ok()
        }
        Column::I16 => text.parse::<i16>().ok().map(i16::cast_unsigned),
        Column::Bits => bits_value(text),
        _ => None,
    }
}

fn bits_value(text: &str) -> Option<u16> {
    let groups: Vec<&str> = text.split(' ').collect();
    let valid = groups.len() == 4
        && groups
            .iter()
            .all(|group| group.len() == 4 && group.bytes().all(|b| matches!(b, b'0' | b'1')));
    if !valid {
        return None;
    }
    u16::from_str_radix(&groups.concat(), 2).ok()
}

pub fn parse_dump(text: &str, address_mode: AddressMode) -> Result<Snapshot, SnapshotError> {
    if !is_dump(text) {
        return Err(SnapshotError::NotADump);
    }
    let records = read_csv(text).map_err(SnapshotError::MalformedRow)?;
    let mut records = records.into_iter().filter(|record| !record.is_blank());
    let Some(first) = records.next() else {
        return Err(SnapshotError::NotADump);
    };
    let header = DumpHeader::parse(&first.fields)?;

    let mut snapshot = Snapshot::new();
    for record in records {
        let (cell, entry) = header
            .row(&record.fields, address_mode)
            .ok_or(SnapshotError::MalformedRow(record.line))?;
        snapshot.insert(cell, entry);
    }
    if snapshot.is_empty() {
        return Err(SnapshotError::NoRows);
    }
    Ok(snapshot)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffKind {
    Same,
    Changed,
    Unread,
}

impl DiffKind {
    const fn of(recorded: u16, current: Option<u16>) -> Self {
        match current {
            None => Self::Unread,
            Some(value) if value == recorded => Self::Same,
            Some(_) => Self::Changed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMark {
    Same,
    Before,
    After,
    Unread,
}

impl DiffMark {
    pub const fn symbol(self) -> char {
        match self {
            Self::Same => '=',
            Self::Before => '-',
            Self::After => '+',
            Self::Unread => '?',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffRow {
    pub cell: RegisterCell,
    pub mark: DiffMark,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub mark: DiffMark,
    pub cell: RegisterCell,
    pub text: String,
    pub emphasis: Vec<RowSegment>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffSummary {
    pub changed: usize,
    pub same: usize,
    pub unread: usize,
}

impl DiffSummary {
    pub const fn all_unread(self) -> bool {
        self.unread > 0 && self.changed == 0 && self.same == 0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diff {
    all: Vec<DiffRow>,
    changed: Vec<DiffRow>,
    pub summary: DiffSummary,
}

impl Diff {
    pub fn new(snapshot: &Snapshot, current: impl Fn(RegisterCell) -> Option<u16>) -> Self {
        let mut diff = Self::default();
        for (&cell, entry) in snapshot {
            let row = |mark| DiffRow { cell, mark };
            match DiffKind::of(entry.value, current(cell)) {
                DiffKind::Same => {
                    diff.summary.same += 1;
                    diff.all.push(row(DiffMark::Same));
                }
                DiffKind::Unread => {
                    diff.summary.unread += 1;
                    diff.all.push(row(DiffMark::Unread));
                }
                DiffKind::Changed => {
                    diff.summary.changed += 1;
                    let pair = [row(DiffMark::Before), row(DiffMark::After)];
                    diff.all.extend(pair);
                    diff.changed.extend(pair);
                }
            }
        }
        diff
    }

    pub fn rows(&self, changed_only: bool) -> &[DiffRow] {
        if changed_only {
            &self.changed
        } else {
            &self.all
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: RegisterType = RegisterType::Input;

    fn records(text: &str) -> Vec<Vec<String>> {
        read_csv(text)
            .expect("valid csv")
            .into_iter()
            .map(|record| record.fields)
            .collect()
    }

    fn owned(fields: &[&str]) -> Vec<String> {
        fields.iter().map(ToString::to_string).collect()
    }

    fn dec(text: &str) -> Result<Snapshot, SnapshotError> {
        parse_dump(text, AddressMode::Dec)
    }

    fn value(snapshot: &Snapshot, cell: RegisterCell) -> u16 {
        snapshot.get(&cell).expect("cell in snapshot").value
    }

    #[test]
    fn csv_lines_quote_only_what_needs_it() {
        assert_eq!(
            csv_line(["holding", "5", "a b"].into_iter()),
            "holding,5,a b\n"
        );
        assert_eq!(
            csv_line(["set: v, A", "say \"hi\"", ""].into_iter()),
            "\"set: v, A\",\"say \"\"hi\"\"\",\n"
        );
    }

    #[test]
    fn the_csv_reader_is_the_inverse_of_the_writer() {
        let cases: [&[&str]; 8] = [
            &["holding", "5", "a b"],
            &["cr\rinside", "x"],
            &["set: v, A", "say \"hi\"", ""],
            &["multi\nline", "x"],
            &["\"", ",", "\n", "\"\""],
            &[""],
            &["", ""],
            &["  padded  ", "trailing,"],
        ];
        for fields in cases {
            let line = csv_line(fields.iter().copied());
            assert_eq!(
                records(&line),
                vec![owned(fields)],
                "round trip of {line:?}"
            );
        }
    }

    #[test]
    fn records_remember_the_line_they_start_on() {
        let text = [
            csv_line(["a", "first\nsecond"].into_iter()),
            csv_line(["b", "c"].into_iter()),
        ]
        .concat();
        let lines: Vec<usize> = read_csv(&text)
            .unwrap()
            .into_iter()
            .map(|record| record.line)
            .collect();
        assert_eq!(lines, vec![1, 3]);
    }

    #[test]
    fn crlf_a_bom_and_a_missing_final_newline_are_tolerated() {
        let text = "\u{feff}type,\"a\r\nb\"\r\ninput,5";
        assert_eq!(
            records(text),
            vec![owned(&["type", "a\nb"]), owned(&["input", "5"])]
        );
    }

    #[test]
    fn terminals_pasting_carriage_returns_still_split_lines() {
        let text = "type,address,u16\rinput,1,5\rinput,2,\"x\ry\"\r";
        assert_eq!(
            read_csv(text).unwrap(),
            vec![
                CsvRecord {
                    line: 1,
                    fields: owned(&["type", "address", "u16"]),
                },
                CsvRecord {
                    line: 2,
                    fields: owned(&["input", "1", "5"]),
                },
                CsvRecord {
                    line: 3,
                    fields: owned(&["input", "2", "x\ny"]),
                },
            ]
        );
        let snapshot = dec("type,address,u16\rinput,1,5\rinput,2,6\r").unwrap();
        assert_eq!(value(&snapshot, (INPUT, 2)), 6);
    }

    #[test]
    fn a_lone_carriage_return_inside_a_field_survives_a_line_feed_dump() {
        for newline in ["\n", "\r\n"] {
            let text = format!(
                "type,address,u16,label{newline}input,1,5,a\rb{newline}input,2,6,\"c\rd\"{newline}"
            );
            assert_eq!(
                records(&text),
                vec![
                    owned(&["type", "address", "u16", "label"]),
                    owned(&["input", "1", "5", "a\rb"]),
                    owned(&["input", "2", "6", "c\rd"]),
                ],
                "{newline:?}"
            );
            assert_eq!(dec(&text).map(|snapshot| snapshot.len()), Ok(2));
        }
    }

    #[test]
    fn broken_quoting_names_the_line_of_the_record() {
        assert_eq!(read_csv("a,b\nc,\"open\nstill open"), Err(2));
        assert_eq!(read_csv("a,\"closed\"junk\n"), Err(1));
    }

    #[test]
    fn a_default_dump_parses_every_row() {
        let text = "type,address,time,u16,i16,hex,ascii,bits,custom,label\n\
            input,0,14:02:42.850,2314,2314,090A,....,0000 1001 0000 1010,231.4 V,voltage L1\n\
            input,9,14:02:42.850,38042,-27494,949A,....,1001 0100 1001 1010,part of ^,\n\
            holding,3,14:02:43.000,7,7,0007,....,0000 0000 0000 0111,-,\"set: v, A\"\n";
        let snapshot = dec(text).unwrap();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(
            snapshot.get(&(INPUT, 0)),
            Some(&SnapshotEntry {
                value: 2314,
                time: Some("14:02:42.850".to_string()),
            })
        );
        assert_eq!(value(&snapshot, (INPUT, 9)), 38042);
        assert_eq!(value(&snapshot, (RegisterType::Holding, 3)), 7);
    }

    #[test]
    fn the_raw_value_comes_from_any_raw_column_alone() {
        for (column, text) in [
            ("u16", "38042"),
            ("hex", "949A"),
            ("i16", "-27494"),
            ("bits", "1001 0100 1001 1010"),
        ] {
            let dump = format!("type,address,{column},label\ninput,9,{text},x\n");
            let snapshot = dec(&dump).unwrap_or_else(|e| panic!("{column}: {e}"));
            assert_eq!(value(&snapshot, (INPUT, 9)), 38042, "from {column}");
            assert_eq!(snapshot[&(INPUT, 9)].time, None, "no time column");
        }
    }

    #[test]
    fn u16_wins_over_the_other_raw_columns() {
        let snapshot = dec("type,address,bits,i16,hex,u16\ninput,1,0000 0000 0000 0001,2,0003,4\n");
        assert_eq!(value(&snapshot.unwrap(), (INPUT, 1)), 4);
        let snapshot = dec("type,address,bits,i16,hex\ninput,1,0000 0000 0000 0001,2,0003\n");
        assert_eq!(value(&snapshot.unwrap(), (INPUT, 1)), 3);
        let snapshot = dec("type,address,bits,i16\ninput,1,0000 0000 0000 0001,2\n");
        assert_eq!(value(&snapshot.unwrap(), (INPUT, 1)), 2);
    }

    #[test]
    fn dumps_without_the_needed_columns_are_rejected() {
        assert_eq!(
            dec("address,type,u16\n5,input,1\n"),
            Err(SnapshotError::NotADump)
        );
        assert_eq!(dec("hello there"), Err(SnapshotError::NotADump));
        assert_eq!(
            dec("type,time,u16\ninput,-,1\n"),
            Err(SnapshotError::NoAddressColumn)
        );
        assert_eq!(
            dec("type,address,ascii,custom,label\ninput,1,....,-,x\n"),
            Err(SnapshotError::NoRawColumn)
        );
        assert_eq!(dec("type,address,u16\n\n"), Err(SnapshotError::NoRows));
    }

    #[test]
    fn a_malformed_row_names_its_line() {
        let header = "type,address,u16\n";
        for (row, line) in [
            ("input,1,2\ninput,2\n", 3),
            ("input,1,2\ninput,2,3,4\n", 3),
            ("register,1,2\n", 2),
            ("input,x,2\n", 2),
            ("input,1,70000\n", 2),
            ("input,1,\"2\n", 2),
        ] {
            assert_eq!(
                dec(&format!("{header}{row}")),
                Err(SnapshotError::MalformedRow(line)),
                "{row:?}"
            );
        }
        assert_eq!(
            dec("type,address,hex\ninput,1,FF\n"),
            Err(SnapshotError::MalformedRow(2)),
            "hex is four digits"
        );
        assert_eq!(
            dec("type,address,bits\ninput,1,1111111100000000\n"),
            Err(SnapshotError::MalformedRow(2)),
            "bits come in four groups"
        );
    }

    #[test]
    fn addresses_follow_the_address_mode_and_accept_a_hex_prefix() {
        let text = "type,address,u16\ninput,1F,5\n";
        assert_eq!(dec(text), Err(SnapshotError::MalformedRow(2)));
        let snapshot = parse_dump(text, AddressMode::Hex).unwrap();
        assert_eq!(value(&snapshot, (INPUT, 31)), 5);

        for mode in AddressMode::ALL {
            let snapshot = parse_dump("type,address,u16\nINPUT,0x1f,6\n", mode).unwrap();
            assert_eq!(value(&snapshot, (INPUT, 31)), 6, "{mode:?}");
        }
    }

    #[test]
    fn blank_lines_are_skipped_and_the_last_duplicate_wins() {
        let text = "\u{feff}type,address,time,u16\r\n\r\ninput,1,,5\r\ninput,1,12:00,6\r\n\r\n\r\n";
        let snapshot = dec(text).unwrap();
        assert_eq!(
            snapshot.get(&(INPUT, 1)),
            Some(&SnapshotEntry {
                value: 6,
                time: Some("12:00".to_string()),
            })
        );
        assert_eq!(
            dec("type,address,time,u16\ninput,1,,5\n").unwrap()[&(INPUT, 1)].time,
            None
        );
    }

    #[test]
    fn ages_written_in_the_ago_mode_are_not_kept_as_the_snapshot_time() {
        for age in ["now", "3s ago", "59s ago", "12m ago", ">1h ago"] {
            let snapshot = dec(&format!("type,address,time,u16\ninput,1,{age},5\n")).unwrap();
            assert_eq!(snapshot[&(INPUT, 1)].time, None, "{age}");
        }
        for time in ["14:02:42.850", "nowhere", "s ago", "3h ago"] {
            let snapshot = dec(&format!("type,address,time,u16\ninput,1,{time},5\n")).unwrap();
            assert_eq!(snapshot[&(INPUT, 1)].time.as_deref(), Some(time), "{time}");
        }
    }

    fn snapshot_of(values: &[(u16, u16)]) -> Snapshot {
        values
            .iter()
            .map(|&(address, value)| ((INPUT, address), SnapshotEntry { value, time: None }))
            .collect()
    }

    fn current(values: &[(u16, u16)]) -> impl Fn(RegisterCell) -> Option<u16> + '_ {
        move |cell| {
            values
                .iter()
                .find(|&&(address, _)| (INPUT, address) == cell)
                .map(|&(_, value)| value)
        }
    }

    const RECORDED: [(u16, u16); 4] = [(1, 10), (2, 20), (3, 30), (4, 40)];
    const NOW: [(u16, u16); 3] = [(1, 10), (2, 21), (4, 44)];

    fn marks(rows: &[DiffRow]) -> Vec<(u16, char)> {
        rows.iter()
            .map(|row| (row.cell.1, row.mark.symbol()))
            .collect()
    }

    #[test]
    fn cells_are_classified_against_the_current_values() {
        assert_eq!(DiffKind::of(5, Some(5)), DiffKind::Same);
        assert_eq!(DiffKind::of(5, Some(6)), DiffKind::Changed);
        assert_eq!(DiffKind::of(5, None), DiffKind::Unread);

        let summary = Diff::new(&snapshot_of(&RECORDED), current(&NOW)).summary;
        assert_eq!(
            summary,
            DiffSummary {
                changed: 2,
                same: 1,
                unread: 1,
            }
        );
        assert!(!summary.all_unread());
        assert!(
            Diff::new(&snapshot_of(&RECORDED), current(&[]))
                .summary
                .all_unread()
        );
    }

    #[test]
    fn changed_cells_become_a_before_and_after_pair_in_address_order() {
        let diff = Diff::new(&snapshot_of(&RECORDED), current(&NOW));
        assert_eq!(
            marks(diff.rows(false)),
            vec![(1, '='), (2, '-'), (2, '+'), (3, '?'), (4, '-'), (4, '+')]
        );
    }

    #[test]
    fn the_changed_filter_keeps_only_the_pairs() {
        let diff = Diff::new(&snapshot_of(&RECORDED), current(&NOW));
        assert_eq!(
            marks(diff.rows(true)),
            vec![(2, '-'), (2, '+'), (4, '-'), (4, '+')]
        );
        let unchanged = Diff::new(&snapshot_of(&RECORDED), current(&RECORDED));
        assert!(unchanged.rows(true).is_empty());
        assert_eq!(unchanged.rows(false).len(), RECORDED.len());
    }
}
