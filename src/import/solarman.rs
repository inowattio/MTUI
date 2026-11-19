use super::Device;
use crate::config::Registers;
use crate::custom::{
    ASCII_REGISTERS_MAX, BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry,
    OpKind, ScaleMode, ScaleRepr,
};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::de::{IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_norway::{Mapping, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::slice;

const DEFAULT_CODE: u8 = 0x03;
const VARIANT_KEYS: [&str; 5] = ["scale", "min", "max", "default", "step"];

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(deserializer).map(|_| true)
}

struct Lenient(Value);

impl<'de> Deserialize<'de> for Lenient {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(LenientVisitor).map(Self)
    }
}

struct LenientVisitor;

impl<'de> Visitor<'de> for LenientVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a YAML value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }

    fn visit_str<E>(self, value: &str) -> Result<Value, E> {
        Ok(Value::String(value.to_string()))
    }

    fn visit_string<E>(self, value: String) -> Result<Value, E> {
        Ok(Value::String(value))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Lenient::deserialize(deserializer).map(|Lenient(value)| value)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut list = Vec::new();
        while let Some(Lenient(value)) = seq.next_element()? {
            list.push(value);
        }
        Ok(Value::Sequence(list))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut mapping = Mapping::new();
        while let Some((Lenient(key), Lenient(value))) = map.next_entry()? {
            mapping.insert(key, value);
        }
        Ok(Value::Mapping(mapping))
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Sequence(list)) => !list.is_empty(),
        Some(Value::Mapping(map)) => !map.is_empty(),
        Some(Value::Tagged(tagged)) => truthy(Some(&tagged.value)),
        Some(Value::Null) | None => false,
    }
}

fn function_code(value: &Value) -> Option<u8> {
    let code = match value {
        Value::Mapping(map) => map.get("read").or_else(|| map.get("write"))?,
        number => number,
    };
    u8::try_from(code.as_u64()?).ok().filter(|&code| code != 0)
}

const fn table(code: u8) -> RegisterType {
    match code {
        0x01 | 0x05 | 0x0F => RegisterType::Coil,
        0x02 => RegisterType::Discrete,
        0x04 => RegisterType::Input,
        _ => RegisterType::Holding,
    }
}

#[derive(Deserialize)]
struct Profile {
    info: Option<Value>,
    default: Option<Value>,
    requests: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "present")]
    requests_fine_control: bool,
    parameters: Vec<Group>,
}

#[derive(Deserialize)]
struct Group {
    group: Option<Value>,
    code: Option<Value>,
    items: Vec<Value>,
}

#[derive(Deserialize)]
struct Request {
    start: u16,
    end: u16,
    code: Option<Value>,
    mb_functioncode: Option<Value>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Reading {
    registers: Vec<u16>,
    code: Option<Value>,
    scale: Option<f64>,
    offset: Option<f64>,
    divide: Option<f64>,
    mask: Option<u64>,
    bit: Option<u8>,
    bitmask: Option<u64>,
    #[serde(deserialize_with = "present")]
    signed: bool,
    magnitude: Option<Value>,
}

#[derive(Deserialize)]
struct Item {
    name: Option<String>,
    rule: Option<i64>,
    #[serde(flatten)]
    reading: Reading,
    uom: Option<String>,
    unit_of_measurement: Option<String>,
    inverted: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    reverse_bytes: bool,
    #[serde(default, deserialize_with = "present")]
    disabled: bool,
    lookup: Option<Vec<Value>>,
    sensors: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct Sensor {
    #[serde(flatten)]
    reading: Reading,
    operator: Option<String>,
    multiply: Option<Reading>,
}

#[derive(Deserialize)]
struct Choice {
    key: Option<Value>,
    bit: Option<Value>,
    mode: Option<String>,
    value: Value,
}

fn pick(list: &[Value], variant: usize) -> Value {
    list.get(variant)
        .or_else(|| list.last())
        .cloned()
        .unwrap_or(Value::Null)
}

fn nested(list: &[Value]) -> bool {
    list.first().is_some_and(Value::is_sequence)
}

fn variants(value: &Value) -> usize {
    let Value::Mapping(map) = value else {
        return 1;
    };
    map.iter()
        .map(|(key, entry)| {
            let key = key.as_str().unwrap_or_default();
            match entry {
                Value::Sequence(list) if VARIANT_KEYS.contains(&key) => list.len(),
                Value::Sequence(list) if key == "registers" && nested(list) => list.len(),
                Value::Sequence(list) if key == "sensors" => {
                    list.iter().map(variants).max().unwrap_or(1)
                }
                Value::Mapping(_) => variants(entry),
                _ => 1,
            }
        })
        .max()
        .unwrap_or(1)
}

fn specialize(value: &mut Value, variant: usize) {
    let Value::Mapping(map) = value else {
        return;
    };
    for (key, entry) in map.iter_mut() {
        let key = key.as_str().unwrap_or_default();
        match entry {
            Value::Sequence(list) if VARIANT_KEYS.contains(&key) => {
                *entry = pick(list, variant);
            }
            Value::Sequence(list) if key == "registers" && nested(list) => {
                *entry = pick(list, variant);
            }
            Value::Sequence(list) if key == "sensors" => {
                for sensor in list {
                    specialize(sensor, variant);
                }
            }
            Value::Mapping(_) => specialize(entry, variant),
            _ => {}
        }
    }
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

fn steps(registers: &[u16], step: i32) -> bool {
    registers
        .array_windows()
        .all(|&[a, b]| i32::from(b) - i32::from(a) == step)
}

struct Span {
    address: u16,
    next: Vec<u16>,
    order: WordOrder,
}

fn span(registers: &[u16]) -> Option<Span> {
    let (&first, rest) = registers.split_first()?;
    Some(if rest.is_empty() {
        Span {
            address: first,
            next: Vec::new(),
            order: WordOrder::ABCD,
        }
    } else if steps(registers, 1) {
        Span {
            address: first,
            next: Vec::new(),
            order: WordOrder::CDAB,
        }
    } else if steps(registers, -1) {
        Span {
            address: *registers.last()?,
            next: Vec::new(),
            order: WordOrder::ABCD,
        }
    } else {
        Span {
            address: first,
            next: rest.to_vec(),
            order: WordOrder::CDAB,
        }
    })
}

const fn repr(count: usize, signed: bool) -> Option<CustomRepr> {
    Some(match (count, signed) {
        (1, false) => CustomRepr::U16,
        (1, true) => CustomRepr::I16,
        (2, false) => CustomRepr::U32,
        (2, true) => CustomRepr::I32,
        (4, false) => CustomRepr::U64,
        (4, true) => CustomRepr::I64,
        _ => return None,
    })
}

fn single_bit(mask: u64) -> Option<u8> {
    mask.is_power_of_two()
        .then(|| u8::try_from(mask.trailing_zeros()).ok())
        .flatten()
}

enum Selection {
    Whole,
    Bit(u8),
    Unsupported,
}

impl Reading {
    fn selection(&self, width: u32) -> Selection {
        let full = u64::MAX >> 64u32.saturating_sub(width);
        let mut bits = Vec::new();
        if let Some(mask) = self
            .mask
            .map(|mask| mask & full)
            .filter(|&mask| mask != full)
        {
            match single_bit(mask) {
                Some(bit) => bits.push(bit),
                None => return Selection::Unsupported,
            }
        }
        if let Some(bitmask) = self.bitmask {
            match single_bit(bitmask & full) {
                Some(bit) => bits.push(bit),
                None => return Selection::Unsupported,
            }
        }
        bits.extend(self.bit);
        match bits.split_first() {
            None => Selection::Whole,
            Some((&bit, rest))
                if rest.iter().all(|&other| other == bit) && u32::from(bit) < width =>
            {
                Selection::Bit(bit)
            }
            Some(_) => Selection::Unsupported,
        }
    }

    fn sign_magnitude(&self) -> bool {
        truthy(self.magnitude.as_ref())
    }

    const fn selects(&self) -> bool {
        self.mask.is_some() || self.bit.is_some() || self.bitmask.is_some()
    }

    const fn adjusts(&self) -> bool {
        self.offset.is_some() || self.divide.is_some() || self.selects()
    }

    fn ops(&self, scale: Option<f64>) -> Vec<CustomOp> {
        [
            op(OpKind::Sub, self.offset, 0.0),
            op(OpKind::Mul, scale, 1.0),
            op(OpKind::Div, self.divide, 1.0),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

fn op(op: OpKind, value: Option<f64>, neutral: f64) -> Option<CustomOp> {
    value
        .filter(|&v| (v - neutral).abs() > f64::EPSILON)
        .map(|v| CustomOp { op, v })
}

fn numeric_rule(registers: &[u16], signed: bool) -> Option<CustomRule> {
    let repr = repr(registers.len(), signed)?;
    let span = span(registers)?;
    Some(CustomRule {
        address: span.address,
        repr,
        next: span.next,
        word_order: Some(span.order),
        ..Default::default()
    })
}

fn negated(sign: f64) -> Option<CustomOp> {
    (sign < 0.0).then_some(CustomOp {
        op: OpKind::Mul,
        v: -1.0,
    })
}

fn keys(key: &Value) -> Vec<i64> {
    match key {
        Value::Sequence(list) => list.iter().filter_map(Value::as_i64).collect(),
        key => key.as_i64().into_iter().collect(),
    }
}

fn bit_sum(bits: &[Value]) -> Option<i64> {
    bits.iter().try_fold(0i64, |sum, bit| {
        let bit = u32::try_from(bit.as_u64()?).ok().filter(|&bit| bit < 63)?;
        Some(sum | 1 << bit)
    })
}

fn lookup(choices: &[Value], width: u32) -> (Vec<EnumEntry>, Vec<BitEntry>) {
    let mut entries = Vec::new();
    let mut bits = Vec::new();
    for choice in choices {
        let Ok(choice) = Choice::deserialize(choice) else {
            continue;
        };
        let Some(text) = text(&choice.value) else {
            continue;
        };
        let any = choice.mode.as_deref() == Some("|");
        match (&choice.bit, &choice.key) {
            (Some(Value::Sequence(list)), _) => {
                entries.extend(bit_sum(list).map(|value| EnumEntry { value, text }));
            }
            (Some(bit), _) if any => {
                let bit = bit.as_u64().and_then(|bit| u8::try_from(bit).ok());
                if let Some(bit) = bit.filter(|&bit| u32::from(bit) < width) {
                    bits.push(BitEntry { bit, name: text });
                }
            }
            (Some(bit), _) => {
                entries
                    .extend(bit_sum(slice::from_ref(bit)).map(|value| EnumEntry { value, text }));
            }
            (None, Some(key)) => {
                entries.extend(keys(key).into_iter().map(|value| EnumEntry {
                    value,
                    text: text.clone(),
                }));
            }
            (None, None) => {}
        }
    }
    (entries, bits)
}

struct Context<'a> {
    code: u8,
    requests: Vec<Request>,
    groups: &'a [Group],
}

struct Decoded {
    cell: RegisterCell,
    label: String,
    rule: Option<CustomRule>,
    computed: bool,
}

impl Item {
    fn name(&self, group: &Group) -> Option<String> {
        let name = self.name.as_deref()?.trim();
        let name = if name.is_empty() {
            group.group.as_ref().and_then(text)?
        } else {
            name.to_string()
        };
        (!name.trim().is_empty()).then_some(name)
    }

    fn unit(&self) -> Option<&str> {
        self.uom
            .as_deref()
            .or(self.unit_of_measurement.as_deref())
            .map(str::trim)
            .filter(|unit| !unit.is_empty())
    }

    fn label(&self, name: &str) -> String {
        match self.unit() {
            Some(unit) => format!("{name} [{unit}]"),
            None => name.to_string(),
        }
    }

    fn suffix(&self) -> String {
        self.unit().map_or_default(|unit| format!(" {unit}"))
    }

    const fn signed(&self) -> bool {
        matches!(self.rule, Some(2 | 4))
    }

    fn inversion(&self) -> f64 {
        if self.signed() && truthy(self.inverted.as_ref()) {
            -1.0
        } else {
            1.0
        }
    }

    fn rule(&self, name: &str) -> Option<CustomRule> {
        match self.rule? {
            1 | 3 => self.unsigned(name),
            2 | 4 => self.signed_rule(),
            5 => self.ascii(),
            _ => None,
        }
    }

    fn unsigned(&self, name: &str) -> Option<CustomRule> {
        let reading = &self.reading;
        let mut rule = numeric_rule(&reading.registers, false)?;
        let width = u32::try_from(rule.register_count() * 16).ok()?;
        match reading.selection(width) {
            Selection::Unsupported => None,
            Selection::Bit(bit) => {
                rule.bits.push(BitEntry {
                    bit,
                    name: name.to_string(),
                });
                Some(rule)
            }
            Selection::Whole => {
                if let Some(choices) = &self.lookup {
                    (rule.enum_map, rule.bits) = lookup(choices, width);
                } else {
                    rule.ops = reading.ops(reading.scale);
                    rule.suffix = self.suffix();
                }
                Some(rule)
            }
        }
    }

    fn signed_rule(&self) -> Option<CustomRule> {
        let reading = &self.reading;
        if reading.sign_magnitude() {
            return None;
        }
        let mut rule = numeric_rule(&reading.registers, true)?;
        rule.ops = reading.ops(reading.scale);
        rule.ops.extend(negated(self.inversion()));
        rule.suffix = self.suffix();
        Some(rule)
    }

    fn ascii(&self) -> Option<CustomRule> {
        let registers = &self.reading.registers;
        let (&address, rest) = registers.split_first()?;
        let count = u8::try_from(registers.len())
            .ok()
            .filter(|&count| count <= ASCII_REGISTERS_MAX)?;
        Some(CustomRule {
            address,
            repr: CustomRepr::Ascii,
            registers: Some(count),
            next: if steps(registers, 1) {
                Vec::new()
            } else {
                rest.to_vec()
            },
            suffix: self.suffix(),
            word_order: Some(if self.reverse_bytes {
                WordOrder::BADC
            } else {
                WordOrder::ABCD
            }),
            ..Default::default()
        })
    }

    fn computed(&self, sensor: &Sensor, code: u8, alone: bool) -> Option<CustomRule> {
        if !alone || self.lookup.is_some() || !matches!(self.rule, Some(1..=4)) {
            return None;
        }
        let reading = &sensor.reading;
        let sign = match sensor.operator.as_deref() {
            Some("subtract") => -self.inversion(),
            Some("multiply" | "divide") => return None,
            _ => self.inversion(),
        };
        if reading.sign_magnitude() || reading.selects() {
            return None;
        }
        let mut rule = numeric_rule(&reading.registers, reading.signed)?;
        let scale = reading.scale.or(self.reading.scale);
        let Some(factor) = &sensor.multiply else {
            rule.ops = reading.ops(scale);
            rule.ops.extend(negated(sign));
            rule.suffix = self.suffix();
            return Some(rule);
        };
        let factor_code = factor.code.as_ref().and_then(function_code);
        if reading.adjusts() || factor.adjusts() || factor.sign_magnitude() {
            return None;
        }
        if factor_code.is_some_and(|factor_code| table(factor_code) != table(code)) {
            return None;
        }
        let [register] = factor.registers[..] else {
            return None;
        };
        rule.scale = Some(CustomScale {
            register,
            mode: ScaleMode::Mul,
            repr: ScaleRepr::Plain(if factor.signed {
                CustomRepr::I16
            } else {
                CustomRepr::U16
            }),
        });
        rule.ops = [
            op(OpKind::Mul, scale, 1.0),
            op(OpKind::Mul, factor.scale.or(scale), 1.0),
            negated(sign),
        ]
        .into_iter()
        .flatten()
        .collect();
        rule.suffix = self.suffix();
        Some(rule)
    }
}

fn anchor(registers: &[u16]) -> Option<u16> {
    span(registers).map(|span| span.address)
}

impl<'a> Context<'a> {
    fn new(profile: &'a Profile) -> Self {
        let code = profile
            .default
            .as_ref()
            .and_then(|default| default.get("code"))
            .and_then(function_code)
            .unwrap_or(DEFAULT_CODE);
        let requests = match &profile.requests {
            Some(requests) if !profile.requests_fine_control => requests
                .iter()
                .filter_map(|request| Request::deserialize(request).ok())
                .collect(),
            _ => Vec::new(),
        };
        Self {
            code,
            requests,
            groups: &profile.parameters,
        }
    }

    fn requested(&self, address: u16) -> Option<u8> {
        let request = self
            .requests
            .iter()
            .rev()
            .find(|request| (request.start..=request.end).contains(&address))?;
        request
            .code
            .as_ref()
            .or(request.mb_functioncode.as_ref())
            .and_then(function_code)
    }

    fn code(&self, item: &Item, group: &Group, registers: &[u16]) -> u8 {
        item.reading
            .code
            .as_ref()
            .and_then(function_code)
            .or_else(|| group.code.as_ref().and_then(function_code))
            .or_else(|| self.requested(*registers.iter().min()?))
            .unwrap_or(self.code)
    }

    fn decode(&self, group: &Group, value: &Value, variant: usize) -> Option<Decoded> {
        let mut value = value.clone();
        specialize(&mut value, variant);
        let item: Item = serde_norway::from_value(value).ok()?;
        if item.disabled || item.rule.is_none() {
            return None;
        }
        let name = item.name(group)?;
        let label = item.label(&name);
        if let Some(sensors) = item.sensors.as_ref().filter(|sensors| !sensors.is_empty()) {
            let sensors: Vec<Sensor> = sensors
                .iter()
                .filter_map(|sensor| Sensor::deserialize(sensor).ok())
                .filter(|sensor| !sensor.reading.registers.is_empty())
                .collect();
            let first = sensors.first()?;
            let registers: Vec<u16> = sensors
                .iter()
                .flat_map(|sensor| {
                    let factor = sensor.multiply.iter().flat_map(|m| &m.registers);
                    sensor.reading.registers.iter().chain(factor).copied()
                })
                .collect();
            let code = self.code(&item, group, &registers);
            let code = first
                .reading
                .code
                .as_ref()
                .and_then(function_code)
                .unwrap_or(code);
            return Some(Decoded {
                cell: (table(code), anchor(&first.reading.registers)?),
                label,
                rule: item.computed(first, code, sensors.len() == 1),
                computed: true,
            });
        }
        let registers = &item.reading.registers;
        let code = self.code(&item, group, registers);
        Some(Decoded {
            cell: (table(code), anchor(registers)?),
            label,
            rule: item.rule(&name),
            computed: false,
        })
    }

    fn import(&self, variant: usize) -> Import {
        let mut import = Import::default();
        let mut computed = Vec::new();
        for group in self.groups {
            for value in &group.items {
                match self.decode(group, value, variant) {
                    Some(decoded) if decoded.computed => computed.push(decoded),
                    Some(decoded) => import.add(decoded),
                    None => {}
                }
            }
        }
        let direct: BTreeSet<RegisterCell> = import.labels.keys().copied().collect();
        for decoded in computed {
            if !direct.contains(&decoded.cell) {
                import.add(decoded);
            }
        }
        import
    }
}

#[derive(Default, PartialEq)]
struct Import {
    labels: BTreeMap<RegisterCell, String>,
    rules: BTreeMap<RegisterCell, CustomRule>,
    replaced: usize,
}

const fn bits_only(rule: &CustomRule) -> bool {
    !rule.bits.is_empty() && rule.enum_map.is_empty() && rule.ops.is_empty() && rule.scale.is_none()
}

const fn symbolic(rule: &CustomRule) -> bool {
    (!rule.bits.is_empty() || !rule.enum_map.is_empty())
        && rule.ops.is_empty()
        && rule.scale.is_none()
}

fn same_words(a: &CustomRule, b: &CustomRule) -> bool {
    (a.repr, a.word_order, &a.next) == (b.repr, b.word_order, &b.next)
}

impl Import {
    fn label(&mut self, cell: RegisterCell, label: String) {
        match self.labels.get_mut(&cell) {
            Some(previous) if previous.split("; ").any(|part| part == label) => {}
            Some(previous) => {
                previous.push_str("; ");
                previous.push_str(&label);
            }
            None => {
                self.labels.insert(cell, label);
            }
        }
    }

    fn rule(&mut self, cell: RegisterCell, rule: CustomRule) {
        if let Some(previous) = self.rules.get_mut(&cell)
            && bits_only(&rule)
            && symbolic(previous)
            && same_words(previous, &rule)
        {
            for entry in rule.bits {
                if !previous.bits.iter().any(|e| e.bit == entry.bit) {
                    previous.bits.push(entry);
                }
            }
            return;
        }
        if let Some(previous) = self.rules.insert(cell, rule)
            && self.rules.get(&cell) != Some(&previous)
        {
            self.replaced += 1;
        }
    }

    fn add(&mut self, decoded: Decoded) {
        self.label(decoded.cell, decoded.label);
        if let Some(rule) = decoded.rule {
            self.rule(decoded.cell, rule);
        }
    }

    fn into_device(self, name: String) -> Device {
        let pinned: Vec<RegisterCell> = self.labels.keys().copied().collect();
        Device {
            name,
            registers: Registers::from_views(&pinned, &self.labels, &self.rules),
            replaced: self.replaced,
        }
    }
}

impl Profile {
    fn is_definition(&self) -> bool {
        self.parameters
            .iter()
            .flat_map(|group| &group.items)
            .any(|item| item.get("registers").is_some_and(Value::is_sequence))
    }

    fn variants(&self) -> usize {
        self.parameters
            .iter()
            .flat_map(|group| &group.items)
            .map(variants)
            .max()
            .unwrap_or(1)
    }

    fn naming(&self, fallback: &str) -> Naming {
        let info = self.info.as_ref();
        let field = |key: &str| info.and_then(|info| info.get(key));
        Naming {
            manufacturer: field("manufacturer")
                .and_then(text)
                .unwrap_or_else(|| fallback.to_string()),
            models: match field("model") {
                Some(Value::Sequence(models)) => models.iter().filter_map(text).collect(),
                Some(model) => text(model).into_iter().collect(),
                None => Vec::new(),
            },
        }
    }
}

struct Naming {
    manufacturer: String,
    models: Vec<String>,
}

impl Naming {
    const fn named(&self, variant: usize) -> bool {
        variant < self.models.len()
    }

    fn name(&self, variant: usize) -> String {
        let manufacturer = &self.manufacturer;
        match (self.models.get(variant), self.models.last()) {
            (Some(model), _) => format!("{manufacturer} {model}"),
            (None, Some(model)) => format!("{manufacturer} {model} (mod {variant})"),
            (None, None) if variant == 0 => manufacturer.clone(),
            (None, None) => format!("{manufacturer} (mod {variant})"),
        }
    }
}

pub fn parse(text: &str) -> Option<Vec<Device>> {
    parse_named(text, "Solarman")
}

pub fn parse_named(text: &str, fallback: &str) -> Option<Vec<Device>> {
    let Lenient(value) = serde_norway::from_str(text).ok()?;
    let profile: Profile = serde_norway::from_value(value).ok()?;
    if !profile.is_definition() {
        return None;
    }
    let context = Context::new(&profile);
    let naming = profile.naming(fallback);
    let mut imports: Vec<(usize, Import)> = Vec::new();
    for variant in 0..profile.variants().max(naming.models.len()) {
        let import = context.import(variant);
        let repeated = imports.iter().any(|(_, kept)| *kept == import);
        if import.labels.is_empty() || (repeated && !naming.named(variant)) {
            continue;
        }
        imports.push((variant, import));
    }
    let devices: Vec<Device> = imports
        .into_iter()
        .map(|(variant, import)| import.into_device(naming.name(variant)))
        .filter(|device| device.registers.total() > 0)
        .collect();
    (!devices.is_empty()).then_some(devices)
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map_or_default(|first| first.to_uppercase().chain(chars).collect())
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(['-', '_']).filter(|word| !word.is_empty())
}

fn stem(name: &str) -> &str {
    [".yaml", ".yml"]
        .into_iter()
        .find_map(|extension| {
            let cut = name.len().checked_sub(extension.len())?;
            let tail = name.get(cut..)?;
            tail.eq_ignore_ascii_case(extension)
                .then(|| name.get(..cut))
                .flatten()
        })
        .unwrap_or(name)
}

pub fn title(file_name: &str) -> (String, String) {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = stem(base);
    let (vendor, model) = stem.split_once('_').unwrap_or((stem, ""));
    let vendor: Vec<String> = words(vendor).map(capitalized).collect();
    let model: Vec<String> = words(model)
        .map(|word| {
            if word.chars().any(char::is_uppercase) {
                word.to_string()
            } else {
                capitalized(word)
            }
        })
        .collect();
    (vendor.join(" "), model.join(" "))
}

#[cfg(test)]
mod tests {
    use super::{parse, parse_named, title};
    use crate::config::{RegisterEntry, Registers};
    use crate::custom::{CustomRepr, CustomScale, ScaleMode, ScaleRepr};
    use crate::import::Device;
    use crate::modbus::WordOrder;
    use crate::register::RegisterType::{self, Holding, Input};

    fn single(text: &str) -> Device {
        let mut devices = parse(text).expect("importable");
        assert_eq!(devices.len(), 1);
        devices.remove(0)
    }

    fn entry(registers: &Registers, kind: RegisterType, address: u16) -> &RegisterEntry {
        registers
            .entries()
            .find(|&(cell, _)| cell == (kind, address))
            .map(|(_, entry)| entry)
            .unwrap_or_else(|| panic!("{kind:?} {address} imported"))
    }

    fn shown(registers: &Registers, kind: RegisterType, address: u16, words: &[u16]) -> String {
        entry(registers, kind, address)
            .custom
            .as_ref()
            .expect("rule")
            .evaluate(words, WordOrder::ABCD)
    }

    const DEYE_HYBRID: &str = r#"
#
# Deye | Hybrid Inverter
#

info:
  manufacturer: Deye
  model: SG0*LP1

default:
  update_interval: 5
  digits: 6

parameters:
  - group: Info
    update_interval: 3600
    items:
      - name: "Device"
        class: "enum"
        rule: 1
        registers: [0x0000]
        icon: "mdi:information"
        lookup:
          - key: 0x0000
            value: "Unknown"
          - key: [0x0002, 0x0200]
            value: "String Inverter"
          - key: [0x0003, 0x0300]
            value: "Single-Phase Hybrid Inverter"
        attributes:
          [
            "Device Serial Number",
            "Device Rated Power",
          ]

      - name: "Device Serial Number"
        attribute:
        rule: 5
        registers: [0x0003, 0x0004, 0x0005, 0x0006, 0x0007]

      - name: "Device Control Board Firmware Version"
        attribute:
        rule: 7
        delimiter: ""
        registers: [0x000D, 0x000C, 0x000B]

      - name: "Device Communication Board Firmware Version"
        attribute:
        rule: 7
        hex:
        delimiter: ""
        registers: [0x000E, 0x000A]

      - name: "Device Rated Power"
        attribute:
        class: "power"
        state_class: "measurement"
        uom: "W"
        scale: 0.1
        rule: 4
        registers: [0x0010, 0x0011]
        validation:
          min: 1
          max: 18000
          default: 18000

      - name: "Device MPPTs"
        attribute:
        rule: 1
        mask: 0x0F00
        divide: 256
        registers: [0x0012]
        range:
          min: 257
        validation:
          max: 12
          default: 2

      - name: "Device Phases"
        attribute:
        rule: 1
        mask: 0x000F
        registers: [0x0012]
        range:
          min: 257
        validation:
          max: 3
          default: 3

  - group: Control
    items:
      - name: "Device Fault"
        class: "enum"
        rule: 3
        registers: [0x0067, 0x0068, 0x0069, 0x006A]
        icon: "mdi:message-alert-outline"
        lookup:
          - key: 0
            value: "OK"
          - bit: 6
            value: "DC/DC Soft Start failure"
          - bit: 9
            value: "Auxiliary power supply failure"
          - bit: 63
            value: "Temperature is too high"
          - key: default
            value: Problem

  - group: Meter
    items:
      - name: Total Energy Import
        alt: Total Energy Bought
        class: "energy"
        state_class: "total_increasing"
        uom: "kWh"
        scale: 0.1
        rule: 1
        registers: [0x004E, 0x0050]
        icon: "mdi:transmission-tower-export"
        validation:
          min: 0.1

  - group: Battery
    items:
      - name: "Battery Temperature"
        class: "temperature"
        state_class: "measurement"
        uom: "°C"
        scale: 0.1
        rule: 1
        offset: 1000
        registers: [0x00B6]

      - name: "Battery Current"
        class: "current"
        state_class: "measurement"
        uom: "A"
        scale: [0.01, 0.01, 0.1]
        rule: 2
        registers: [0x00BF]
        icon: "mdi:current-dc"

  - group: Load
    items:
      - name: "Generator"
        description: Generator ready
        name_lookup: "io_mode_select"
        update_interval: 30
        platform: "binary_sensor"
        device_class: power
        rule: 1
        registers: [0x00C3]
        bit: 8
"#;

    #[test]
    fn definitions_without_info_take_the_given_name() {
        let text = "parameters:\n  - group: Grid\n    items:\n      - name: Voltage\n        rule: 1\n        registers: [0x0010]\n";
        let named = parse_named(text, "Sofar Hybrid").expect("definition");
        assert_eq!(named[0].name, "Sofar Hybrid");
        assert_eq!(parse(text).expect("definition")[0].name, "Solarman");
    }

    #[test]
    fn converts_a_real_deye_hybrid_definition() {
        let devices = parse(DEYE_HYBRID).expect("importable");
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Deye SG0*LP1", "Deye SG0*LP1 (mod 2)"]);
        assert_eq!(
            shown(&devices[1].registers, Holding, 0xBF, &[(-150i16) as u16]),
            "-15 A"
        );
        let device = &devices[0];
        assert_eq!(device.replaced, 0);
        let r = &device.registers;
        assert!(r.inputs.is_empty());
        assert!(r.holdings.iter().all(|e| e.pinned && e.label.is_some()));

        assert_eq!(shown(r, Holding, 0, &[0x0200]), "String Inverter");
        assert_eq!(
            shown(r, Holding, 0, &[0x0003]),
            "Single-Phase Hybrid Inverter"
        );

        let serial = entry(r, Holding, 3).custom.as_ref().expect("rule");
        assert_eq!(
            (serial.repr, serial.registers, serial.word_order),
            (CustomRepr::Ascii, Some(5), Some(WordOrder::ABCD))
        );
        assert_eq!(
            serial.evaluate(&[0x3231, 0x3334, 0x3536, 0x3738, 0x3930], WordOrder::ABCD),
            "2134567890"
        );

        let rated = entry(r, Holding, 0x10);
        assert_eq!(rated.label.as_deref(), Some("Device Rated Power [W]"));
        let rule = rated.custom.as_ref().expect("rule");
        assert_eq!(
            (rule.repr, rule.word_order),
            (CustomRepr::I32, Some(WordOrder::CDAB))
        );
        assert_eq!(rule.evaluate(&[0x86A0, 0x0001], WordOrder::ABCD), "10000 W");

        for (address, label) in [
            (0x0B, "Device Control Board Firmware Version"),
            (0x0E, "Device Communication Board Firmware Version"),
            (0x12, "Device MPPTs; Device Phases"),
        ] {
            let info = entry(r, Holding, address);
            assert_eq!(info.label.as_deref(), Some(label));
            assert!(info.custom.is_none(), "{label}");
        }

        assert_eq!(shown(r, Holding, 0x67, &[0, 0, 0, 0]), "OK");
        assert_eq!(
            shown(r, Holding, 0x67, &[1 << 6, 0, 0, 0]),
            "DC/DC Soft Start failure"
        );
        assert_eq!(
            shown(r, Holding, 0x67, &[1 << 9, 0, 0, 0]),
            "Auxiliary power supply failure"
        );
        assert_eq!(shown(r, Holding, 0x67, &[1 << 6 | 1 << 9, 0, 0, 0]), "576");

        let import = entry(r, Holding, 0x4E).custom.as_ref().expect("rule");
        assert_eq!(
            (import.repr, &import.next, import.word_order),
            (CustomRepr::U32, &vec![0x50], Some(WordOrder::CDAB))
        );
        assert_eq!(import.evaluate(&[1, 2], WordOrder::ABCD), "13107.3 kWh");

        assert_eq!(shown(r, Holding, 0xB6, &[1250]), "25 °C");
        assert_eq!(shown(r, Holding, 0xBF, &[(-150i16) as u16]), "-1.5 A");
        assert_eq!(shown(r, Holding, 0xC3, &[0x0100]), "Generator");
        assert_eq!(shown(r, Holding, 0xC3, &[0x00FF]), "(none)");
    }

    const PYLONTECH_FORCE: &str = r#"
#
# Pylontech Force H1/H2/H3, configuration modifier respective values are 0/1/2
#

info:
  manufacturer: Pylontech
  model: [Force H1, Force H2, Force H3]

default:
  update_interval: 5
  code: 0x03
  digits: 6

parameters:
  - group: Info
    update_interval: 3600
    items:
      - name: "Device Brand"
        rule: 5
        registers: [4096, 4097, 4098]

  - group: Battery Module 5
    pack: 6
    items:
      - name: "Battery Module 5 Voltage"
        class: "voltage"
        state_class: "measurement"
        uom: "V"
        scale: 0.01
        rule: 1
        registers: [[5221], [], [5221]]
        icon: "mdi:battery"

      - name: "Cell 0 - 15 Voltage"
        class: "voltage"
        state_class: "measurement"
        uom: "V"
        scale: 0.001
        rule: 1
        registers: [[], [5391]]
"#;

    #[test]
    fn model_lists_become_one_device_per_configuration_modifier() {
        let devices = parse(PYLONTECH_FORCE).expect("importable");
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Pylontech Force H1",
                "Pylontech Force H2",
                "Pylontech Force H3"
            ]
        );
        let addresses: Vec<Vec<u16>> = devices
            .iter()
            .map(|d| d.registers.holdings.iter().map(|e| e.address).collect())
            .collect();
        assert_eq!(
            addresses,
            [vec![4096, 5221], vec![4096, 5391], vec![4096, 5221, 5391]]
        );
        assert_eq!(
            shown(&devices[2].registers, Holding, 5391, &[3312]),
            "3.312 V"
        );
        let brand = entry(&devices[0].registers, Holding, 4096);
        assert_eq!(brand.custom.as_ref().and_then(|c| c.registers), Some(3));
    }

    const SOLIS_HYBRID: &str = r#"
default:
  update_interval: 10
  code: 0x04
  min_span: -1
  max_size: 100
  digits: 6

requests:
  - start: 43000
    end: 43300
    code: 0x03

parameters:
  - group: InverterStatus
    items:
      - name: "Inverter Status"
        rule: 1
        registers: [33095]
        icon: "mdi:home-lightning-bolt"
        lookup:
          - key: 0x0
            value: "Waiting State"
          - key: 0x3
            value: "On Grid/Generating"

      - name: "Inverter Voltage"
        class: "voltage"
        state_class: "measurement"
        uom: "V"
        scale: 0.1
        rule: 1
        registers: [33073]
        icon: "mdi:transmission-tower"

      - name: "Backup Mode SOC"
        platform: number
        state_class: "measurement"
        code:
          read: 0x03
          write: 0x06
        uom: "%"
        rule: 1
        registers: [43024]
        range:
          min: 0
          max: 100
        configurable:
          min: 0
          max: 100
          step: 1
        icon: "mdi:battery"

      - name: "Overdischarge SOC"
        state_class: "measurement"
        uom: "%"
        rule: 1
        registers: [43011]
        icon: "mdi:battery"

  - group: Settings
    items:
      - name: "Date & Time"
        platform: datetime
        update_interval: 60
        rule: 8
        code: 0x03
        registers: [43000, 43001, 43002, 43003, 43004, 43005]
"#;

    #[test]
    fn function_codes_come_from_items_requests_and_defaults() {
        let device = single(SOLIS_HYBRID);
        assert_eq!(device.name, "Solarman");
        let r = &device.registers;
        let addresses =
            |entries: &[RegisterEntry]| -> Vec<u16> { entries.iter().map(|e| e.address).collect() };
        assert_eq!(addresses(&r.inputs), [33073, 33095]);
        assert_eq!(addresses(&r.holdings), [43000, 43011, 43024]);
        assert_eq!(shown(r, Input, 33095, &[3]), "On Grid/Generating");
        assert_eq!(shown(r, Input, 33073, &[2301]), "230.1 V");
        assert_eq!(shown(r, Holding, 43024, &[80]), "80 %");
        let clock = entry(r, Holding, 43000);
        assert_eq!(
            (clock.label.as_deref(), clock.custom.is_none()),
            (Some("Date & Time"), true)
        );
    }

    const AFORE_BNT: &str = r#"
default:
  update_interval: 5
  code: 0x04
  digits: 6

parameters:
  - group: Grid
    items:
      - name: "Grid L1 Voltage"
        l: 1
        class: "voltage"
        state_class: "measurement"
        uom: "V"
        scale: 0.1
        rule: 1
        registers: [0x0001]

      - name: "Grid L1 Current"
        l: 1
        class: "current"
        state_class: "measurement"
        uom: "A"
        scale: 0.1
        rule: 1
        registers: [0x0004]

      - name: "Grid L1 Power"
        l: 1
        class: "power"
        state_class: "measurement"
        uom: "W"
        rule: 1
        digits: 3
        scale: 0.1
        sensors:
          - registers: [0x0001]
            multiply:
              registers: [0x0004]

  - group: PV
    items:
      - name: PV Power
        alt: DC Power
        mppt: 1
        class: "power"
        state_class: "measurement"
        uom: "W"
        rule: 1
        digits: 3
        scale: 0.1
        sensors:
          - registers: [0x0007]
            multiply:
              registers: [0x0008]
          - registers: [0x0009]
            multiply:
              registers: [0x000A]

  - group: Device
    items:
      - name: "Total Production"
        persistent:
        class: "energy"
        state_class: "total_increasing"
        uom: "Wh"
        rule: 3
        registers: [0x0017, 0x0016]
        icon: "mdi:solar-power"
        validation:
          min: 0.1
          dev: 100
          invalidate_all: 2

      - name: "Device State"
        class: "enum"
        rule: 1
        registers: [0x0000]
        icon: "mdi:state-machine"
        lookup:
          - key: 0
            value: "Off"
          - bit: 1
            value: "Standby"
          - bit: 8
            value: "Starting"
          - bit: 13
            value: "Running"

      - name: "Grid"
        platform: "binary_sensor"
        device_class: power
        rule: 1
        mask: 0x200
        registers: [0x0000]
"#;

    #[test]
    fn a_real_afore_definition_keeps_direct_readings_over_computed_ones() {
        let device = single(AFORE_BNT);
        assert_eq!(device.replaced, 0);
        let r = &device.registers;
        assert!(r.holdings.is_empty());
        let voltage = entry(r, Input, 1);
        assert_eq!(voltage.label.as_deref(), Some("Grid L1 Voltage [V]"));
        assert_eq!(shown(r, Input, 1, &[2304, 50]), "230.4 V");
        assert_eq!(entry(r, Input, 7).label.as_deref(), Some("PV Power [W]"));
        assert!(entry(r, Input, 7).custom.is_none());

        let total = entry(r, Input, 0x16).custom.as_ref().expect("rule");
        assert_eq!(
            (total.repr, total.word_order, total.next.is_empty()),
            (CustomRepr::U32, Some(WordOrder::ABCD), true)
        );
        assert_eq!(
            total.evaluate(&[0x0001, 0x0002], WordOrder::ABCD),
            "65538 Wh"
        );

        let state = entry(r, Input, 0);
        assert_eq!(state.label.as_deref(), Some("Device State; Grid"));
        assert_eq!(shown(r, Input, 0, &[0]), "Off");
        assert_eq!(shown(r, Input, 0, &[1 << 13]), "Running");
        assert_eq!(shown(r, Input, 0, &[1 << 1]), "Standby");
        assert_eq!(shown(r, Input, 0, &[1 << 13 | 1 << 1]), "(none)");
        assert_eq!(shown(r, Input, 0, &[1 << 9]), "Grid");
    }

    const SOLARMAN_DTSD422: &str = "
default:
  update_interval: 5
  digits: 6

parameters:
  - group: Voltage
    items:
      - name: CT1 Voltage
        class: voltage
        uom: V
        scale: 0.1
        rule: 1
        registers: [0x0001]

  - group: Current
    items:
      - name: CT1 Current
        class: current
        state_class: measurement
        uom: A
        scale: 0.001
        rule: 4
        magnitude: True
        registers: [0x0008, 0x0007]
";

    #[test]
    fn sign_magnitude_values_degrade_to_labels() {
        let device = single(SOLARMAN_DTSD422);
        let r = &device.registers;
        assert_eq!(shown(r, Holding, 1, &[2311]), "231.1 V");
        let current = entry(r, Holding, 7);
        assert_eq!(
            (current.label.as_deref(), current.custom.is_none()),
            (Some("CT1 Current [A]"), true)
        );
    }

    #[test]
    fn single_computed_sensors_scale_by_their_factor_register() {
        let device = single(
            "
info:
  model: Meter
parameters:
  - group: Computed
    items:
      - name: PV Power
        uom: W
        rule: 1
        scale: 0.1
        sensors:
          - registers: [10]
            multiply:
              registers: [11]
              scale: 0.01
      - name: Battery
        uom: W
        rule: 2
        sensors:
          - registers: [20]
            signed:
            operator: subtract
      - name: Sum
        rule: 1
        sensors:
          - registers: [30]
          - registers: [31]
      - name: Shifted
        rule: 1
        sensors:
          - registers: [40]
            offset: 5
            multiply:
              registers: [41]
      - name: Plain
        rule: 1
        registers: [50]
",
        );
        assert_eq!(device.name, "Solarman Meter");
        let r = &device.registers;
        let power = entry(r, Holding, 10).custom.as_ref().expect("rule");
        assert_eq!(
            power.scale,
            Some(CustomScale {
                register: 11,
                mode: ScaleMode::Mul,
                repr: ScaleRepr::Plain(CustomRepr::U16),
            })
        );
        assert_eq!(power.evaluate(&[2000, 500], WordOrder::ABCD), "1000 W");
        assert_eq!(shown(r, Holding, 20, &[5]), "-5 W");
        for address in [30, 40] {
            let e = entry(r, Holding, address);
            assert!(e.label.is_some() && e.custom.is_none(), "{address}");
        }
        assert!(r.holdings.iter().all(|e| e.address != 31));
    }

    #[test]
    fn conflicting_rules_count_as_replaced_while_bits_merge() {
        let device = single(
            "
parameters:
  - group: State
    items:
      - name: Mode
        rule: 1
        registers: [1]
        lookup:
          - key: 0
            value: Idle
      - name: Mode
        rule: 1
        registers: [1]
        lookup:
          - key: 0
            value: Idle
      - name: Work Mode
        rule: 1
        registers: [1]
        lookup:
          - key: 0
            value: Self Use
      - name: Status
        rule: 1
        registers: [2]
      - name: Fan
        rule: 1
        registers: [2]
        bit: 0
      - name: Pump
        rule: 1
        registers: [2]
        mask: 0x0004
      - name: Gone
        rule: 1
        disabled:
        registers: [3]
",
        );
        assert_eq!(device.replaced, 2);
        let r = &device.registers;
        assert_eq!(
            entry(r, Holding, 1).label.as_deref(),
            Some("Mode; Work Mode")
        );
        assert_eq!(shown(r, Holding, 1, &[0]), "Self Use");
        assert_eq!(
            entry(r, Holding, 2).label.as_deref(),
            Some("Status; Fan; Pump")
        );
        assert_eq!(shown(r, Holding, 2, &[0b101]), "Fan|Pump");
        assert_eq!(r.holdings.len(), 2);
    }

    const SOFAR_HYBRID: &str = r#"
#
# Sofar HYD 3000~6000-ES (Hybrid Inverter) | ME 3000-SP (Battery Storage Inverter)
#

default:
  update_interval: 5
  code: 0x03
  digits: 6

parameters:
  - group: PV
    items:
      - name: "PV1 Voltage"
        class: "voltage"
        mppt: 1
        state_class: "measurement"
        uom: "V"
        scale: 0.1
        rule: 1
        registers: [[], [0x0250]]
        icon: "mdi:solar-power-variant"

  - group: Battery
    items:
      - name: "Battery Voltage"
        class: "voltage"
        state_class: "measurement"
        uom: "V"
        scale: [0.01, 0.1]
        rule: 1
        registers: [0x020E]
"#;

    #[test]
    fn configuration_modifier_lists_add_devices_without_a_model_list() {
        let devices = parse(SOFAR_HYBRID).expect("importable");
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Solarman", "Solarman (mod 1)"]);
        let addresses: Vec<Vec<u16>> = devices
            .iter()
            .map(|d| d.registers.holdings.iter().map(|e| e.address).collect())
            .collect();
        assert_eq!(addresses, [vec![0x020E], vec![0x020E, 0x0250]]);
        assert_eq!(
            shown(&devices[0].registers, Holding, 0x020E, &[5120]),
            "51.2 V"
        );
        assert_eq!(
            shown(&devices[1].registers, Holding, 0x020E, &[512]),
            "51.2 V"
        );
        assert_eq!(
            shown(&devices[1].registers, Holding, 0x0250, &[3555]),
            "355.5 V"
        );
    }

    #[test]
    fn configuration_modifiers_without_differences_are_not_repeated() {
        let devices = parse(
            "
info:
  manufacturer: Deye
  model: [SG0*LP3, SG0*HP3]
parameters:
  - group: Battery
    items:
      - name: Battery Voltage
        rule: 1
        scale: [0.1, 0.1, 0.1, 1]
        registers: [1]
      - name: Battery Limit
        rule: 1
        registers: [2]
        configurable:
          max: [61, 800, 900]
",
        )
        .expect("importable");
        let names: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Deye SG0*LP3", "Deye SG0*HP3", "Deye SG0*HP3 (mod 3)"]
        );
        assert_eq!(shown(&devices[2].registers, Holding, 1, &[52]), "52");
    }

    const DEYE_HYBRID_AC_COUPLING: &str = r#"
parameters:
  - group: Settings
    items:
      - name: "Generator"
        platform: "switch"
        rule: 1
        registers: [0x0146]
        value:
          bit: 13

      - name: "Meter"
        description: Ex_Meter For CT
        platform: "switch"
        rule: 1
        registers: [0x0146]

      - name: AC Coupling
        description: Couple inverters and batteries using AC lines
        platform: "select"
        rule: 1
        registers: [0x0146]
        lookup:
          - bit: 0
            mode: "|"
            value: Disabled
          - bit: 6
            mode: "|"
            value: Grid
          - bit: 7
            mode: "|"
            value: Load
"#;

    #[test]
    fn lookup_bits_with_the_any_mode_become_bit_names_that_replace_plain_readings() {
        let device = single(DEYE_HYBRID_AC_COUPLING);
        assert_eq!(device.replaced, 1);
        let r = &device.registers;
        assert_eq!(
            entry(r, Holding, 0x0146).label.as_deref(),
            Some("Generator; Meter; AC Coupling")
        );
        let rule = entry(r, Holding, 0x0146).custom.as_ref().expect("rule");
        assert!(rule.enum_map.is_empty());
        assert_eq!(rule.bits.len(), 3);
        assert_eq!(shown(r, Holding, 0x0146, &[1 << 6]), "Grid");
        assert_eq!(shown(r, Holding, 0x0146, &[1 << 6 | 1 << 13]), "Grid");
    }

    const SOLIS_1P_5G: &str = r#"
default:
  update_interval: 5
  code: 0x04
  digits: 6

parameters:
  - group: InverterStatus
    items:
      - name: "Working Status"
        rule: 1
        registers: [3071]
        icon: "mdi:list-status"

      # Working Status Bit decode
      - name: "WStatus Normal"
        mask: 0x01
        divide: 1
        rule: 1
        registers: [3071]
        icon: "mdi:list-status"

      - name: "WStatus Initialising"
        mask: 0x02
        divide: 2
        rule: 1
        registers: [3071]
        icon: "mdi:list-status"

      - name: "WStatus Grid off"
        mask: 0x04
        divide: 4
        rule: 1
        registers: [3071]
        icon: "mdi:list-status"
"#;

    #[test]
    fn bit_items_replace_a_plain_reading_and_merge_with_each_other() {
        let device = single(SOLIS_1P_5G);
        assert_eq!(device.replaced, 1);
        let r = &device.registers;
        assert_eq!(
            entry(r, Input, 3071).label.as_deref(),
            Some("Working Status; WStatus Normal; WStatus Initialising; WStatus Grid off")
        );
        assert_eq!(
            shown(r, Input, 3071, &[0b101]),
            "WStatus Normal|WStatus Grid off"
        );
    }

    #[test]
    fn duplicate_keys_keep_the_last_value_like_upstream() {
        let device = single(
            "
parameters:
  - group: A
    items:
      - name: X
        rule: 1
        registers: [1]
      - name: Y
        rule: 1
        scale: 0.5
        scale: 0.1
        registers: [2]
",
        );
        let r = &device.registers;
        assert_eq!(shown(r, Holding, 1, &[7]), "7");
        assert_eq!(shown(r, Holding, 2, &[25]), "2.5");
    }

    #[test]
    fn items_without_a_name_key_are_skipped() {
        let device = single(
            "
parameters:
  - group: A
    items:
      - rule: 1
        registers: [1]
      - name: ''
        rule: 1
        registers: [2]
",
        );
        let labels: Vec<(u16, Option<&str>)> = device
            .registers
            .holdings
            .iter()
            .map(|e| (e.address, e.label.as_deref()))
            .collect();
        assert_eq!(labels, [(2, Some("A"))]);
    }

    #[test]
    fn rejects_text_that_is_not_a_solarman_definition() {
        for text in [
            "",
            ":::",
            "- 1\n- 2",
            "parameters: []",
            "parameters: {}",
            "parameters:\n  - group: A\n    items:\n      - name: X\n        rule: 1\n",
            "parameters:\n  - group: A\n",
            "parameters:\n  - group: A\n    items:\n      - name: X\n        registers: [1]\n",
            "modbus:\n  - name: hub\n    sensors:\n      - name: V\n        address: 1\n",
            r#"{"mbmap": 2, "registers": [{"name": "A", "area": "holding", "address": 1}]}"#,
        ] {
            assert!(parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn odd_values_are_skipped_without_panicking() {
        let device = single(
            "
info:
  manufacturer: [odd]
  model: []
parameters:
  - group: 7
    items:
      - name: Huge
        rule: 1
        registers: [70000]
      - name: Text scale
        rule: 1
        scale: lots
        registers: [1]
      - name: Odd lookup
        rule: 1
        registers: [2]
        lookup: [1, {bit: 200, value: x}, {bit: [1, 99], value: y}, {key: [a, 3], value: z}, {key: 4, value: [w]}]
      - name: Long text
        rule: 5
        registers: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126]
      - name: ''
        rule: 3
        mask: 0xFFFFFFFFFFFFFFFF
        registers: [300, 301, 302]
      - name: Wide mask
        rule: 1
        mask: 0x0F00
        bit: 9
        registers: [400]
      - name: Empty
        rule: 1
        registers: []
      - name: Nested
        rule: 1
        registers: [[]]
",
        );
        assert_eq!(device.name, "Solarman");
        let r = &device.registers;
        let labels: Vec<(u16, &str)> = r
            .holdings
            .iter()
            .map(|e| (e.address, e.label.as_deref().unwrap_or_default()))
            .collect();
        assert_eq!(
            labels,
            [
                (0, "Long text"),
                (2, "Odd lookup"),
                (300, "7"),
                (400, "Wide mask")
            ]
        );
        let odd = entry(r, Holding, 2).custom.as_ref().expect("rule");
        assert_eq!(
            odd.enum_map.iter().map(|e| e.value).collect::<Vec<_>>(),
            [3]
        );
        assert!(odd.bits.is_empty());
        for address in [0, 300, 400] {
            assert!(entry(r, Holding, address).custom.is_none(), "{address}");
        }
    }

    #[test]
    fn title_splits_vendor_and_model_from_the_file_name() {
        let pair = |vendor: &str, model: &str| (vendor.to_string(), model.to_string());
        assert_eq!(title("deye_hybrid.yaml"), pair("Deye", "Hybrid"));
        assert_eq!(
            title("afore_BNTxxxKTL-2mppt.yaml"),
            pair("Afore", "BNTxxxKTL 2mppt")
        );
        assert_eq!(
            title("solarman_dtsd422-d3.yaml"),
            pair("Solarman", "Dtsd422 D3")
        );
        assert_eq!(
            title("astro-energy_micro.yaml"),
            pair("Astro Energy", "Micro")
        );
        assert_eq!(title("solis_3p-5g.yml"), pair("Solis", "3p 5g"));
        assert_eq!(
            title("inverter_definitions/sofar_g3hyd.YAML"),
            pair("Sofar", "G3hyd")
        );
        assert_eq!(
            title("kstar_hybrid_v2.json"),
            pair("Kstar", "Hybrid V2.json")
        );
        assert_eq!(title("custom.yaml"), pair("Custom", ""));
        assert_eq!(title(""), pair("", ""));
        assert_eq!(title(".yaml"), pair("", ""));
        assert_eq!(title("éa_ßb-Ωc.yaml"), pair("Éa", "SSb Ωc"));
    }
}
