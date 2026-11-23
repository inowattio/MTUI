use crate::config::Registers;
use crate::custom::{
    ASCII_REGISTERS_MAX, BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry,
    OpKind, ScaleMode, ScaleRepr,
};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

pub const VERSION: i64 = 2;

#[derive(Deserialize)]
struct Header {
    mbmap: Integer,
}

#[derive(Deserialize)]
struct Edition {
    revision: Option<Integer>,
    revised: Option<String>,
    x_status: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Verified,
    Official,
    Unofficial,
}

impl Status {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Official => "official",
            Self::Unofficial => "unofficial",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    number: Option<i64>,
    date: Option<String>,
}

fn is_date(text: &str) -> bool {
    text.len() == 10
        && text.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            _ => b.is_ascii_digit(),
        })
}

impl fmt::Display for Revision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.number, &self.date) {
            (Some(number), Some(date)) => write!(f, "revision {number}, revised {date}"),
            (Some(number), None) => write!(f, "revision {number}"),
            (None, Some(date)) => write!(f, "revised {date}"),
            (None, None) => Ok(()),
        }
    }
}

#[derive(Deserialize)]
struct DeviceMap {
    mbmap: Integer,
    address_base: Option<Integer>,
    byte_order: Option<String>,
    word_order: Option<String>,
    #[serde(default)]
    enums: Map<String, Value>,
    #[serde(default)]
    sentinels: Map<String, Value>,
    registers: Vec<Value>,
    firmware_min: Option<String>,
    firmware_max: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Integer {
    Number(i64),
    Text(String),
}

impl Integer {
    fn value(&self) -> Option<i64> {
        match self {
            Self::Number(number) => Some(*number),
            Self::Text(text) => parse_integer(text),
        }
    }
}

fn parse_integer(text: &str) -> Option<i64> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Area {
    Holding,
    Input,
    Coil,
    Discrete,
}

impl From<Area> for RegisterType {
    fn from(area: Area) -> Self {
        match area {
            Area::Holding => Self::Holding,
            Area::Input => Self::Input,
            Area::Coil => Self::Coil,
            Area::Discrete => Self::Discrete,
        }
    }
}

#[derive(Deserialize)]
struct Register {
    name: String,
    area: Area,
    address: Integer,
    #[serde(rename = "type")]
    data_type: Option<String>,
    unit: Option<String>,
    length: Option<Integer>,
    bit: Option<Integer>,
    bits: Option<Integer>,
    scale: Option<f64>,
    scale_factor: Option<String>,
    offset: Option<f64>,
    #[serde(rename = "enum")]
    labels: Option<Value>,
    byte_order: Option<String>,
    word_order: Option<String>,
    firmware_min: Option<String>,
    firmware_max: Option<String>,
}

fn version(text: &str) -> Option<Vec<u64>> {
    text.split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        })
        .collect()
}

fn against(firmware: &[u64], bound: &[u64]) -> Ordering {
    bound
        .iter()
        .enumerate()
        .map(|(i, bound)| firmware.get(i).unwrap_or(&0).cmp(bound))
        .find(|order| order.is_ne())
        .unwrap_or(Ordering::Equal)
}

fn within(firmware: &[u64], bound: Option<&str>, keep: fn(Ordering) -> bool) -> bool {
    bound
        .and_then(version)
        .is_none_or(|bound| keep(against(firmware, &bound)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareRange {
    min: Option<String>,
    max: Option<String>,
}

impl FirmwareRange {
    fn new(min: Option<&str>, max: Option<&str>) -> Option<Self> {
        let valid = |bound: Option<&str>| {
            bound
                .filter(|bound| version(bound).is_some())
                .map(str::to_string)
        };
        let range = Self {
            min: valid(min),
            max: valid(max),
        };
        (range.min.is_some() || range.max.is_some()).then_some(range)
    }

    fn contains(&self, firmware: &[u64]) -> bool {
        within(firmware, self.min.as_deref(), Ordering::is_ge)
            && within(firmware, self.max.as_deref(), Ordering::is_le)
    }

    pub fn applies(&self, firmware: &str) -> bool {
        version(firmware).is_none_or(|firmware| self.contains(&firmware))
    }

    fn key(&self) -> (Vec<u64>, Vec<u64>) {
        let bound =
            |bound: Option<&str>, open: u64| bound.and_then(version).unwrap_or_else(|| vec![open]);
        (
            bound(self.min.as_deref(), 0),
            bound(self.max.as_deref(), u64::MAX),
        )
    }
}

impl fmt::Display for FirmwareRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.min, &self.max) {
            (Some(min), Some(max)) if min == max => write!(f, "{min} only"),
            (Some(min), Some(max)) => write!(f, "{min} to {max}"),
            (Some(min), None) => write!(f, "{min} and later"),
            (None, Some(max)) => write!(f, "up to {max}"),
            (None, None) => write!(f, "any"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Firmware {
    pub map: Option<FirmwareRange>,
    pub ranges: Vec<(FirmwareRange, usize)>,
}

fn table_entries(table: Option<&Value>) -> Vec<EnumEntry> {
    let Some(Value::Object(table)) = table else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(key, text)| {
            Some(EnumEntry {
                value: parse_integer(key)?,
                text: text.as_str()?.to_string(),
            })
        })
        .collect()
}

fn sentinel(value: i64, data_type: &str) -> Option<i64> {
    let (bits, signed) = match data_type {
        "int16" => (16, true),
        "uint16" => (16, false),
        "int32" => (32, true),
        "uint32" => (32, false),
        "int64" => return Some(value),
        "uint64" => return (value >= 0).then_some(value),
        _ => return None,
    };
    let span = 1i64 << bits;
    let (min, max) = if signed {
        (-span / 2, span / 2 - 1)
    } else {
        (0, span - 1)
    };
    match value {
        value if (min..=max).contains(&value) => Some(value),
        value if signed && (0..span).contains(&value) => Some(value - span),
        _ => None,
    }
}

fn repr(data_type: Option<&str>) -> Option<CustomRepr> {
    Some(match data_type? {
        "uint16" => CustomRepr::U16,
        "int16" => CustomRepr::I16,
        "uint32" => CustomRepr::U32,
        "int32" => CustomRepr::I32,
        "float32" => CustomRepr::F32,
        "uint64" => CustomRepr::U64,
        "int64" => CustomRepr::I64,
        "float64" => CustomRepr::F64,
        "string" => CustomRepr::Ascii,
        _ => return None,
    })
}

fn little(register: Option<&str>, map: Option<&str>) -> bool {
    register.or(map) == Some("little")
}

const fn word_order(byte_swap: bool, word_swap: bool) -> WordOrder {
    match (byte_swap, word_swap) {
        (false, false) => WordOrder::ABCD,
        (true, false) => WordOrder::BADC,
        (false, true) => WordOrder::CDAB,
        (true, true) => WordOrder::DCBA,
    }
}

type Names<'a> = BTreeMap<&'a str, &'a Register>;

impl Register {
    fn firmware(&self) -> Option<FirmwareRange> {
        FirmwareRange::new(self.firmware_min.as_deref(), self.firmware_max.as_deref())
    }

    fn applies_to(&self, firmware: Option<&[u64]>) -> bool {
        firmware.is_none_or(|firmware| self.firmware().is_none_or(|range| range.contains(firmware)))
    }

    fn address(&self, base: i64) -> Option<u16> {
        let address = self.address.value()?.checked_sub(base)?;
        u16::try_from(address).ok()
    }

    fn exponent(&self, area: Area, base: i64) -> Option<CustomScale> {
        if self.area != area || self.bits.is_some() || self.scale_factor.is_some() {
            return None;
        }
        let repr = match self.data_type.as_deref()? {
            "int16" => ScaleRepr::Sunspec,
            "uint16" => ScaleRepr::Plain(CustomRepr::U16),
            "int32" => ScaleRepr::Plain(CustomRepr::I32),
            "uint32" => ScaleRepr::Plain(CustomRepr::U32),
            "int64" => ScaleRepr::Plain(CustomRepr::I64),
            "uint64" => ScaleRepr::Plain(CustomRepr::U64),
            _ => return None,
        };
        Some(CustomScale {
            register: self.address(base)?,
            mode: ScaleMode::Pow10,
            repr,
        })
    }

    fn unit(&self) -> Option<&str> {
        self.unit.as_deref().filter(|unit| !unit.is_empty())
    }

    fn label(&self) -> String {
        match self.unit() {
            Some(unit) => format!("{} [{unit}]", self.name),
            None => self.name.clone(),
        }
    }

    fn length(&self) -> Option<u8> {
        let length = self.length.as_ref()?.value().filter(|&n| n >= 1)?;
        u8::try_from(length.min(ASCII_REGISTERS_MAX.into())).ok()
    }

    fn shared_bit(&self, bit: i64) -> Option<u8> {
        let width = self.bits.as_ref().and_then(Integer::value).unwrap_or(1);
        u8::try_from(bit).ok().filter(|&bit| width == 1 && bit < 16)
    }
}

impl DeviceMap {
    fn enum_map(&self, register: &Register) -> Vec<EnumEntry> {
        let table = match &register.labels {
            Some(Value::String(name)) => self.enums.get(name),
            other => other.as_ref(),
        };
        let mut entries = table_entries(table);
        if let Some(data_type) = register.data_type.as_deref() {
            for entry in table_entries(self.sentinels.get(data_type)) {
                let Some(value) = sentinel(entry.value, data_type) else {
                    continue;
                };
                if !entries.iter().any(|known| known.value == value) {
                    entries.push(EnumEntry { value, ..entry });
                }
            }
        }
        entries
    }

    fn rule(
        &self,
        register: &Register,
        address: u16,
        base: i64,
        names: &Names<'_>,
    ) -> Option<CustomRule> {
        let repr = repr(register.data_type.as_deref())?;
        let text = repr == CustomRepr::Ascii;
        let registers = if text { Some(register.length()?) } else { None };
        let scale = match register.scale_factor.as_deref().filter(|_| !text) {
            Some(name) => Some(names.get(name)?.exponent(register.area, base)?),
            None => None,
        };
        let ops = [
            register
                .scale
                .filter(|&v| (v - 1.0).abs() > f64::EPSILON)
                .map(|v| CustomOp { op: OpKind::Mul, v }),
            register
                .offset
                .filter(|&v| v != 0.0)
                .map(|v| CustomOp { op: OpKind::Add, v }),
        ];
        let byte_swap = little(register.byte_order.as_deref(), self.byte_order.as_deref());
        let word_swap = !text && little(register.word_order.as_deref(), self.word_order.as_deref());
        Some(CustomRule {
            address,
            repr,
            registers,
            scale,
            ops: ops.into_iter().flatten().collect(),
            enum_map: self.enum_map(register),
            suffix: register.unit().map_or_default(|unit| format!(" {unit}")),
            word_order: Some(word_order(byte_swap, word_swap)),
            ..Default::default()
        })
    }
}

#[derive(Default)]
struct Import {
    labels: BTreeMap<RegisterCell, String>,
    rules: BTreeMap<RegisterCell, CustomRule>,
}

impl Import {
    fn label(&mut self, cell: RegisterCell, label: String) {
        let label = match self.labels.remove(&cell) {
            Some(previous) => format!("{previous}; {label}"),
            None => label,
        };
        self.labels.insert(cell, label);
    }

    fn register(&mut self, map: &DeviceMap, base: i64, names: &Names<'_>, register: &Register) {
        let Some(address) = register.address(base) else {
            return;
        };
        let cell = (register.area.into(), address);
        self.label(cell, register.label());
        if cell.0.is_bit() {
            return;
        }
        if let Some(bit) = register.bit.as_ref().and_then(Integer::value) {
            if let Some(bit) = register.shared_bit(bit) {
                self.rules.entry(cell).or_default().bits.push(BitEntry {
                    bit,
                    name: register.name.clone(),
                });
            }
        } else if let Some(rule) = map.rule(register, address, base, names) {
            self.rules.insert(cell, rule);
        }
    }

    fn into_registers(self) -> Registers {
        let pinned: Vec<RegisterCell> = self.labels.keys().copied().collect();
        Registers::from_views(&pinned, &self.labels, &self.rules)
    }
}

fn json(text: &str) -> &str {
    text.trim_start_matches('\u{feff}').trim()
}

pub fn unsupported_version(text: &str) -> Option<i64> {
    let header: Header = serde_json::from_str(json(text)).ok()?;
    header.mbmap.value().filter(|&version| version != VERSION)
}

pub fn status(text: &str) -> Option<Status> {
    let edition: Edition = serde_json::from_str(json(text)).ok()?;
    [Status::Verified, Status::Official, Status::Unofficial]
        .into_iter()
        .find(|status| edition.x_status.as_deref() == Some(status.label()))
}

pub fn revision(text: &str) -> Option<Revision> {
    let edition: Edition = serde_json::from_str(json(text)).ok()?;
    let revision = Revision {
        number: edition
            .revision
            .as_ref()
            .and_then(Integer::value)
            .filter(|&number| number >= 1),
        date: edition.revised.filter(|date| is_date(date)),
    };
    (revision.number.is_some() || revision.date.is_some()).then_some(revision)
}

pub fn firmware(text: &str) -> Option<Firmware> {
    let map: DeviceMap = serde_json::from_str(json(text)).ok()?;
    let mut ranges: Vec<(FirmwareRange, usize)> = Vec::new();
    for range in map
        .registers
        .iter()
        .filter_map(|value| Register::deserialize(value).ok())
        .filter_map(|register| register.firmware())
    {
        match ranges.iter_mut().find(|(known, _)| *known == range) {
            Some((_, count)) => *count += 1,
            None => ranges.push((range, 1)),
        }
    }
    if ranges.is_empty() {
        return None;
    }
    ranges.sort_by_key(|(range, _)| range.key());
    Some(Firmware {
        map: FirmwareRange::new(map.firmware_min.as_deref(), map.firmware_max.as_deref()),
        ranges,
    })
}

pub fn parse(text: &str) -> Option<Registers> {
    parse_for_firmware(text, "")
}

pub fn parse_for_firmware(text: &str, firmware: &str) -> Option<Registers> {
    let firmware = version(firmware);
    let map: DeviceMap = serde_json::from_str(json(text)).ok()?;
    if map.mbmap.value()? != VERSION {
        return None;
    }
    let base = map
        .address_base
        .as_ref()
        .and_then(Integer::value)
        .unwrap_or(0);
    let registers: Vec<Register> = map
        .registers
        .iter()
        .filter_map(|value| Register::deserialize(value).ok())
        .filter(|register| register.applies_to(firmware.as_deref()))
        .collect();
    let mut names = Names::new();
    for register in &registers {
        names.entry(register.name.as_str()).or_insert(register);
    }
    let mut import = Import::default();
    for register in &registers {
        import.register(&map, base, &names, register);
    }
    let registers = import.into_registers();
    (registers.total() > 0).then_some(registers)
}

#[cfg(test)]
mod tests {
    use super::{
        FirmwareRange, Status, firmware, json, parse, parse_for_firmware, revision, sentinel,
        status, unsupported_version, version, within,
    };
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::{CustomRepr, CustomScale, ScaleMode, ScaleRepr};
    use crate::modbus::WordOrder;
    use serde_json::{Value, json};
    use std::cmp::Ordering;

    fn map(registers: Value) -> Value {
        json!({"mbmap": 2, "vendor": "V", "model": "M", "word_order": "little", "registers": registers})
    }

    #[test]
    fn converts_types_scale_unit_and_order() {
        let r = parse(
            &map(json!([
                {"name": "Voltage", "area": "input", "address": "0x0010", "type": "float32", "unit": "V"},
                {"name": "Energy", "area": "holding", "address": 4, "type": "uint32", "scale": 0.1, "unit": "kWh", "word_order": "big"},
                {"name": "Temp", "area": "holding", "address": 6, "type": "int16", "offset": -40, "byte_order": "little"},
            ]))
            .to_string(),
        )
        .unwrap();
        let voltage = &r.inputs[0];
        assert_eq!(
            (voltage.address, voltage.label.as_deref()),
            (16, Some("Voltage [V]"))
        );
        let rule = voltage.custom.as_ref().unwrap();
        assert_eq!(
            (rule.repr, rule.word_order, rule.suffix.as_str()),
            (CustomRepr::F32, Some(WordOrder::CDAB), " V")
        );
        assert!(voltage.pinned);

        let energy = r.holdings[0].custom.as_ref().unwrap();
        assert_eq!(energy.word_order, Some(WordOrder::ABCD));
        assert_eq!(energy.evaluate(&[0, 1234], WordOrder::ABCD), "123.4 kWh");
        let temp = r.holdings[1].custom.as_ref().unwrap();
        assert_eq!(temp.word_order, Some(WordOrder::DCBA));
        assert_eq!(temp.ops[0].display(), "+-40");
    }

    #[test]
    fn shares_bits_and_resolves_enum_tables() {
        let mut value = map(json!([
            {"name": "Relay", "area": "holding", "address": 1, "type": "bool", "bit": 0},
            {"name": "Alarm", "area": "holding", "address": 1, "type": "bool", "bit": 3},
            {"name": "State", "area": "holding", "address": 2, "type": "uint16", "enum": "run"},
            {"name": "Mode", "area": "holding", "address": 3, "type": "uint16", "enum": {"0x1": "Auto"}},
            {"name": "Out", "area": "coil", "address": 0, "type": "bool"},
        ]));
        value["enums"] = json!({"run": {"0": "Stopped", "1": "Running"}});
        let r = parse(&value.to_string()).unwrap();
        let bits = r.holdings[0].custom.as_ref().unwrap();
        assert_eq!(r.holdings[0].label.as_deref(), Some("Relay; Alarm"));
        assert_eq!(bits.evaluate(&[0b1001], WordOrder::ABCD), "Relay|Alarm");
        let state = r.holdings[1].custom.as_ref().unwrap();
        assert_eq!(state.evaluate(&[1], WordOrder::ABCD), "Running");
        assert_eq!(r.holdings[2].custom.as_ref().unwrap().enum_map[0].value, 1);
        assert_eq!(
            (r.coils[0].label.as_deref(), r.coils[0].custom.is_none()),
            (Some("Out"), true)
        );
    }

    #[test]
    fn reads_strings_with_length_and_byte_order() {
        let r = parse(
            &map(json!([
                {"name": "Serial", "area": "holding", "address": 0, "type": "string", "length": 3},
                {"name": "Swapped", "area": "holding", "address": 3, "type": "string", "length": 2, "byte_order": "little"},
                {"name": "Huge", "area": "holding", "address": 5, "type": "string", "length": 500},
            ]))
            .to_string(),
        )
        .unwrap();
        let serial = r.holdings[0].custom.as_ref().unwrap();
        assert_eq!(
            (serial.repr, serial.registers, serial.word_order),
            (CustomRepr::Ascii, Some(3), Some(WordOrder::ABCD))
        );
        assert_eq!(
            serial.evaluate(&[0x4142, 0x4320, 0x0000], WordOrder::ABCD),
            "ABC"
        );
        let swapped = r.holdings[1].custom.as_ref().unwrap();
        assert_eq!(swapped.word_order, Some(WordOrder::BADC));
        assert_eq!(swapped.evaluate(&[0x4241, 0x4443], WordOrder::ABCD), "ABCD");
        assert_eq!(r.holdings[2].custom.as_ref().unwrap().registers, Some(125));
    }

    #[test]
    fn applies_address_base_and_skips_what_cannot_be_read() {
        let mut value = map(json!([
            {"name": "A", "area": "holding", "address": 1, "type": "uint16"},
            {"name": "Below base", "area": "holding", "address": 0, "type": "uint16"},
            {"name": "Serial", "area": "holding", "address": 10, "type": "string"},
            {"name": "Bad", "area": "Holding", "address": 2, "type": "uint16"},
        ]));
        value["address_base"] = json!(1);
        let r = parse(&value.to_string()).unwrap();
        assert_eq!(
            r.holdings.iter().map(|e| e.address).collect::<Vec<_>>(),
            [0, 9]
        );
        assert!(r.holdings[1].custom.is_none());
        assert!(parse(r#"{"mbmap": 3, "registers": []}"#).is_none());
        assert!(parse(r#"{"holdings": []}"#).is_none());
        assert!(parse(r#"{"mbmap": 2, "registers": []}"#).is_none());
    }

    #[test]
    fn reports_only_versions_it_cannot_read() {
        assert_eq!(
            unsupported_version(r#"{"mbmap": 3, "registers": []}"#),
            Some(3)
        );
        assert_eq!(unsupported_version(r#"{"mbmap": "0x1"}"#), Some(1));
        assert_eq!(unsupported_version(&map(json!([])).to_string()), None);
        assert_eq!(unsupported_version(r#"{"holdings": []}"#), None);
        assert_eq!(unsupported_version(r#"{"mbmap": "next"}"#), None);
        assert_eq!(unsupported_version("mbmap: 3"), None);
    }

    #[test]
    fn scale_factors_point_at_the_first_register_with_that_name() {
        let mut value = map(json!([
            {"name": "AC power", "area": "holding", "address": 11, "type": "int16", "unit": "W", "scale_factor": "W_SF"},
            {"name": "W_SF", "area": "holding", "address": 12, "type": "int16"},
            {"name": "W_SF", "area": "holding", "address": 13, "type": "uint16"},
            {"name": "Energy", "area": "holding", "address": 21, "type": "uint32", "scale_factor": "E_SF", "scale": 2, "offset": 1},
            {"name": "E_SF", "area": "holding", "address": 23, "type": "uint32"},
        ]));
        value["address_base"] = json!(1);
        let r = parse(&value.to_string()).unwrap();

        let power = r.holdings[0].custom.as_ref().unwrap();
        assert_eq!(
            power.scale,
            Some(CustomScale {
                register: 11,
                mode: ScaleMode::Pow10,
                repr: ScaleRepr::Sunspec,
            })
        );
        assert_eq!(
            power.evaluate(&[2345, (-1i16) as u16], WordOrder::ABCD),
            "234.5 W"
        );
        assert_eq!(
            power.evaluate(&[2345, 0x8000], WordOrder::ABCD),
            format!("{UNINTERPRETABLE} W")
        );

        let energy = r.holdings[3].custom.as_ref().unwrap();
        assert_eq!(
            energy.scale.map(|scale| (scale.register, scale.repr)),
            Some((22, ScaleRepr::Plain(CustomRepr::U32)))
        );
        assert_eq!(energy.evaluate(&[5, 0, 2, 0], WordOrder::ABCD), "1001");
    }

    #[test]
    fn unusable_scale_factors_leave_the_register_label_only() {
        let r = parse(
            &map(json!([
                {"name": "Missing", "area": "holding", "address": 0, "type": "int16", "unit": "W", "scale_factor": "nope"},
                {"name": "Float", "area": "holding", "address": 1, "type": "int16", "scale_factor": "F_SF"},
                {"name": "F_SF", "area": "holding", "address": 2, "type": "float32"},
                {"name": "Field", "area": "holding", "address": 4, "type": "int16", "scale_factor": "B_SF"},
                {"name": "B_SF", "area": "holding", "address": 5, "type": "uint16", "bit": 0, "bits": 4},
                {"name": "Chained", "area": "holding", "address": 6, "type": "int16", "scale_factor": "Field"},
                {"name": "Elsewhere", "area": "holding", "address": 7, "type": "int16", "scale_factor": "I_SF"},
                {"name": "I_SF", "area": "input", "address": 8, "type": "int16"},
                {"name": "Text", "area": "holding", "address": 9, "type": "string", "length": 1, "scale_factor": "I_SF"},
            ]))
            .to_string(),
        )
        .unwrap();
        let ruled = |address: u16| {
            r.holdings
                .iter()
                .find(|e| e.address == address)
                .and_then(|e| e.custom.as_ref())
        };
        assert_eq!(r.holdings[0].label.as_deref(), Some("Missing [W]"));
        for address in [0, 1, 4, 6, 7] {
            assert!(ruled(address).is_none(), "{address}");
        }
        let text = ruled(9).unwrap();
        assert_eq!((text.repr, text.scale), (CustomRepr::Ascii, None));
    }

    #[test]
    fn versions_are_dotted_whole_numbers() {
        assert_eq!(version("1.02.7"), Some(vec![1, 2, 7]));
        assert_eq!(version("2"), Some(vec![2]));
        for text in ["", "2.", ".1", "1..2", "v2", "2.1b", "-1", " 2"] {
            assert_eq!(version(text), None, "{text:?}");
        }
    }

    #[test]
    fn bounds_cover_every_version_that_starts_with_them() {
        let min = |firmware: &str, bound: &str| {
            within(&version(firmware).unwrap(), Some(bound), Ordering::is_ge)
        };
        let max = |firmware: &str, bound: &str| {
            within(&version(firmware).unwrap(), Some(bound), Ordering::is_le)
        };
        assert!(min("2.1", "2.1") && min("2.1.4", "2.1") && min("3.0", "2.1"));
        assert!(!min("2.0.9", "2.1") && !min("2", "2.1"));
        assert!(min("1.10", "1.9") && min("1.2", "1.02"));
        assert!(max("2.9.9", "2") && max("2.3.7", "2.3") && max("2", "2.3"));
        assert!(!max("3.0", "2") && !max("2.4", "2.3"));
        assert!(within(&[1], Some("not a version"), Ordering::is_ge));
        assert!(within(&[1], None, Ordering::is_le));
    }

    #[test]
    fn a_firmware_version_skips_the_registers_of_other_firmware() {
        let text = map(json!([
            {"name": "Voltage", "area": "input", "address": 10, "type": "uint16"},
            {"name": "Energy", "area": "input", "address": 20, "type": "uint32", "firmware_max": "2.0"},
            {"name": "Energy", "area": "input", "address": 40, "type": "uint64", "firmware_min": "2.1"},
            {"name": "Power factor", "area": "input", "address": 30, "type": "int16", "firmware_min": "2.1"},
        ]))
        .to_string();
        let addresses = |firmware: &str| {
            parse_for_firmware(&text, firmware)
                .unwrap()
                .inputs
                .iter()
                .map(|e| e.address)
                .collect::<Vec<_>>()
        };
        assert_eq!(addresses("1.9"), [10, 20]);
        assert_eq!(addresses("2.0.5"), [10, 20]);
        assert_eq!(addresses("2.1"), [10, 30, 40]);
        assert_eq!(addresses(""), [10, 20, 30, 40]);
        assert_eq!(addresses("2."), [10, 20, 30, 40]);
        assert!(firmware(&text).is_some());
        assert!(
            firmware(
                &map(json!([{"name": "A", "area": "input", "address": 0, "type": "uint16"}]))
                    .to_string()
            )
            .is_none()
        );
    }

    #[test]
    fn scale_factor_names_resolve_among_the_registers_of_the_firmware() {
        let text = map(json!([
            {"name": "Power", "area": "holding", "address": 0, "type": "int16", "scale_factor": "SF"},
            {"name": "SF", "area": "holding", "address": 1, "type": "int16", "firmware_max": "1"},
            {"name": "SF", "area": "holding", "address": 2, "type": "int16", "firmware_min": "2"},
        ]))
        .to_string();
        let scale_register = |firmware: &str| {
            parse_for_firmware(&text, firmware).unwrap().holdings[0]
                .custom
                .as_ref()
                .and_then(|rule| rule.scale)
                .map(|scale| scale.register)
        };
        assert_eq!(scale_register("1.4"), Some(1));
        assert_eq!(scale_register("2.0"), Some(2));
        assert_eq!(scale_register(""), Some(1));
    }

    #[test]
    fn firmware_lists_each_range_once_in_version_order_with_its_register_count() {
        let mut value = map(json!([
            {"name": "A", "area": "input", "address": 0, "type": "uint16", "firmware_min": "1.10"},
            {"name": "B", "area": "input", "address": 1, "type": "uint16", "firmware_min": "1.9"},
            {"name": "C", "area": "input", "address": 2, "type": "uint16", "firmware_min": "1.10"},
            {"name": "D", "area": "input", "address": 3, "type": "uint16", "firmware_max": "1"},
            {"name": "E", "area": "input", "address": 4, "type": "uint16", "firmware_min": "2", "firmware_max": "2"},
            {"name": "F", "area": "input", "address": 5, "type": "uint16", "firmware_min": "v3"},
            {"name": "G", "area": "input", "address": 6, "type": "uint16"},
        ]));
        value["firmware_min"] = json!("1");
        value["firmware_max"] = json!("2");
        let found = firmware(&value.to_string()).unwrap();

        let listed: Vec<(String, usize)> = found
            .ranges
            .iter()
            .map(|(range, count)| (range.to_string(), *count))
            .collect();
        assert_eq!(
            listed,
            [
                ("up to 1".to_string(), 1),
                ("1.9 and later".to_string(), 1),
                ("1.10 and later".to_string(), 2),
                ("2 only".to_string(), 1),
            ]
        );
        let map_range = found.map.unwrap();
        assert_eq!(map_range.to_string(), "1 to 2");
        assert!(map_range.applies("2.7") && map_range.applies("") && map_range.applies("2."));
        assert!(!map_range.applies("3.0") && !map_range.applies("0.9"));
    }

    #[test]
    fn a_range_applies_to_the_versions_it_covers() {
        let range = FirmwareRange::new(Some("1.5"), None).unwrap();
        assert!(range.applies("1.5") && range.applies("2") && !range.applies("1.4.9"));
        assert!(FirmwareRange::new(Some("x"), Some("")).is_none());
        assert!(FirmwareRange::new(None, None).is_none());
    }

    #[test]
    fn revision_shows_the_number_and_date_that_are_well_formed() {
        let shown = |extra: Value| {
            let mut value = map(json!([]));
            for (key, item) in extra.as_object().unwrap() {
                value[key] = item.clone();
            }
            revision(&value.to_string()).map(|r| r.to_string())
        };
        assert_eq!(
            shown(json!({"revision": 3, "revised": "2026-11-14"})).as_deref(),
            Some("revision 3, revised 2026-11-14")
        );
        assert_eq!(
            shown(json!({"revision": "0x2"})).as_deref(),
            Some("revision 2")
        );
        assert_eq!(
            shown(json!({"revision": 0, "revised": "2026-02-01"})).as_deref(),
            Some("revised 2026-02-01")
        );
        assert_eq!(shown(json!({"revised": "14/11/2026"})), None);
        assert_eq!(shown(json!({})), None);
    }

    #[test]
    fn status_is_one_of_the_library_values() {
        let read = |value: Value| {
            let mut map = map(json!([]));
            map["x_status"] = value;
            status(&map.to_string())
        };
        assert_eq!(read(json!("verified")), Some(Status::Verified));
        assert_eq!(read(json!("official")), Some(Status::Official));
        assert_eq!(read(json!("unofficial")), Some(Status::Unofficial));
        assert_eq!(read(json!("Unofficial")), None);
        assert_eq!(read(json!("draft")), None);
        assert_eq!(read(json!(1)), None);
        assert_eq!(status(&map(json!([])).to_string()), None);
    }

    #[test]
    fn a_leading_byte_order_mark_is_ignored() {
        let mut value = map(json!([
            {"name": "A", "area": "input", "address": 0, "type": "uint16", "firmware_min": "2"},
        ]));
        value["revision"] = json!(4);
        value["x_status"] = json!("verified");
        let text = format!("\u{feff}{value}\n");
        assert_eq!(json(&text), value.to_string());
        assert_eq!(parse(&text).unwrap().labels(), 1);
        assert!(firmware(&text).is_some());
        assert_eq!(revision(&text).unwrap().to_string(), "revision 4");
        assert_eq!(status(&text), Some(Status::Verified));
        assert_eq!(
            unsupported_version("\u{feff}{\"mbmap\": 3, \"registers\": []}"),
            Some(3)
        );
    }

    #[test]
    fn sentinels_fit_their_type() {
        assert_eq!(sentinel(0xFFFF, "uint16"), Some(0xFFFF));
        assert_eq!(sentinel(0x7FFF, "int16"), Some(0x7FFF));
        assert_eq!(sentinel(0x8000, "int16"), Some(-32768));
        assert_eq!(sentinel(-32768, "int16"), Some(-32768));
        assert_eq!(sentinel(0xFFFF_FFFF, "int32"), Some(-1));
        assert_eq!(sentinel(0x1_0000, "uint16"), None);
        assert_eq!(sentinel(-1, "uint32"), None);
        assert_eq!(sentinel(-1, "uint64"), None);
        assert_eq!(sentinel(i64::MAX, "int64"), Some(i64::MAX));
        assert_eq!(sentinel(1, "float32"), None);
    }

    #[test]
    fn sentinels_label_reserved_raw_values_unless_the_register_enum_has_them() {
        let mut value = map(json!([
            {"name": "Power", "area": "holding", "address": 0, "type": "uint16", "scale": 0.1, "unit": "W"},
            {"name": "Temp", "area": "holding", "address": 1, "type": "int16"},
            {"name": "State", "area": "holding", "address": 2, "type": "uint16", "enum": {"0xFFFF": "Idle"}},
            {"name": "Energy", "area": "holding", "address": 3, "type": "uint32"},
            {"name": "Ratio", "area": "holding", "address": 5, "type": "float32"},
        ]));
        value["sentinels"] = json!({
            "uint16": {"0xFFFF": "Not available", "0xFFFE": "Over range"},
            "int16": {"0x8000": "Not implemented"},
            "uint32": {"0xFFFFFFFF": "Not available"},
            "float32": {"0": "Ignored"},
        });
        let r = parse(&value.to_string()).unwrap();
        let rule = |i: usize| r.holdings[i].custom.as_ref().unwrap();

        assert_eq!(rule(0).evaluate(&[0xFFFE], WordOrder::ABCD), "Over range W");
        assert_eq!(rule(0).evaluate(&[1234], WordOrder::ABCD), "123.4 W");
        assert_eq!(rule(0).numeric(&[0xFFFF], WordOrder::ABCD), None);
        assert_eq!(
            rule(1).evaluate(&[0x8000], WordOrder::ABCD),
            "Not implemented"
        );
        assert_eq!(rule(1).evaluate(&[0x7FFF], WordOrder::ABCD), "32767");
        assert_eq!(rule(2).evaluate(&[0xFFFF], WordOrder::ABCD), "Idle");
        assert_eq!(rule(2).evaluate(&[0xFFFE], WordOrder::ABCD), "Over range");
        assert_eq!(
            rule(3).evaluate(&[0xFFFF, 0xFFFF], WordOrder::ABCD),
            "Not available"
        );
        assert!(rule(4).enum_map.is_empty());
    }
}
