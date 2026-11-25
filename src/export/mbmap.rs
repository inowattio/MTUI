use crate::config::{Config, RegisterEntry};
use crate::custom::{CustomOp, CustomRepr, CustomRule, OpKind, ScaleMode, ScaleRepr};
use crate::import::mbmap::VERSION;
use crate::modbus::{DataBits, Interface, Parity, WordOrder};
use crate::register::{RegisterCell, RegisterType};
use serde::Serialize;
use serde_json::ser::Formatter;
use std::collections::{BTreeMap, BTreeSet};
use std::io;

const SCHEMA: &str =
    "https://raw.githubusercontent.com/nobrega8/mbmap-library/main/schema/mbmap-2.schema.json";
const VENDOR: &str = "Unknown";
const MODEL: &str = "MTUI export";
const DESCRIPTION: &str = "Exported from MTUI";
const EXPANDED: usize = 2;
const WORD_BITS: u8 = 16;
const SPACE: usize = 1 << 16;
const INTEGERS: [&str; 6] = ["int16", "uint16", "int32", "uint32", "int64", "uint64"];

#[derive(Serialize)]
struct DeviceMap {
    #[serde(rename = "$schema")]
    schema: &'static str,
    mbmap: i64,
    vendor: &'static str,
    model: String,
    description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    protocol: Option<&'static str>,
    unit_id: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baudrate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parity: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    databits: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stopbits: Option<u8>,
    byte_order: &'static str,
    word_order: &'static str,
    address_base: u8,
    registers: Vec<Register>,
}

#[derive(Clone, Serialize)]
struct Register {
    name: String,
    area: &'static str,
    address: u16,
    #[serde(rename = "type")]
    data_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bit: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scale_factor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    access: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    byte_order: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    word_order: Option<&'static str>,
    #[serde(rename = "enum", skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<i64, String>,
}

#[derive(Default)]
struct Layout {
    depth: usize,
    has_value: bool,
}

impl Layout {
    fn line<W: ?Sized + io::Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b"\n")?;
        writer.write_all("  ".repeat(self.depth).as_bytes())
    }

    fn open<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        self.depth += 1;
        self.has_value = false;
        writer.write_all(bracket)
    }

    fn close<W: ?Sized + io::Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        let expanded = self.depth <= EXPANDED;
        self.depth -= 1;
        if expanded && self.has_value {
            self.line(writer)?;
        }
        writer.write_all(bracket)
    }

    fn item<W: ?Sized + io::Write>(&self, writer: &mut W, first: bool) -> io::Result<()> {
        if !first {
            writer.write_all(b",")?;
        }
        if self.depth <= EXPANDED {
            self.line(writer)
        } else if first {
            Ok(())
        } else {
            writer.write_all(b" ")
        }
    }
}

impl Formatter for Layout {
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"[")
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"]")
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.item(writer, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"{")
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"}")
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.item(writer, first)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }
}

const fn side(little: bool) -> &'static str {
    if little { "little" } else { "big" }
}

const fn swaps_words(order: WordOrder) -> bool {
    matches!(order, WordOrder::CDAB | WordOrder::DCBA)
}

const fn oriented(bytes: bool, words: bool) -> WordOrder {
    match (bytes, words) {
        (false, false) => WordOrder::ABCD,
        (true, false) => WordOrder::BADC,
        (false, true) => WordOrder::CDAB,
        (true, true) => WordOrder::DCBA,
    }
}

fn width(data_type: &str) -> usize {
    match data_type {
        "int32" | "uint32" => 2,
        "int64" | "uint64" => 4,
        _ => 1,
    }
}

fn fits(address: u16, count: usize) -> bool {
    usize::from(address) + count <= SPACE
}

const fn area(kind: RegisterType) -> &'static str {
    match kind {
        RegisterType::Holding => "holding",
        RegisterType::Input => "input",
        RegisterType::Coil => "coil",
        RegisterType::Discrete => "discrete",
    }
}

const fn rank(kind: RegisterType) -> u8 {
    match kind {
        RegisterType::Holding => 0,
        RegisterType::Input => 1,
        RegisterType::Coil => 2,
        RegisterType::Discrete => 3,
    }
}

const fn data_type(repr: CustomRepr) -> Option<&'static str> {
    Some(match repr {
        CustomRepr::U16 => "uint16",
        CustomRepr::I16 => "int16",
        CustomRepr::U32 => "uint32",
        CustomRepr::I32 => "int32",
        CustomRepr::F32 => "float32",
        CustomRepr::U64 => "uint64",
        CustomRepr::I64 => "int64",
        CustomRepr::F64 => "float64",
        CustomRepr::Ascii => "string",
        CustomRepr::F16 | CustomRepr::U128 | CustomRepr::I128 | CustomRepr::F128 => return None,
    })
}

const fn exponent_type(repr: ScaleRepr) -> Option<&'static str> {
    Some(match repr {
        ScaleRepr::Sunspec | ScaleRepr::Plain(CustomRepr::I16) => "int16",
        ScaleRepr::Plain(CustomRepr::U16) => "uint16",
        ScaleRepr::Plain(CustomRepr::I32) => "int32",
        ScaleRepr::Plain(CustomRepr::U32) => "uint32",
        ScaleRepr::Plain(CustomRepr::I64) => "int64",
        ScaleRepr::Plain(CustomRepr::U64) => "uint64",
        ScaleRepr::Plain(_) => return None,
    })
}

fn affine(ops: &[CustomOp]) -> Option<(f64, f64)> {
    ops.iter()
        .try_fold((1.0, 0.0), |(a, b): (f64, f64), op| match op.op {
            OpKind::Add => Some((a, b + op.v)),
            OpKind::Sub => Some((a, b - op.v)),
            OpKind::Mul => Some((a * op.v, b * op.v)),
            OpKind::Div if op.v != 0.0 => Some((a / op.v, b / op.v)),
            OpKind::Pow if op.v == 1.0 => Some((a, b)),
            OpKind::Div | OpKind::Pow => None,
        })
        .filter(|(a, b)| a.is_finite() && b.is_finite())
}

fn named(text: &str) -> Option<&str> {
    Some(text).filter(|text| text.chars().any(|c| !c.is_whitespace()))
}

fn fallback((kind, address): RegisterCell) -> String {
    format!("{} {address}", kind.name())
}

fn contiguous(rule: &CustomRule, address: u16) -> bool {
    fits(address, rule.register_count())
        && rule
            .word_addresses()
            .zip(0u16..)
            .all(|(word, i)| word == address.wrapping_add(i))
}

impl Register {
    const fn new(name: String, (kind, address): RegisterCell, data_type: &'static str) -> Self {
        Self {
            name,
            area: area(kind),
            address,
            data_type,
            length: None,
            bit: None,
            scale: None,
            scale_factor: None,
            offset: None,
            unit: None,
            access: None,
            byte_order: None,
            word_order: None,
            labels: BTreeMap::new(),
        }
    }

    fn orient(&mut self, order: WordOrder, text: bool, defaults: WordOrder) {
        let bytes = order.swaps_bytes();
        let words = swaps_words(order);
        self.byte_order = (bytes != defaults.swaps_bytes()).then_some(side(bytes));
        self.word_order = (!text && words != swaps_words(defaults)).then_some(side(words));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Need {
    rank: u8,
    address: u16,
    data_type: &'static str,
    order: WordOrder,
}

struct Own {
    cell: RegisterCell,
    register: Register,
    retypable: bool,
    exponent: Option<(&'static str, WordOrder)>,
    need: Option<Need>,
}

struct Export {
    device: WordOrder,
    access: Option<&'static str>,
    own: Vec<Own>,
    bits: Vec<(RegisterCell, Register)>,
    extra: Vec<(RegisterCell, Register)>,
}

impl Export {
    fn new(config: &Config) -> Self {
        Self {
            device: config.device.word_order,
            access: (!config.read_only).then_some("RW"),
            own: Vec::new(),
            bits: Vec::new(),
            extra: Vec::new(),
        }
    }

    const fn flatten(&self, order: WordOrder, count: usize) -> WordOrder {
        if count == 1 {
            oriented(order.swaps_bytes(), swaps_words(self.device))
        } else {
            order
        }
    }

    fn access(&self, kind: RegisterType) -> Option<&'static str> {
        self.access.filter(|_| kind.is_writable())
    }

    fn plain(&mut self, cell: RegisterCell, name: String) {
        let mut register = Register::new(name, cell, "uint16");
        register.access = self.access(cell.0);
        self.own.push(Own {
            cell,
            register,
            retypable: true,
            exponent: None,
            need: None,
        });
    }

    fn entry(&mut self, cell: RegisterCell, entry: &RegisterEntry) {
        let label = entry.label.as_deref().and_then(named);
        if cell.0.is_bit() {
            if let Some(label) = label {
                let mut register = Register::new(label.to_string(), cell, "bool");
                register.access = self.access(cell.0);
                self.bits.push((cell, register));
            }
            return;
        }
        let Some(rule) = &entry.custom else {
            if let Some(label) = label {
                self.plain(cell, label.to_string());
            }
            return;
        };
        let name = label.map_or_else(|| fallback(cell), str::to_string);
        if data_type(rule.repr).is_none() || !contiguous(rule, cell.1) {
            self.plain(cell, name);
        } else if rule.repr != CustomRepr::Ascii
            && rule
                .bits
                .iter()
                .any(|entry| usize::from(entry.bit / WORD_BITS) < rule.register_count())
        {
            self.with_bits(cell, label, rule);
        } else {
            self.rule(cell, name, rule);
        }
    }

    fn with_bits(&mut self, cell: RegisterCell, label: Option<&str>, rule: &CustomRule) {
        let order = rule.order(self.device);
        let count = rule.register_count();
        let flip = if order.swaps_bytes() { 8 } else { 0 };
        let bits: Vec<(u16, u8, String)> = rule
            .bits
            .iter()
            .filter(|entry| usize::from(entry.bit / WORD_BITS) < count)
            .filter_map(|entry| {
                let word = usize::from(entry.bit / WORD_BITS);
                let offset = if swaps_words(order) {
                    word
                } else {
                    count - 1 - word
                };
                let address = cell.1.checked_add(u16::try_from(offset).ok()?)?;
                let name =
                    named(&entry.name).map_or_else(|| format!("Bit {}", entry.bit), str::to_string);
                Some((address, (entry.bit % WORD_BITS) ^ flip, name))
            })
            .collect();
        let joined = bits
            .iter()
            .filter(|(address, _, _)| *address == cell.1)
            .map(|(_, _, name)| name.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        let name = label.and_then(|label| {
            if label == joined {
                return None;
            }
            let rest = label
                .strip_suffix(joined.as_str())
                .and_then(|rest| rest.strip_suffix("; "))
                .unwrap_or(label);
            named(rest)
        });
        let bare = rule.repr == CustomRepr::U16
            && rule.scale.is_none()
            && rule.ops.is_empty()
            && rule.enum_map.is_empty()
            && named(&rule.suffix).is_none()
            && rule.word_order.is_none_or(|order| order == self.device);
        if name.is_some() || !bare {
            let name = name.map_or_else(|| fallback(cell), str::to_string);
            self.rule(cell, name, rule);
        }
        for (address, bit, name) in bits {
            let at = (cell.0, address);
            let mut register = Register::new(name, at, "bool");
            register.bit = Some(bit);
            register.access = self.access(cell.0);
            self.bits.push((at, register));
        }
    }

    fn rule(&mut self, cell: RegisterCell, name: String, rule: &CustomRule) {
        let Some(kind) = data_type(rule.repr) else {
            return self.plain(cell, name);
        };
        let text = rule.repr == CustomRepr::Ascii;
        let unit = named(&rule.suffix).map(|suffix| suffix.trim().to_string());
        let stripped = unit
            .as_deref()
            .and_then(|unit| name.strip_suffix(&format!(" [{unit}]")))
            .and_then(named)
            .map(str::to_string);
        let name = stripped.unwrap_or(name);
        let mut register = Register::new(name, cell, kind);
        let order = self.flatten(rule.order(self.device), rule.register_count());
        register.orient(order, text, self.device);
        register.unit = unit;
        register.access = self.access(cell.0);
        let mut need = None;
        if text {
            register.length = Some(rule.register_count());
        } else {
            if let Some((scale, offset)) = affine(&rule.ops) {
                register.scale = Some(scale).filter(|&v| (v - 1.0).abs() > f64::EPSILON);
                register.offset = Some(offset).filter(|&v| v != 0.0);
            }
            for entry in &rule.enum_map {
                if let Some(text) = named(&entry.text) {
                    register
                        .labels
                        .entry(entry.value)
                        .or_insert_with(|| text.to_string());
                }
            }
            need = rule
                .scale()
                .filter(|scale| scale.mode == ScaleMode::Pow10)
                .and_then(|scale| {
                    Some(Need {
                        rank: rank(cell.0),
                        address: scale.register,
                        data_type: exponent_type(scale.repr)?,
                        order: self.flatten(
                            scale.word_order.or(rule.word_order).unwrap_or(self.device),
                            scale.repr.register_count(),
                        ),
                    })
                });
        }
        let exponent = (need.is_none() && INTEGERS.contains(&kind)).then_some((kind, order));
        self.own.push(Own {
            cell,
            register,
            retypable: false,
            exponent,
            need,
        });
    }

    fn resolve(&mut self) {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for register in self
            .own
            .iter()
            .map(|own| &own.register)
            .chain(self.bits.iter().map(|(_, register)| register))
        {
            *counts.entry(register.name.clone()).or_default() += 1;
        }
        let mut taken: BTreeSet<String> = counts.keys().cloned().collect();
        let mut dedicated: Vec<(Need, String)> = Vec::new();
        for i in 0..self.own.len() {
            let Some(need) = self.own[i]
                .need
                .filter(|need| fits(need.address, width(need.data_type)))
            else {
                continue;
            };
            let found = self.own.iter().position(|own| {
                rank(own.cell.0) == need.rank
                    && own.cell.1 == need.address
                    && own.need.is_none()
                    && counts.get(&own.register.name) == Some(&1)
                    && (own.retypable || own.exponent == Some((need.data_type, need.order)))
            });
            let name = match found {
                Some(j) => {
                    let device = self.device;
                    let own = &mut self.own[j];
                    if own.retypable {
                        own.retypable = false;
                        own.exponent = Some((need.data_type, need.order));
                        own.register.data_type = need.data_type;
                        own.register.orient(need.order, false, device);
                    }
                    own.register.name.clone()
                }
                None => self.dedicated(need, &mut taken, &mut dedicated),
            };
            self.own[i].register.scale_factor = Some(name);
        }
    }

    fn dedicated(
        &mut self,
        need: Need,
        taken: &mut BTreeSet<String>,
        dedicated: &mut Vec<(Need, String)>,
    ) -> String {
        if let Some((_, name)) = dedicated.iter().find(|(known, _)| *known == need) {
            return name.clone();
        }
        let kind = RegisterType::ALL[usize::from(need.rank)];
        let cell = (kind, need.address);
        let base = format!("{} SF", fallback(cell));
        let name = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base} {n}"),
            })
            .find(|name| !taken.contains(name))
            .unwrap_or(base);
        taken.insert(name.clone());
        dedicated.push((need, name.clone()));
        let mut register = Register::new(name.clone(), cell, need.data_type);
        register.orient(need.order, false, self.device);
        self.extra.push((cell, register));
        name
    }

    fn registers(self) -> Vec<Register> {
        let mut rows: Vec<(u8, u16, u8, Register)> = self
            .extra
            .into_iter()
            .map(|(cell, register)| (cell, 0, register))
            .chain(self.own.into_iter().map(|own| (own.cell, 1, own.register)))
            .chain(
                self.bits
                    .into_iter()
                    .map(|(cell, register)| (cell, 2, register)),
            )
            .map(|((kind, address), order, register)| (rank(kind), address, order, register))
            .collect();
        rows.sort_by_key(|&(kind, address, order, _)| (kind, address, order));
        rows.into_iter()
            .map(|(_, _, _, register)| register)
            .collect()
    }
}

fn connection(map: &mut DeviceMap, interface: &Interface) {
    match interface {
        Interface::Tcp(params) => {
            map.protocol = Some("tcp");
            map.port = Some(params.port).filter(|&port| port > 0);
        }
        Interface::RtuOverTcp(_) => map.protocol = Some("rtu"),
        Interface::Serial(params) => {
            map.protocol = Some("rtu");
            map.baudrate = Some(params.baud_rate).filter(|&baud| baud > 0);
            map.parity = Some(match params.parity {
                Parity::None => "N",
                Parity::Odd => "O",
                Parity::Even => "E",
            });
            map.databits = match params.data_bits {
                DataBits::Seven => Some(7),
                DataBits::Eight => Some(8),
                DataBits::Five | DataBits::Six => None,
            };
            map.stopbits = Some(u8::from(params.stop_bits));
        }
        Interface::Mock => {}
    }
}

pub fn export(config: &Config) -> String {
    let mut export = Export::new(config);
    for (cell, entry) in config.registers.entries() {
        export.entry(cell, entry);
    }
    export.resolve();
    let order = config.device.word_order;
    let mut map = DeviceMap {
        schema: SCHEMA,
        mbmap: VERSION,
        vendor: VENDOR,
        model: named(&config.name).map_or_else(|| MODEL.to_string(), str::to_string),
        description: DESCRIPTION,
        protocol: None,
        unit_id: config.device.unit_id,
        port: None,
        baudrate: None,
        parity: None,
        databits: None,
        stopbits: None,
        byte_order: side(order.swaps_bytes()),
        word_order: side(swaps_words(order)),
        address_base: 0,
        registers: export.registers(),
    };
    connection(&mut map, &config.device.interface);
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, Layout::default());
    if map.serialize(&mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(out).map_or_else(|_| String::new(), |text| text + "\n")
}

#[cfg(test)]
mod tests {
    use super::{export, oriented, swaps_words};
    use crate::config::{Config, RegisterEntry, Registers};
    use crate::custom::{
        BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry, OpKind, ScaleMode,
        ScaleRepr,
    };
    use crate::import::mbmap::{firmware, parse, revision, status};
    use crate::modbus::{
        DataBits, DeviceConfig, Interface, InterfaceSerialParams, InterfaceTcpParams, Parity,
        StopBits, WordOrder,
    };
    use crate::register::RegisterType;
    use serde_json::{Value, json};

    fn entry(address: u16, label: Option<&str>, custom: Option<CustomRule>) -> RegisterEntry {
        RegisterEntry {
            address,
            label: label.map(str::to_string),
            pinned: true,
            custom: custom.map(|rule| CustomRule { address, ..rule }),
        }
    }

    fn rule(repr: CustomRepr) -> CustomRule {
        CustomRule {
            repr,
            ..Default::default()
        }
    }

    fn op(op: OpKind, v: f64) -> CustomOp {
        CustomOp { op, v }
    }

    fn config(order: WordOrder, registers: Registers) -> Config {
        Config {
            name: "Meter".to_string(),
            device: DeviceConfig {
                word_order: order,
                ..Default::default()
            },
            registers,
            ..Default::default()
        }
    }

    fn holdings(entries: Vec<RegisterEntry>) -> Registers {
        Registers {
            holdings: entries,
            ..Default::default()
        }
    }

    fn round_trip(config: &Config) -> Registers {
        parse(&export(config)).unwrap()
    }

    fn map(config: &Config) -> Value {
        serde_json::from_str(&export(config)).unwrap()
    }

    fn find(registers: &Registers, kind: RegisterType, address: u16) -> &RegisterEntry {
        registers
            .entries()
            .find(|&((k, a), _)| k == kind && a == address)
            .map(|(_, entry)| entry)
            .unwrap()
    }

    fn holding(registers: &Registers, address: u16) -> &RegisterEntry {
        find(registers, RegisterType::Holding, address)
    }

    fn custom(registers: &Registers, address: u16) -> &CustomRule {
        holding(registers, address).custom.as_ref().unwrap()
    }

    fn label(registers: &Registers, address: u16) -> &str {
        holding(registers, address).label.as_deref().unwrap()
    }

    fn normal(rule: &CustomRule, address: u16, device: WordOrder) -> CustomRule {
        let mut rule = rule.clone();
        rule.address = address;
        let flat = |order: WordOrder, count: usize| {
            if count == 1 {
                oriented(order.swaps_bytes(), swaps_words(device))
            } else {
                order
            }
        };
        let order = rule.order(device);
        if let Some(scale) = rule.scale.as_mut() {
            scale.word_order = Some(flat(
                scale.word_order.or(rule.word_order).unwrap_or(device),
                scale.repr.register_count(),
            ));
        }
        rule.word_order = Some(match rule.repr {
            CustomRepr::Ascii if order.swaps_bytes() => WordOrder::BADC,
            CustomRepr::Ascii => WordOrder::ABCD,
            _ => flat(order, rule.register_count()),
        });
        if rule.repr == CustomRepr::Ascii {
            rule.registers = u8::try_from(rule.register_count()).ok();
        }
        rule.enum_map.sort_by_key(|entry| entry.value);
        rule
    }

    fn registers_of(value: &Value) -> &Vec<Value> {
        value["registers"].as_array().unwrap()
    }

    fn named<'a>(value: &'a Value, name: &str) -> &'a Value {
        registers_of(value)
            .iter()
            .find(|register| register["name"] == name)
            .unwrap()
    }

    #[test]
    fn labels_and_rules_the_format_expresses_round_trip() {
        let device = WordOrder::CDAB;
        let enums = [(10, "Manual"), (2, "Auto"), (0, "Off")]
            .map(|(value, text)| EnumEntry {
                value,
                text: text.to_string(),
            })
            .to_vec();
        let registers = Registers {
            holdings: vec![
                entry(
                    0,
                    Some("Energy [kWh]"),
                    Some(CustomRule {
                        ops: vec![op(OpKind::Mul, 0.1)],
                        suffix: " kWh".to_string(),
                        ..rule(CustomRepr::U32)
                    }),
                ),
                entry(
                    2,
                    Some("Temp [°C]"),
                    Some(CustomRule {
                        ops: vec![op(OpKind::Add, -40.0)],
                        suffix: " °C".to_string(),
                        word_order: Some(WordOrder::DCBA),
                        ..rule(CustomRepr::I16)
                    }),
                ),
                entry(
                    3,
                    Some("Mode"),
                    Some(CustomRule {
                        enum_map: enums,
                        ..rule(CustomRepr::U16)
                    }),
                ),
                entry(
                    4,
                    Some("Ratio"),
                    Some(CustomRule {
                        word_order: Some(WordOrder::ABCD),
                        ..rule(CustomRepr::F32)
                    }),
                ),
                entry(
                    6,
                    Some("Serial"),
                    Some(CustomRule {
                        registers: Some(3),
                        ..rule(CustomRepr::Ascii)
                    }),
                ),
                entry(
                    9,
                    Some("Name"),
                    Some(CustomRule {
                        registers: Some(2),
                        word_order: Some(WordOrder::BADC),
                        ..rule(CustomRepr::Ascii)
                    }),
                ),
                entry(
                    11,
                    Some("Total"),
                    Some(CustomRule {
                        ops: vec![op(OpKind::Mul, 2.0), op(OpKind::Add, 1.0)],
                        ..rule(CustomRepr::I64)
                    }),
                ),
                entry(
                    15,
                    Some("Big"),
                    Some(CustomRule {
                        word_order: Some(WordOrder::DCBA),
                        ..rule(CustomRepr::F64)
                    }),
                ),
            ],
            inputs: vec![entry(
                0,
                Some("Voltage [V]"),
                Some(CustomRule {
                    suffix: " V".to_string(),
                    ..rule(CustomRepr::F32)
                }),
            )],
            coils: vec![entry(0, Some("Relay"), None)],
            discretes: vec![entry(1, Some("Door"), None)],
        };
        let original = config(device, registers);
        let back = round_trip(&original);

        assert_eq!(back.entries().count(), original.registers.entries().count());
        for ((kind, address), before) in original.registers.entries() {
            let after = find(&back, kind, address);
            assert_eq!(after.label, before.label, "{kind:?} {address}");
            assert_eq!(
                after.custom.as_ref().map(|r| normal(r, address, device)),
                before.custom.as_ref().map(|r| normal(r, address, device)),
                "{kind:?} {address}"
            );
        }
    }

    #[test]
    fn a_scale_factor_names_the_exponent_register_it_points_at() {
        let device = WordOrder::CDAB;
        let original = config(
            device,
            holdings(vec![
                entry(
                    11,
                    Some("AC power [W]"),
                    Some(CustomRule {
                        suffix: " W".to_string(),
                        scale: Some(CustomScale {
                            register: 12,
                            mode: ScaleMode::Pow10,
                            repr: ScaleRepr::Sunspec,
                            word_order: None,
                        }),
                        ..rule(CustomRepr::I16)
                    }),
                ),
                entry(12, Some("W_SF"), None),
                entry(
                    20,
                    Some("Energy"),
                    Some(CustomRule {
                        scale: Some(CustomScale {
                            register: 22,
                            mode: ScaleMode::Pow10,
                            repr: ScaleRepr::Plain(CustomRepr::U32),
                            word_order: Some(WordOrder::ABCD),
                        }),
                        ..rule(CustomRepr::U32)
                    }),
                ),
                entry(
                    22,
                    Some("E_SF"),
                    Some(CustomRule {
                        word_order: Some(WordOrder::ABCD),
                        ..rule(CustomRepr::U32)
                    }),
                ),
            ]),
        );
        let value = map(&original);
        assert_eq!(named(&value, "AC power")["scale_factor"], "W_SF");
        assert_eq!(named(&value, "W_SF")["type"], "int16");
        assert_eq!(named(&value, "Energy")["scale_factor"], "E_SF");
        assert_eq!(registers_of(&value).len(), 4);

        let back = round_trip(&original);
        for address in [11, 20, 22] {
            assert_eq!(
                normal(custom(&back, address), address, device),
                normal(custom(&original.registers, address), address, device),
            );
        }
        assert_eq!(label(&back, 12), "W_SF");
        assert_eq!(custom(&back, 12).repr, CustomRepr::I16);
    }

    #[test]
    fn exponents_outside_the_map_get_a_register_of_their_own_or_are_dropped() {
        let scaled = |register: u16, mode: ScaleMode, repr: ScaleRepr| {
            Some(CustomRule {
                scale: Some(CustomScale {
                    register,
                    mode,
                    repr,
                    word_order: None,
                }),
                ..rule(CustomRepr::U16)
            })
        };
        let original = config(
            WordOrder::ABCD,
            holdings(vec![
                entry(
                    0,
                    Some("P"),
                    scaled(5, ScaleMode::Pow10, ScaleRepr::Plain(CustomRepr::I16)),
                ),
                entry(
                    1,
                    Some("Q"),
                    scaled(6, ScaleMode::Mul, ScaleRepr::Plain(CustomRepr::U16)),
                ),
                entry(
                    2,
                    Some("R"),
                    scaled(7, ScaleMode::Pow10, ScaleRepr::Plain(CustomRepr::F32)),
                ),
                entry(
                    3,
                    Some("S"),
                    scaled(8, ScaleMode::Pow10, ScaleRepr::Plain(CustomRepr::U16)),
                ),
                entry(8, Some("Dup"), None),
                entry(9, Some("Dup"), None),
            ]),
        );
        let value = map(&original);
        assert_eq!(named(&value, "P")["scale_factor"], "Holding 5 SF");
        assert_eq!(named(&value, "Holding 5 SF")["type"], "int16");
        assert_eq!(named(&value, "S")["scale_factor"], "Holding 8 SF");

        let back = round_trip(&original);
        let exponent = custom(&back, 0).scale.unwrap();
        assert_eq!((exponent.register, exponent.repr), (5, ScaleRepr::Sunspec));
        assert_eq!(label(&back, 5), "Holding 5 SF");
        assert_eq!(custom(&back, 1).scale, None);
        assert_eq!(custom(&back, 2).scale, None);
        assert_eq!(
            custom(&back, 3)
                .scale
                .map(|scale| (scale.register, scale.repr)),
            Some((8, ScaleRepr::Plain(CustomRepr::U16)))
        );
        assert_eq!(label(&back, 8), "Holding 8 SF; Dup");
        assert_eq!(label(&back, 9), "Dup");
    }

    fn bits(entries: &[(u8, &str)]) -> Vec<BitEntry> {
        entries
            .iter()
            .map(|&(bit, name)| BitEntry {
                bit,
                name: name.to_string(),
            })
            .collect()
    }

    fn bools(value: &Value) -> Vec<(u64, u64, String)> {
        registers_of(value)
            .iter()
            .filter(|register| register["type"] == "bool")
            .map(|register| {
                (
                    register["address"].as_u64().unwrap(),
                    register["bit"].as_u64().unwrap(),
                    register["name"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn bit_names_become_bool_registers_of_the_word() {
        let device = WordOrder::ABCD;
        let original = config(
            device,
            holdings(vec![
                entry(
                    0,
                    Some("Relay; Alarm"),
                    Some(CustomRule {
                        bits: bits(&[(0, "Relay"), (3, "Alarm")]),
                        ..Default::default()
                    }),
                ),
                entry(
                    1,
                    Some("Status"),
                    Some(CustomRule {
                        bits: bits(&[(1, "Run"), (20, "High"), (40, "Gone")]),
                        ..rule(CustomRepr::U32)
                    }),
                ),
            ]),
        );
        let value = map(&original);
        let at = |address: u16| {
            registers_of(&value)
                .iter()
                .filter(|register| register["address"] == address)
                .map(|register| (register["name"].clone(), register["type"].clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            at(0),
            [
                (json!("Relay"), json!("bool")),
                (json!("Alarm"), json!("bool"))
            ]
        );
        assert_eq!(
            at(1),
            [
                (json!("Status"), json!("uint32")),
                (json!("High"), json!("bool"))
            ]
        );
        assert_eq!(at(2), [(json!("Run"), json!("bool"))]);
        assert_eq!(
            bools(&value),
            [
                (0, 0, "Relay".to_string()),
                (0, 3, "Alarm".to_string()),
                (1, 4, "High".to_string()),
                (2, 1, "Run".to_string())
            ]
        );

        let back = round_trip(&original);
        assert_eq!(label(&back, 0), "Relay; Alarm");
        assert_eq!(
            normal(custom(&back, 0), 0, device),
            normal(custom(&original.registers, 0), 0, device)
        );
        assert_eq!(custom(&back, 1).repr, CustomRepr::U32);
    }

    #[test]
    fn bools_point_at_the_wire_bit_the_rule_decodes() {
        let cases: [(CustomRepr, &[(u8, &str)]); 3] = [
            (CustomRepr::U16, &[(0, "A"), (9, "B"), (15, "C")]),
            (
                CustomRepr::U32,
                &[(0, "Low0"), (12, "Low12"), (16, "High0"), (31, "Top")],
            ),
            (
                CustomRepr::U64,
                &[(5, "W0"), (17, "W1"), (40, "W2"), (63, "W3")],
            ),
        ];
        for device in WordOrder::ALL {
            for own in [None, Some(WordOrder::BADC), Some(WordOrder::CDAB)] {
                for (repr, names) in cases {
                    let rule = CustomRule {
                        address: 10,
                        bits: bits(names),
                        word_order: own,
                        ..rule(repr)
                    };
                    let value = map(&config(
                        device,
                        holdings(vec![entry(10, Some("Status"), Some(rule.clone()))]),
                    ));
                    let rows = bools(&value);
                    assert_eq!(rows.len(), names.len(), "{device:?} {own:?} {repr:?}");
                    for (address, bit, name) in rows {
                        let mut words = vec![0u16; repr.register_count()];
                        words[usize::try_from(address - 10).unwrap()] = 1 << bit;
                        assert_eq!(
                            rule.evaluate(&words, device),
                            name,
                            "{device:?} {own:?} {repr:?} {address} {bit}"
                        );
                    }
                }
            }
        }

        let run = config(
            WordOrder::BADC,
            holdings(vec![entry(
                0,
                None,
                Some(CustomRule {
                    bits: bits(&[(0, "Run")]),
                    ..Default::default()
                }),
            )]),
        );
        assert_eq!(bools(&map(&run)), [(0, 8, "Run".to_string())]);

        let status = config(
            WordOrder::ABCD,
            holdings(vec![entry(
                10,
                Some("Status"),
                Some(CustomRule {
                    bits: bits(&[(0, "Low0"), (16, "High0")]),
                    ..rule(CustomRepr::U32)
                }),
            )]),
        );
        assert_eq!(
            bools(&map(&status)),
            [(10, 0, "High0".to_string()), (11, 0, "Low0".to_string())]
        );
    }

    #[test]
    fn values_past_the_last_address_degrade_and_their_exponents_are_dropped() {
        let scaled = |register: u16, repr: ScaleRepr| {
            Some(CustomRule {
                scale: Some(CustomScale {
                    register,
                    mode: ScaleMode::Pow10,
                    repr,
                    word_order: None,
                }),
                ..rule(CustomRepr::U16)
            })
        };
        let device = WordOrder::ABCD;
        let original = config(
            device,
            Registers {
                holdings: vec![
                    entry(
                        100,
                        Some("P"),
                        scaled(65534, ScaleRepr::Plain(CustomRepr::U64)),
                    ),
                    entry(
                        101,
                        Some("Q"),
                        scaled(65535, ScaleRepr::Plain(CustomRepr::U32)),
                    ),
                    entry(
                        102,
                        Some("R"),
                        scaled(65535, ScaleRepr::Plain(CustomRepr::U16)),
                    ),
                    entry(
                        65530,
                        Some("Str"),
                        Some(CustomRule {
                            registers: Some(7),
                            ..rule(CustomRepr::Ascii)
                        }),
                    ),
                    entry(65535, Some("Last"), None),
                ],
                inputs: vec![
                    entry(65532, Some("Edge"), Some(rule(CustomRepr::U32))),
                    entry(65534, Some("Fits"), Some(rule(CustomRepr::U32))),
                    entry(65535, Some("Wrap32"), Some(rule(CustomRepr::U32))),
                ],
                ..Default::default()
            },
        );
        let value = map(&original);
        for register in registers_of(&value) {
            let count = register["length"].as_u64().unwrap_or_else(|| {
                match register["type"].as_str().unwrap() {
                    "uint32" | "int32" | "float32" => 2,
                    "uint64" | "int64" | "float64" => 4,
                    _ => 1,
                }
            });
            assert!(
                register["address"].as_u64().unwrap() + count <= 65536,
                "{register}"
            );
        }
        assert_eq!(named(&value, "Wrap32")["type"], "uint16");
        assert_eq!(named(&value, "Str")["type"], "uint16");
        assert!(named(&value, "Str").get("length").is_none());
        assert_eq!(named(&value, "Fits")["type"], "uint32");
        assert!(named(&value, "P").get("scale_factor").is_none());
        assert!(named(&value, "Q").get("scale_factor").is_none());
        assert_eq!(named(&value, "R")["scale_factor"], "Last");
        assert_eq!(named(&value, "Last")["type"], "uint16");
        assert_eq!(registers_of(&value).len(), 8);

        let back = round_trip(&original);
        assert_eq!(custom(&back, 100).scale, None);
        assert_eq!(custom(&back, 101).scale, None);
        assert_eq!(
            custom(&back, 102)
                .scale
                .map(|scale| (scale.register, scale.repr)),
            Some((65535, ScaleRepr::Plain(CustomRepr::U16)))
        );
        assert_eq!(label(&back, 65530), "Str");
        assert_eq!(custom(&back, 65530).repr, CustomRepr::U16);
        let wrap = find(&back, RegisterType::Input, 65535);
        assert_eq!(wrap.label.as_deref(), Some("Wrap32"));
        assert_eq!(wrap.custom.as_ref().unwrap().repr, CustomRepr::U16);
        let fits = find(&back, RegisterType::Input, 65534);
        assert_eq!(fits.custom.as_ref().unwrap().repr, CustomRepr::U32);
    }

    #[test]
    fn a_sixteen_bit_exponent_matches_regardless_of_word_order() {
        let device = WordOrder::CDAB;
        let power = |word_order: Option<WordOrder>| {
            entry(
                11,
                Some("P"),
                Some(CustomRule {
                    scale: Some(CustomScale {
                        register: 12,
                        mode: ScaleMode::Pow10,
                        repr: ScaleRepr::Sunspec,
                        word_order,
                    }),
                    ..rule(CustomRepr::I16)
                }),
            )
        };
        let exponent = entry(
            12,
            Some("W_SF"),
            Some(CustomRule {
                word_order: Some(WordOrder::ABCD),
                ..rule(CustomRepr::I16)
            }),
        );
        let original = config(device, holdings(vec![power(None), exponent.clone()]));
        let value = map(&original);
        assert_eq!(named(&value, "P")["scale_factor"], "W_SF");
        assert!(named(&value, "W_SF").get("word_order").is_none());
        assert!(named(&value, "W_SF").get("byte_order").is_none());
        assert_eq!(registers_of(&value).len(), 2);

        let back = round_trip(&original);
        assert_eq!(label(&back, 12), "W_SF");
        assert_eq!(custom(&back, 12).word_order, Some(device));
        for address in [11, 12] {
            assert_eq!(
                normal(custom(&back, address), address, device),
                normal(custom(&original.registers, address), address, device),
            );
        }

        let swapped = config(
            device,
            holdings(vec![power(Some(WordOrder::BADC)), exponent]),
        );
        let value = map(&swapped);
        assert_eq!(named(&value, "P")["scale_factor"], "Holding 12 SF");
        assert_eq!(named(&value, "Holding 12 SF")["byte_order"], "little");
        assert!(named(&value, "Holding 12 SF").get("word_order").is_none());
        let back = round_trip(&swapped);
        assert_eq!(label(&back, 12), "Holding 12 SF; W_SF");
        assert_eq!(
            normal(custom(&back, 11), 11, device),
            normal(custom(&swapped.registers, 11), 11, device),
        );
    }

    #[test]
    fn ops_fold_into_one_scale_and_one_offset() {
        let ops = |list: Vec<CustomOp>| {
            let original = config(
                WordOrder::ABCD,
                holdings(vec![entry(
                    0,
                    Some("V"),
                    Some(CustomRule {
                        ops: list,
                        ..rule(CustomRepr::U16)
                    }),
                )]),
            );
            custom(&round_trip(&original), 0).ops.clone()
        };
        assert_eq!(
            ops(vec![op(OpKind::Add, 5.0), op(OpKind::Mul, 2.0)]),
            [op(OpKind::Mul, 2.0), op(OpKind::Add, 10.0)]
        );
        assert_eq!(
            ops(vec![op(OpKind::Div, 4.0), op(OpKind::Sub, 1.5)]),
            [op(OpKind::Mul, 0.25), op(OpKind::Add, -1.5)]
        );
        assert_eq!(ops(vec![op(OpKind::Mul, 1.0), op(OpKind::Pow, 1.0)]), []);
        assert_eq!(ops(vec![op(OpKind::Pow, 2.0)]), []);
        assert_eq!(ops(vec![op(OpKind::Div, 0.0)]), []);
    }

    #[test]
    fn what_the_format_cannot_hold_degrades_to_a_labelled_word() {
        let device = WordOrder::ABCD;
        let original = config(
            device,
            holdings(vec![
                entry(0, Some("Half"), Some(rule(CustomRepr::F16))),
                entry(1, Some("Wide"), Some(rule(CustomRepr::U128))),
                entry(
                    10,
                    Some("Split"),
                    Some(CustomRule {
                        next: vec![20],
                        ..rule(CustomRepr::U32)
                    }),
                ),
                entry(
                    12,
                    Some("Joined"),
                    Some(CustomRule {
                        next: vec![13],
                        ..rule(CustomRepr::U32)
                    }),
                ),
                entry(
                    14,
                    Some("E"),
                    Some(CustomRule {
                        prefix: "~".to_string(),
                        suffix: "kWh".to_string(),
                        decimals: Some(2),
                        ..rule(CustomRepr::U16)
                    }),
                ),
                entry(30, None, Some(rule(CustomRepr::I16))),
                entry(40, None, None),
                entry(50, Some("Note"), None),
            ]),
        );
        let back = round_trip(&original);
        let plain = |address: u16| CustomRule {
            address,
            word_order: Some(device),
            ..rule(CustomRepr::U16)
        };
        for (address, text) in [(0, "Half"), (1, "Wide"), (10, "Split"), (50, "Note")] {
            assert_eq!(label(&back, address), text);
            assert_eq!(custom(&back, address), &plain(address));
        }
        assert_eq!(
            custom(&back, 12),
            &CustomRule {
                address: 12,
                word_order: Some(device),
                ..rule(CustomRepr::U32)
            }
        );
        let text = custom(&back, 14);
        assert_eq!(label(&back, 14), "E [kWh]");
        assert_eq!(
            (text.prefix.as_str(), text.suffix.as_str(), text.decimals),
            ("", " kWh", None)
        );
        assert_eq!(label(&back, 30), "Holding 30");
        assert_eq!(custom(&back, 30).repr, CustomRepr::I16);
        assert!(back.entries().all(|((_, address), _)| address != 40));
    }

    #[test]
    fn the_header_describes_the_device_and_each_register_takes_one_line() {
        let mut original = config(
            WordOrder::DCBA,
            Registers {
                holdings: vec![entry(0, Some("H"), Some(rule(CustomRepr::U16)))],
                inputs: vec![entry(0, Some("I"), None)],
                coils: vec![entry(0, Some("C"), None)],
                discretes: vec![entry(0, Some("D"), None)],
            },
        );
        original.device.unit_id = 7;
        original.device.interface = Interface::Tcp(InterfaceTcpParams {
            ip: "10.0.0.2".to_string(),
            port: 1502,
        });
        let text = export(&original);
        assert_eq!(text, export(&original));
        assert!(text.ends_with("}\n"));
        let value: Value = serde_json::from_str(&text).unwrap();
        for (key, expected) in [
            ("mbmap", json!(2)),
            ("vendor", json!("Unknown")),
            ("model", json!("Meter")),
            ("protocol", json!("tcp")),
            ("unit_id", json!(7)),
            ("port", json!(1502)),
            ("byte_order", json!("little")),
            ("word_order", json!("little")),
            ("address_base", json!(0)),
        ] {
            assert_eq!(value[key], expected, "{key}");
        }
        assert!(value.get("baudrate").is_none());
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("    {\"name\": "))
            .collect();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("\"area\": \"holding\", \"address\": 0, \"type\": \"uint16\""));
        let access: Vec<&Value> = registers_of(&value).iter().map(|r| &r["access"]).collect();
        assert_eq!(
            access,
            [&json!("RW"), &Value::Null, &json!("RW"), &Value::Null]
        );
        assert_eq!(
            (revision(&text), status(&text), firmware(&text)),
            (None, None, None)
        );

        original.read_only = true;
        original.device.interface = Interface::Serial(InterfaceSerialParams {
            path: "/dev/ttyUSB0".to_string(),
            baud_rate: 19200,
            data_bits: DataBits::Eight,
            parity: Parity::Even,
            stop_bits: StopBits::Two,
        });
        let value = map(&original);
        for (key, expected) in [
            ("protocol", json!("rtu")),
            ("baudrate", json!(19200)),
            ("parity", json!("E")),
            ("databits", json!(8)),
            ("stopbits", json!(2)),
        ] {
            assert_eq!(value[key], expected, "{key}");
        }
        assert!(value.get("port").is_none());
        assert!(
            registers_of(&value)
                .iter()
                .all(|r| r.get("access").is_none())
        );

        let empty = Config {
            name: " ".to_string(),
            ..Default::default()
        };
        let value = map(&empty);
        assert_eq!(value["model"], "MTUI export");
        assert!(value.get("protocol").is_none());
        assert!(registers_of(&value).is_empty());
        assert!(parse(&export(&empty)).is_none());
    }
}
