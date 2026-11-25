use crate::constants::UNINTERPRETABLE;
use crate::interpretator::{f16_to_f32, f128_to_f64};
use crate::modbus::WordOrder;
use crate::register::RegisterCell;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::ops::Deref;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CustomRepr {
    #[default]
    U16,
    I16,
    F16,
    U32,
    I32,
    F32,
    U64,
    I64,
    F64,
    U128,
    I128,
    F128,
    Ascii,
}

pub const ASCII_REGISTERS_MAX: u8 = 125;
pub const ASCII_REGISTERS_DEFAULT: u8 = 8;

impl CustomRepr {
    pub const ALL: [Self; 13] = [
        Self::U16,
        Self::I16,
        Self::F16,
        Self::U32,
        Self::I32,
        Self::F32,
        Self::U64,
        Self::I64,
        Self::F64,
        Self::U128,
        Self::I128,
        Self::F128,
        Self::Ascii,
    ];

    pub const fn register_count(self) -> usize {
        match self {
            Self::U16 | Self::I16 | Self::F16 | Self::Ascii => 1,
            Self::U32 | Self::I32 | Self::F32 => 2,
            Self::U64 | Self::I64 | Self::F64 => 4,
            Self::U128 | Self::I128 | Self::F128 => 8,
        }
    }

    pub fn decode(self, raw: u128) -> f64 {
        match self {
            Self::U16 => raw as f64,
            Self::I16 => raw as u16 as i16 as f64,
            Self::F16 => f16_to_f32(raw as u16) as f64,
            Self::U32 => raw as f64,
            Self::I32 => raw as u32 as i32 as f64,
            Self::F32 => f32::from_bits(raw as u32) as f64,
            Self::U64 => raw as f64,
            Self::I64 => raw as i64 as f64,
            Self::F64 => f64::from_bits(raw as u64),
            Self::U128 => raw as f64,
            Self::I128 => raw as i128 as f64,
            Self::F128 => f128_to_f64(raw),
            Self::Ascii => f64::NAN,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::U16 => "u16",
            Self::I16 => "i16",
            Self::F16 => "f16",
            Self::U32 => "u32",
            Self::I32 => "i32",
            Self::F32 => "f32",
            Self::U64 => "u64",
            Self::I64 => "i64",
            Self::F64 => "f64",
            Self::U128 => "u128",
            Self::I128 => "i128",
            Self::F128 => "f128",
            Self::Ascii => "ascii",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Continuation {
    Middle,
    End,
    Detached,
}

impl Continuation {
    pub fn marker(self, owner: impl std::fmt::Display) -> String {
        match self {
            Self::Middle => "|".to_string(),
            Self::End => format!("`- {owner}"),
            Self::Detached => format!("-> {owner}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CustomCell {
    Value(String),
    PartOf {
        owner: u16,
        continuation: Continuation,
    },
}

impl CustomCell {
    pub fn detached(self) -> Self {
        match self {
            Self::PartOf { owner, .. } => Self::PartOf {
                owner,
                continuation: Continuation::Detached,
            },
            value => value,
        }
    }

    pub fn value(self) -> Option<String> {
        match self {
            Self::Value(text) => Some(text),
            Self::PartOf { .. } => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpKind {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl OpKind {
    pub const fn symbol(self) -> char {
        match self {
            Self::Add => '+',
            Self::Sub => '-',
            Self::Mul => '*',
            Self::Div => '/',
            Self::Pow => '^',
        }
    }

    const fn from_symbol(c: char) -> Option<Self> {
        match c {
            '+' => Some(Self::Add),
            '-' => Some(Self::Sub),
            '*' | 'x' | 'X' => Some(Self::Mul),
            '/' => Some(Self::Div),
            '^' => Some(Self::Pow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(into = "String", try_from = "String")]
pub struct CustomOp {
    pub op: OpKind,
    pub v: f64,
}

impl From<CustomOp> for String {
    fn from(op: CustomOp) -> Self {
        op.display()
    }
}

impl TryFrom<String> for CustomOp {
    type Error = &'static str;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        parse_op(&input)
    }
}

impl CustomOp {
    fn apply(self, value: f64) -> f64 {
        match self.op {
            OpKind::Add => value + self.v,
            OpKind::Sub => value - self.v,
            OpKind::Mul => value * self.v,
            OpKind::Div => {
                if self.v == 0.0 {
                    f64::NAN
                } else {
                    value / self.v
                }
            }
            OpKind::Pow => value.powf(self.v),
        }
    }

    pub fn display(self) -> String {
        format!("{}{}", self.op.symbol(), self.v)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScaleMode {
    #[default]
    Pow10,
    Mul,
    Div,
}

impl ScaleMode {
    pub const ALL: [Self; 3] = [Self::Pow10, Self::Mul, Self::Div];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Pow10 => "* 10^reg",
            Self::Mul => "* reg",
            Self::Div => "/ reg",
        }
    }

    fn apply(self, value: f64, factor: f64) -> f64 {
        match self {
            Self::Pow10 if !factor.is_finite() => f64::NAN,
            Self::Pow10 if factor.fract() != 0.0 => value * 10f64.powf(factor),
            Self::Pow10 => match factor as i32 {
                exponent if exponent < 0 => value / 10f64.powi(-exponent),
                exponent => value * 10f64.powi(exponent),
            },
            Self::Mul => value * factor,
            Self::Div if factor == 0.0 => f64::NAN,
            Self::Div => value / factor,
        }
    }

    pub fn describe(self, factor: f64) -> String {
        match self {
            Self::Pow10 => format!("*10^{}", compact(factor)),
            Self::Mul => format!("*{}", compact(factor)),
            Self::Div => format!("/{}", compact(factor)),
        }
    }
}

const SUNSPEC_NOT_IMPLEMENTED: u16 = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleRepr {
    Plain(CustomRepr),
    Sunspec,
}

impl Default for ScaleRepr {
    fn default() -> Self {
        Self::Plain(CustomRepr::U16)
    }
}

impl ScaleRepr {
    pub const ALL: [Self; 13] = [
        Self::Plain(CustomRepr::U16),
        Self::Plain(CustomRepr::I16),
        Self::Sunspec,
        Self::Plain(CustomRepr::F16),
        Self::Plain(CustomRepr::U32),
        Self::Plain(CustomRepr::I32),
        Self::Plain(CustomRepr::F32),
        Self::Plain(CustomRepr::U64),
        Self::Plain(CustomRepr::I64),
        Self::Plain(CustomRepr::F64),
        Self::Plain(CustomRepr::U128),
        Self::Plain(CustomRepr::I128),
        Self::Plain(CustomRepr::F128),
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Plain(repr) => repr.label(),
            Self::Sunspec => "sunspec",
        }
    }

    pub const fn register_count(self) -> usize {
        match self {
            Self::Plain(repr) => repr.register_count(),
            Self::Sunspec => 1,
        }
    }

    fn decode(self, words: &[u16], order: WordOrder) -> Option<f64> {
        let words = words.get(..self.register_count())?;
        Some(match self {
            Self::Sunspec => match order.single(words[0]) {
                SUNSPEC_NOT_IMPLEMENTED => f64::NAN,
                word => f64::from(word as i16),
            },
            Self::Plain(repr) => repr.decode(order.assemble(words)?),
        })
    }
}

impl TryFrom<String> for ScaleRepr {
    type Error = String;

    fn try_from(input: String) -> Result<Self, Self::Error> {
        Self::ALL
            .into_iter()
            .find(|repr| repr.label() == input)
            .ok_or_else(|| format!("unknown scale type {input:?}"))
    }
}

impl Serialize for ScaleRepr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

impl<'de> Deserialize<'de> for ScaleRepr {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CustomScale {
    pub register: u16,
    #[serde(default)]
    pub mode: ScaleMode,
    #[serde(default, skip_serializing_if = "is_default")]
    pub repr: ScaleRepr,
}

impl CustomScale {
    pub fn addresses(self) -> impl Iterator<Item = u16> {
        (0..self.repr.register_count() as u16).map(move |i| self.register.wrapping_add(i))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumEntry {
    pub value: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitEntry {
    pub bit: u8,
    pub name: String,
}

fn serialize_enum<S: Serializer>(entries: &[EnumEntry], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_map(entries.iter().map(|e| (e.value, &e.text)))
}

fn deserialize_enum<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<EnumEntry>, D::Error> {
    let map = BTreeMap::<i64, String>::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(value, text)| EnumEntry { value, text })
        .collect())
}

fn serialize_bits<S: Serializer>(entries: &[BitEntry], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_map(entries.iter().map(|e| (e.bit, &e.name)))
}

fn deserialize_bits<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<BitEntry>, D::Error> {
    let map = BTreeMap::<u8, String>::deserialize(deserializer)?;
    Ok(map
        .into_iter()
        .map(|(bit, name)| BitEntry { bit, name })
        .collect())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CustomRule {
    #[serde(skip)]
    pub address: u16,
    pub repr: CustomRepr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registers: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<CustomScale>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ops: Vec<CustomOp>,
    #[serde(
        default,
        rename = "enum",
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_enum",
        deserialize_with = "deserialize_enum"
    )]
    pub enum_map: Vec<EnumEntry>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_bits",
        deserialize_with = "deserialize_bits"
    )]
    pub bits: Vec<BitEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next: Vec<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_order: Option<WordOrder>,
}

impl CustomRule {
    pub fn register_count(&self) -> usize {
        match (self.repr, self.registers) {
            (CustomRepr::Ascii, Some(n)) => usize::from(n.clamp(1, ASCII_REGISTERS_MAX)),
            (repr, _) => repr.register_count(),
        }
    }

    pub fn word_addresses(&self) -> impl Iterator<Item = u16> + '_ {
        (0..self.register_count()).scan(self.address, |previous, i| {
            let address = if i == 0 {
                self.address
            } else {
                self.next
                    .get(i - 1)
                    .copied()
                    .unwrap_or_else(|| previous.wrapping_add(1))
            };
            *previous = address;
            Some(address)
        })
    }

    pub fn scale(&self) -> Option<CustomScale> {
        self.scale.filter(|_| self.repr != CustomRepr::Ascii)
    }

    pub fn source_addresses(&self) -> impl Iterator<Item = u16> + '_ {
        self.word_addresses()
            .chain(self.scale().into_iter().flat_map(CustomScale::addresses))
    }

    pub fn order(&self, device: WordOrder) -> WordOrder {
        match self.word_order {
            Some(order) => order,
            None if self.repr == CustomRepr::Ascii => WordOrder::ABCD,
            None => device,
        }
    }

    pub fn scale_factor(&self, words: &[u16], order: WordOrder) -> Option<f64> {
        self.scale()?.repr.decode(
            words.get(self.register_count()..)?,
            self.word_order.unwrap_or(order),
        )
    }

    pub fn continuation(&self, address: u16) -> Option<Continuation> {
        let words: Vec<u16> = self.word_addresses().collect();
        let index = words.iter().skip(1).position(|&a| a == address)? + 1;
        let contiguous = |i: usize| words[i] == self.address.wrapping_add(i as u16);
        Some(if !contiguous(index) {
            Continuation::Detached
        } else if index + 1 < words.len() && contiguous(index + 1) {
            Continuation::Middle
        } else {
            Continuation::End
        })
    }

    pub fn raw(&self, words: &[u16], order: WordOrder) -> Option<u128> {
        if self.repr == CustomRepr::Ascii {
            return None;
        }
        self.order(order)
            .assemble(words.get(..self.register_count())?)
    }

    fn text(&self, words: &[u16], order: WordOrder) -> Option<String> {
        let swap = self.order(order).swaps_bytes();
        let text: String = words
            .get(..self.register_count())?
            .iter()
            .flat_map(|&word| if swap { word.swap_bytes() } else { word }.to_be_bytes())
            .take_while(|&byte| byte != 0)
            .map(|byte| {
                if byte == b' ' || byte.is_ascii_graphic() {
                    char::from(byte)
                } else {
                    '.'
                }
            })
            .collect();
        Some(text.trim().to_string())
    }

    pub fn base(&self, words: &[u16], order: WordOrder) -> Option<f64> {
        Some(self.repr.decode(self.raw(words, order)?))
    }

    fn bit_names(&self, raw: u128) -> String {
        let names: Vec<&str> = self
            .bits
            .iter()
            .filter(|e| e.bit < 128 && raw >> e.bit & 1 == 1)
            .map(|e| e.name.as_str())
            .collect();
        if names.is_empty() {
            "(none)".to_string()
        } else {
            names.join("|")
        }
    }

    fn resolve(&self, words: &[u16], order: WordOrder) -> Option<Resolved<'_>> {
        let raw = self.raw(words, order)?;
        let base = self.repr.decode(raw);
        if base.is_finite() {
            let key = base as i64;
            if let Some(entry) = self.enum_map.iter().find(|e| e.value == key) {
                return Some(Resolved::Enum(&entry.text));
            }
        }
        if !self.bits.is_empty() {
            return Some(Resolved::Bits(raw));
        }
        let scaled = match self.scale() {
            Some(scale) => scale.mode.apply(base, self.scale_factor(words, order)?),
            None => base,
        };
        Some(Resolved::Number(
            self.ops.iter().fold(scaled, |v, op| op.apply(v)),
        ))
    }

    pub fn numeric(&self, words: &[u16], order: WordOrder) -> Option<f64> {
        match self.resolve(words, order)? {
            Resolved::Number(v) if v.is_finite() => Some(v),
            _ => None,
        }
    }

    pub fn evaluate(&self, words: &[u16], order: WordOrder) -> String {
        if self.repr == CustomRepr::Ascii {
            return self
                .text(words, order)
                .map_or_default(|text| format!("{}{text}{}", self.prefix, self.suffix));
        }
        let Some(resolved) = self.resolve(words, order) else {
            return String::new();
        };
        let text = match resolved {
            Resolved::Enum(text) => text.to_string(),
            Resolved::Bits(raw) => self.bit_names(raw),
            Resolved::Number(v) if !v.is_finite() => UNINTERPRETABLE.to_string(),
            Resolved::Number(v) => match self.decimals {
                Some(d) => format!("{v:.*}", d as usize),
                None => compact(v),
            },
        };
        format!("{}{text}{}", self.prefix, self.suffix)
    }
}

enum Resolved<'a> {
    Enum(&'a str),
    Bits(u128),
    Number(f64),
}

#[derive(Debug, Default, PartialEq)]
pub struct CustomRules {
    rules: BTreeMap<RegisterCell, CustomRule>,
    owners: BTreeMap<RegisterCell, u16>,
}

impl CustomRules {
    pub fn insert(&mut self, cell: RegisterCell, rule: CustomRule) {
        self.rules.insert(cell, rule);
        self.index();
    }

    pub fn remove(&mut self, cell: &RegisterCell) -> Option<CustomRule> {
        let removed = self.rules.remove(cell);
        self.index();
        removed
    }

    pub fn extend(&mut self, rules: impl IntoIterator<Item = (RegisterCell, CustomRule)>) {
        self.rules.extend(rules);
        self.index();
    }

    pub fn clear(&mut self) {
        self.rules.clear();
        self.owners.clear();
    }

    pub fn part_of(&self, (kind, address): RegisterCell) -> Option<(u16, Continuation)> {
        let owner = *self.owners.get(&(kind, address))?;
        let continuation = self.rules.get(&(kind, owner))?.continuation(address)?;
        Some((owner, continuation))
    }

    fn index(&mut self) {
        self.owners.clear();
        for (&(kind, owner), rule) in &self.rules {
            for address in rule.word_addresses().skip(1) {
                self.owners.entry((kind, address)).or_insert(owner);
            }
        }
    }
}

impl Deref for CustomRules {
    type Target = BTreeMap<RegisterCell, CustomRule>;

    fn deref(&self) -> &Self::Target {
        &self.rules
    }
}

impl From<BTreeMap<RegisterCell, CustomRule>> for CustomRules {
    fn from(rules: BTreeMap<RegisterCell, CustomRule>) -> Self {
        let mut indexed = Self {
            rules,
            owners: BTreeMap::new(),
        };
        indexed.index();
        indexed
    }
}

fn compact(value: f64) -> String {
    let mag = value.abs();
    if mag == 0.0 {
        return "0".to_string();
    }
    if (1e-3..1e10).contains(&mag) {
        return trim_fraction(&format!("{value:.3}")).to_string();
    }
    let scientific = format!("{value:.3e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, ""));
    format!("{}e{exponent}", trim_fraction(mantissa))
}

fn trim_fraction(text: &str) -> &str {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    }
}

pub fn parse_op(input: &str) -> Result<CustomOp, &'static str> {
    let trimmed = input.trim();
    let mut chars = trimmed.chars();
    let symbol = chars.next().ok_or("empty")?;
    let op = OpKind::from_symbol(symbol).ok_or("start with + - * / or ^")?;
    let rest = chars.as_str().trim();
    let v: f64 = rest.parse().map_err(|_| "invalid number")?;
    if !v.is_finite() {
        return Err("must be a finite number");
    }
    Ok(CustomOp { op, v })
}

pub fn parse_enum(input: &str) -> Result<EnumEntry, &'static str> {
    let (value, text) = input.split_once('=').ok_or("use value=text")?;
    let value: i64 = value.trim().parse().map_err(|_| "invalid value")?;
    Ok(EnumEntry {
        value,
        text: text.trim().to_string(),
    })
}

pub fn parse_bit(input: &str) -> Result<BitEntry, &'static str> {
    let (bit, name) = input.split_once('=').ok_or("use bit=name")?;
    let bit: u8 = bit.trim().parse().map_err(|_| "invalid bit")?;
    if bit > 127 {
        return Err("bit must be 0-127");
    }
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty");
    }
    Ok(BitEntry {
        bit,
        name: name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register::RegisterType;

    fn rule(repr: CustomRepr) -> CustomRule {
        CustomRule {
            repr,
            ..Default::default()
        }
    }

    #[test]
    fn word_reprs() {
        assert_eq!(
            rule(CustomRepr::U16).evaluate(&[40000], WordOrder::ABCD),
            "40000"
        );
        assert_eq!(
            rule(CustomRepr::I16).evaluate(&[40000], WordOrder::ABCD),
            "-25536"
        );
    }

    #[test]
    fn dword_needs_two_words() {
        let r = rule(CustomRepr::U32);
        assert_eq!(r.evaluate(&[1], WordOrder::ABCD), "");
        assert_eq!(r.evaluate(&[1, 0], WordOrder::ABCD), "65536");
    }

    #[test]
    fn f32_with_word_order() {
        let r = rule(CustomRepr::F32);
        assert_eq!(r.evaluate(&[0x3F80, 0x0000], WordOrder::ABCD), "1");
    }

    #[test]
    fn dword_needs_four_words() {
        let r = rule(CustomRepr::U64);
        assert_eq!(r.evaluate(&[0, 1, 0], WordOrder::ABCD), "");
        assert_eq!(r.evaluate(&[0, 1, 0, 0], WordOrder::ABCD), "4294967296");
    }

    #[test]
    fn i64_is_signed() {
        let r = rule(CustomRepr::I64);
        assert_eq!(
            r.evaluate(&[0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF], WordOrder::ABCD),
            "-1"
        );
    }

    #[test]
    fn f64_with_word_order() {
        let r = rule(CustomRepr::F64);
        assert_eq!(r.evaluate(&[0x3FF0, 0, 0, 0], WordOrder::ABCD), "1");
        assert_eq!(r.evaluate(&[0xF03F, 0, 0, 0], WordOrder::BADC), "1");
        assert_eq!(r.evaluate(&[0, 0, 0, 0x3FF0], WordOrder::CDAB), "1");
        assert_eq!(r.evaluate(&[0, 0, 0, 0xF03F], WordOrder::DCBA), "1");
    }

    #[test]
    fn qword_needs_eight_words() {
        let r = rule(CustomRepr::U128);
        assert_eq!(r.evaluate(&[0; 7], WordOrder::ABCD), "");
        assert_eq!(
            r.evaluate(&[0, 0, 0, 1, 0, 0, 0, 0], WordOrder::ABCD),
            "1.845e19"
        );
        assert_eq!(r.evaluate(&[0; 8], WordOrder::ABCD), "0");
    }

    #[test]
    fn i128_is_signed() {
        let r = rule(CustomRepr::I128);
        assert_eq!(r.evaluate(&[0xFFFF; 8], WordOrder::ABCD), "-1");
        assert_eq!(r.numeric(&[0xFFFF; 8], WordOrder::DCBA), Some(-1.0));
        assert_eq!(
            rule(CustomRepr::U128).numeric(&[0xFFFF; 8], WordOrder::ABCD),
            Some(u128::MAX as f64)
        );
    }

    #[test]
    fn f128_with_word_order() {
        let r = rule(CustomRepr::F128);
        assert_eq!(
            r.evaluate(&[0x3FFF, 0, 0, 0, 0, 0, 0, 0], WordOrder::ABCD),
            "1"
        );
        assert_eq!(
            r.evaluate(&[0xFF3F, 0, 0, 0, 0, 0, 0, 0], WordOrder::BADC),
            "1"
        );
        assert_eq!(
            r.evaluate(&[0, 0, 0, 0, 0, 0, 0, 0x3FFF], WordOrder::CDAB),
            "1"
        );
        assert_eq!(
            r.evaluate(&[0, 0, 0, 0, 0, 0, 0, 0xFF3F], WordOrder::DCBA),
            "1"
        );
        assert_eq!(
            r.evaluate(&[0xC000, 0x9000, 0, 0, 0, 0, 0, 0], WordOrder::ABCD),
            "-3.125"
        );
    }

    #[test]
    fn bits_reach_the_top_of_a_qword() {
        let mut r = rule(CustomRepr::U128);
        r.bits = vec![bit(127, "sign"), bit(0, "lsb")];
        assert_eq!(
            r.evaluate(&[0x8000, 0, 0, 0, 0, 0, 0, 1], WordOrder::ABCD),
            "sign|lsb"
        );
    }

    #[test]
    fn i64_with_word_order() {
        let r = rule(CustomRepr::I64);
        let regs = [0xFFFF, 0xFFFF, 0xFFFF, 0xFFFE];
        let signed = |order| r.raw(&regs, order).map(|raw| raw as i64);
        assert_eq!(signed(WordOrder::ABCD), Some(-2));
        assert_eq!(signed(WordOrder::BADC), Some(-257));
        assert_eq!(signed(WordOrder::CDAB), Some(-281_474_976_710_657));
        assert_eq!(signed(WordOrder::DCBA), Some(-72_057_594_037_927_937));
    }

    #[test]
    fn per_rule_word_order_overrides_device() {
        let mut r = rule(CustomRepr::F32);
        r.word_order = Some(WordOrder::CDAB);
        assert_eq!(r.evaluate(&[0x0000, 0x3F80], WordOrder::ABCD), "1");
        assert_eq!(r.evaluate(&[0x0000, 0x3F80], WordOrder::DCBA), "1");
    }

    #[test]
    fn a_single_register_follows_the_byte_swap_of_the_word_order() {
        let r = rule(CustomRepr::U16);
        assert_eq!(r.evaluate(&[0x3412], WordOrder::ABCD), "13330");
        assert_eq!(r.evaluate(&[0x3412], WordOrder::CDAB), "13330");
        assert_eq!(r.evaluate(&[0x3412], WordOrder::BADC), "4660");
        assert_eq!(r.evaluate(&[0x3412], WordOrder::DCBA), "4660");

        let mut own = rule(CustomRepr::I16);
        own.word_order = Some(WordOrder::BADC);
        assert_eq!(own.evaluate(&[0xFFFE], WordOrder::ABCD), "-257");
        assert_eq!(own.evaluate(&[0xFEFF], WordOrder::ABCD), "-2");
    }

    #[test]
    fn no_override_follows_device_order() {
        let r = rule(CustomRepr::F32);
        assert_eq!(r.evaluate(&[0x0000, 0x3F80], WordOrder::CDAB), "1");
        assert_eq!(r.evaluate(&[0x3F80, 0x0000], WordOrder::ABCD), "1");
    }

    fn bit(bit: u8, name: &str) -> BitEntry {
        BitEntry {
            bit,
            name: name.to_string(),
        }
    }

    #[test]
    fn bits_name_set_bits() {
        let mut r = rule(CustomRepr::U16);
        r.bits = vec![bit(0, "run"), bit(1, "grid"), bit(15, "beat")];
        assert_eq!(r.evaluate(&[0b11], WordOrder::ABCD), "run|grid");
        assert_eq!(r.evaluate(&[0b10], WordOrder::ABCD), "grid");
        assert_eq!(r.evaluate(&[0x8000], WordOrder::ABCD), "beat");
        assert_eq!(r.evaluate(&[0b100], WordOrder::ABCD), "(none)");
    }

    #[test]
    fn enum_precedes_bits() {
        let mut r = rule(CustomRepr::U16);
        r.enum_map = vec![EnumEntry {
            value: 0,
            text: "idle".into(),
        }];
        r.bits = vec![bit(0, "run")];
        assert_eq!(r.evaluate(&[0], WordOrder::ABCD), "idle");
        assert_eq!(r.evaluate(&[1], WordOrder::ABCD), "run");
    }

    #[test]
    fn bits_span_words_with_order() {
        let mut r = rule(CustomRepr::U32);
        r.bits = vec![bit(16, "high"), bit(0, "low")];
        assert_eq!(r.evaluate(&[1, 0], WordOrder::ABCD), "high");
        assert_eq!(r.evaluate(&[0, 1], WordOrder::ABCD), "low");
        assert_eq!(r.evaluate(&[1, 0], WordOrder::CDAB), "low");
    }

    #[test]
    fn bits_respect_prefix_suffix_and_skip_math() {
        let mut r = rule(CustomRepr::U16);
        r.bits = vec![bit(0, "on")];
        r.ops = vec![CustomOp {
            op: OpKind::Mul,
            v: 100.0,
        }];
        r.prefix = "[".to_string();
        r.suffix = "]".to_string();
        assert_eq!(r.evaluate(&[1], WordOrder::ABCD), "[on]");
        assert_eq!(r.numeric(&[1], WordOrder::ABCD), None);
    }

    #[test]
    fn out_of_range_bit_is_ignored() {
        let mut r = rule(CustomRepr::U16);
        r.bits = vec![bit(63, "top")];
        assert_eq!(r.evaluate(&[0xFFFF], WordOrder::ABCD), "(none)");
    }

    #[test]
    fn parse_bit_helpers() {
        assert_eq!(parse_bit("0=run").unwrap(), bit(0, "run"));
        assert_eq!(parse_bit(" 15 = heartbeat ").unwrap(), bit(15, "heartbeat"));
        assert!(parse_bit("run").is_err());
        assert!(parse_bit("127=x").is_ok());
        assert!(parse_bit("128=x").is_err());
        assert!(parse_bit("a=x").is_err());
        assert!(parse_bit("3=").is_err());
    }

    #[test]
    fn word_addresses_default_contiguous() {
        let mut r = rule(CustomRepr::F64);
        r.address = 100;
        assert_eq!(
            r.word_addresses().collect::<Vec<_>>(),
            vec![100, 101, 102, 103]
        );
    }

    #[test]
    fn word_addresses_jump_then_contiguous() {
        let mut r = rule(CustomRepr::U32);
        r.address = 520;
        r.next = vec![524];
        assert_eq!(r.word_addresses().collect::<Vec<_>>(), vec![520, 524]);

        let mut r = rule(CustomRepr::F64);
        r.address = 520;
        r.next = vec![524];
        assert_eq!(
            r.word_addresses().collect::<Vec<_>>(),
            vec![520, 524, 525, 526]
        );

        r.next = vec![524, 530];
        assert_eq!(
            r.word_addresses().collect::<Vec<_>>(),
            vec![520, 524, 530, 531]
        );
    }

    #[test]
    fn word_addresses_single_register() {
        let mut r = rule(CustomRepr::U16);
        r.address = 7;
        r.next = vec![99];
        assert_eq!(r.word_addresses().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn op_pipeline_order() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![
            CustomOp {
                op: OpKind::Mul,
                v: 0.1,
            },
            CustomOp {
                op: OpKind::Add,
                v: 5.0,
            },
        ];
        r.decimals = Some(1);
        r.prefix = "~ ".to_string();
        r.suffix = " V".to_string();

        assert_eq!(r.evaluate(&[2304], WordOrder::ABCD), "~ 235.4 V");
    }

    #[test]
    fn decimals_formatting() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 3.0,
        }];
        r.decimals = Some(2);
        assert_eq!(r.evaluate(&[10], WordOrder::ABCD), "3.33");
    }

    #[test]
    fn enum_short_circuits_math() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Mul,
            v: 100.0,
        }];
        r.enum_map = vec![
            EnumEntry {
                value: 0,
                text: "Off".into(),
            },
            EnumEntry {
                value: 3,
                text: "Running".into(),
            },
        ];
        assert_eq!(r.evaluate(&[3], WordOrder::ABCD), "Running");

        assert_eq!(r.evaluate(&[2], WordOrder::ABCD), "200");
    }

    #[test]
    fn nan_does_not_match_enum_zero() {
        let mut r = rule(CustomRepr::F32);
        r.enum_map = vec![EnumEntry {
            value: 0,
            text: "Off".into(),
        }];

        assert_eq!(
            r.evaluate(&[0x7FC0, 0x0000], WordOrder::ABCD),
            UNINTERPRETABLE
        );
    }

    #[test]
    fn extreme_floats_use_exponent_notation_without_decimals() {
        let r = rule(CustomRepr::F64);
        assert_eq!(r.evaluate(&[0, 0, 0, 1], WordOrder::ABCD), "4.941e-324");
        assert_eq!(
            r.evaluate(&[0x7E37, 0xE43C, 0x8800, 0x759C], WordOrder::ABCD),
            "1e300"
        );
        assert_eq!(r.evaluate(&[0x3FF0, 0, 0, 0], WordOrder::ABCD), "1");
        assert_eq!(r.evaluate(&[0, 0, 0, 0], WordOrder::ABCD), "0");

        let mut r = rule(CustomRepr::F64);
        r.decimals = Some(2);
        assert_eq!(r.evaluate(&[0, 0, 0, 1], WordOrder::ABCD), "0.00");

        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 1e11,
        }];
        assert_eq!(r.evaluate(&[1432], WordOrder::ABCD), "1.432e-8");
        r.ops[0].v = 3.0;
        assert_eq!(r.evaluate(&[1000], WordOrder::ABCD), "333.333");
        r.ops[0].v = 10.0;
        assert_eq!(r.evaluate(&[2285], WordOrder::ABCD), "228.5");
    }

    #[test]
    fn div_by_zero_is_safe() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 0.0,
        }];
        assert_eq!(r.evaluate(&[10], WordOrder::ABCD), UNINTERPRETABLE);
    }

    fn scaled_by(repr: CustomRepr, mode: ScaleMode, scale: ScaleRepr, register: u16) -> CustomRule {
        let mut r = rule(repr);
        r.address = 100;
        r.scale = Some(CustomScale {
            register,
            mode,
            repr: scale,
        });
        r
    }

    const U16: ScaleRepr = ScaleRepr::Plain(CustomRepr::U16);
    const I16: ScaleRepr = ScaleRepr::Plain(CustomRepr::I16);
    const F32: ScaleRepr = ScaleRepr::Plain(CustomRepr::F32);

    #[test]
    fn pow10_applies_a_power_of_ten_from_any_scale_type() {
        let r = scaled_by(CustomRepr::U16, ScaleMode::Pow10, I16, 110);
        assert_eq!(
            r.evaluate(&[2304, (-1i16) as u16], WordOrder::ABCD),
            "230.4"
        );
        assert_eq!(r.evaluate(&[23, 2], WordOrder::ABCD), "2300");
        assert_eq!(r.evaluate(&[23, 0], WordOrder::ABCD), "23");
        assert_eq!(
            r.numeric(&[12345, (-3i16) as u16], WordOrder::ABCD),
            Some(12.345)
        );
        assert_eq!(
            r.numeric(&[2304, (-1i16) as u16], WordOrder::ABCD),
            Some(230.4)
        );

        let r = scaled_by(CustomRepr::U16, ScaleMode::Pow10, F32, 110);
        assert_eq!(r.evaluate(&[3, 0x4000, 0], WordOrder::ABCD), "300");
        assert_eq!(r.evaluate(&[1, 0x3F00, 0], WordOrder::ABCD), "3.162");
    }

    #[test]
    fn the_sunspec_type_is_an_i16_with_a_not_implemented_marker() {
        let sunspec = scaled_by(CustomRepr::I16, ScaleMode::Pow10, ScaleRepr::Sunspec, 110);
        assert_eq!(
            sunspec.evaluate(&[2304, (-1i16) as u16], WordOrder::ABCD),
            "230.4"
        );
        assert_eq!(
            sunspec.evaluate(&[5, 0x8000], WordOrder::ABCD),
            UNINTERPRETABLE
        );
        assert_eq!(sunspec.numeric(&[5, 0x8000], WordOrder::ABCD), None);

        let plain = scaled_by(CustomRepr::I16, ScaleMode::Pow10, I16, 110);
        assert_eq!(plain.numeric(&[5, 0x8000], WordOrder::ABCD), Some(0.0));

        let mul = scaled_by(CustomRepr::U16, ScaleMode::Mul, ScaleRepr::Sunspec, 110);
        assert_eq!(mul.evaluate(&[5, (-2i16) as u16], WordOrder::ABCD), "-10");
        assert_eq!(mul.evaluate(&[5, 0x8000], WordOrder::ABCD), UNINTERPRETABLE);
    }

    #[test]
    fn a_sunspec_scale_follows_the_byte_swap_of_its_value() {
        let sunspec = scaled_by(CustomRepr::I16, ScaleMode::Pow10, ScaleRepr::Sunspec, 110);
        let plain = scaled_by(CustomRepr::I16, ScaleMode::Pow10, I16, 110);
        for order in [WordOrder::BADC, WordOrder::DCBA] {
            assert_eq!(sunspec.evaluate(&[0x0009, 0xFEFF], order), "23.04");
            assert_eq!(plain.evaluate(&[0x0009, 0xFEFF], order), "23.04");
            assert_eq!(sunspec.numeric(&[0x0009, 0x0080], order), None);
        }
    }

    #[test]
    fn register_scale_multiplies_or_divides_before_ops_and_after_the_words() {
        let mut r = scaled_by(CustomRepr::U32, ScaleMode::Mul, U16, 7);
        r.ops = vec![parse_op("+1").unwrap()];
        assert_eq!(r.evaluate(&[0, 50, 3], WordOrder::ABCD), "151");

        let r = scaled_by(CustomRepr::U16, ScaleMode::Div, U16, 7);
        assert_eq!(r.evaluate(&[1000, 8], WordOrder::ABCD), "125");
        assert_eq!(r.evaluate(&[1000, 0], WordOrder::ABCD), UNINTERPRETABLE);

        let r = scaled_by(CustomRepr::U16, ScaleMode::Div, I16, 7);
        assert_eq!(r.evaluate(&[100, (-4i16) as u16], WordOrder::ABCD), "-25");
    }

    #[test]
    fn a_missing_scale_word_yields_nothing_but_enums_still_match() {
        let mut r = scaled_by(CustomRepr::U16, ScaleMode::Pow10, ScaleRepr::Sunspec, 110);
        assert_eq!(r.evaluate(&[5], WordOrder::ABCD), "");
        r.enum_map = vec![parse_enum("5=five").unwrap()];
        assert_eq!(r.evaluate(&[5], WordOrder::ABCD), "five");
    }

    #[test]
    fn the_scale_register_is_a_source_but_not_a_word_of_the_rule() {
        let r = scaled_by(CustomRepr::U32, ScaleMode::Pow10, ScaleRepr::Sunspec, 110);
        assert_eq!(r.word_addresses().collect::<Vec<_>>(), vec![100, 101]);
        assert_eq!(
            r.source_addresses().collect::<Vec<_>>(),
            vec![100, 101, 110]
        );
        assert_eq!(r.continuation(110), None);

        let rules = CustomRules::from(BTreeMap::from([((RegisterType::Holding, 100), r)]));
        assert_eq!(rules.part_of((RegisterType::Holding, 110)), None);
    }

    #[test]
    fn ascii_ignores_a_scale() {
        let mut r = ascii_rule(2);
        r.address = 100;
        r.scale = Some(CustomScale {
            register: 110,
            mode: ScaleMode::Mul,
            ..Default::default()
        });
        assert_eq!(r.source_addresses().collect::<Vec<_>>(), vec![100, 101]);
        assert_eq!(r.evaluate(&words(b"ABCD"), WordOrder::ABCD), "ABCD");
    }

    #[test]
    fn a_multi_register_scale_decodes_its_own_words_with_the_rule_word_order() {
        let r = scaled_by(CustomRepr::U16, ScaleMode::Mul, F32, 110);
        assert_eq!(
            r.source_addresses().collect::<Vec<_>>(),
            vec![100, 110, 111]
        );
        assert_eq!(r.evaluate(&[100, 0x3FC0, 0], WordOrder::ABCD), "150");
        assert_eq!(r.evaluate(&[100, 0, 0x3FC0], WordOrder::CDAB), "150");
        assert_eq!(r.evaluate(&[100, 0x3FC0], WordOrder::ABCD), "");

        let mut own = r;
        own.word_order = Some(WordOrder::CDAB);
        assert_eq!(own.evaluate(&[100, 0, 0x3FC0], WordOrder::ABCD), "150");
    }

    #[test]
    fn scale_serializes_with_its_type_only_when_not_u16() {
        let r = scaled_by(CustomRepr::I16, ScaleMode::Pow10, ScaleRepr::Sunspec, 40100);
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"repr":"i16","scale":{"register":40100,"mode":"pow10","repr":"sunspec"}}"#
        );
        let r = scaled_by(CustomRepr::U16, ScaleMode::Div, F32, 3);
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"repr":"u16","scale":{"register":3,"mode":"div","repr":"f32"}}"#
        );
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scale, r.scale);

        let terse: CustomRule =
            serde_json::from_str(r#"{"repr":"i16","scale":{"register":40100}}"#).unwrap();
        assert_eq!(
            terse.scale,
            Some(CustomScale {
                register: 40100,
                mode: ScaleMode::Pow10,
                repr: U16,
            })
        );
        assert!(
            serde_json::from_str::<CustomRule>(
                r#"{"repr":"u16","scale":{"register":3,"repr":"ascii"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn serde_round_trip() {
        let mut r = rule(CustomRepr::F32);
        r.ops = vec![CustomOp {
            op: OpKind::Mul,
            v: 0.1,
        }];
        r.enum_map = vec![EnumEntry {
            value: 0,
            text: "Off".into(),
        }];
        r.decimals = Some(2);
        r.prefix = "~ ".into();
        r.word_order = Some(WordOrder::BADC);
        r.bits = vec![bit(3, "warn")];
        r.next = vec![104];
        let json = serde_json::to_string(&r).unwrap();
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn ops_serialize_as_operator_strings() {
        let r: CustomRule =
            serde_json::from_str(r#"{"repr": "u16", "ops": ["/10", " x0.5 ", "+ 2"]}"#).unwrap();
        assert_eq!(
            r.ops.iter().map(|o| o.display()).collect::<Vec<_>>(),
            ["/10", "*0.5", "+2"]
        );
        assert_eq!(
            serde_json::to_string(&r.ops).unwrap(),
            r#"["/10","*0.5","+2"]"#
        );
        assert!(serde_json::from_str::<CustomRule>(r#"{"repr": "u16", "ops": ["10"]}"#).is_err());
    }

    #[test]
    fn enums_and_bits_are_maps_keyed_by_value() {
        let r: CustomRule = serde_json::from_str(
            r#"{"repr": "u16", "enum": {"1": "on", "-1": "fault", "0": "off"}, "bits": {"15": "beat", "0": "run"}}"#,
        )
        .unwrap();
        assert_eq!(
            r.enum_map,
            [(-1, "fault"), (0, "off"), (1, "on")].map(|(value, text)| EnumEntry {
                value,
                text: text.into()
            })
        );
        assert_eq!(r.bits, [bit(0, "run"), bit(15, "beat")]);
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"repr":"u16","enum":{"-1":"fault","0":"off","1":"on"},"bits":{"0":"run","15":"beat"}}"#
        );
    }

    #[test]
    fn empty_fields_are_omitted_when_serializing() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![parse_op("/10").unwrap()];
        r.suffix = " V".into();
        assert_eq!(
            serde_json::to_string(&r).unwrap(),
            r#"{"repr":"u16","ops":["/10"],"suffix":" V"}"#
        );
    }

    #[test]
    fn deserialize_is_terse() {
        let r: CustomRule = serde_json::from_str(r#"{"repr": "u16"}"#).unwrap();
        assert_eq!(r.repr, CustomRepr::U16);
        assert!(r.ops.is_empty());
        assert!(r.enum_map.is_empty());
        assert_eq!(r.prefix, "");
    }

    #[test]
    fn parse_helpers() {
        assert_eq!(
            parse_op("*0.1").unwrap(),
            CustomOp {
                op: OpKind::Mul,
                v: 0.1
            }
        );
        assert_eq!(
            parse_op("+5").unwrap(),
            CustomOp {
                op: OpKind::Add,
                v: 5.0
            }
        );
        assert_eq!(
            parse_op("/10").unwrap(),
            CustomOp {
                op: OpKind::Div,
                v: 10.0
            }
        );
        assert!(parse_op("5").is_err());
        assert!(parse_op("*abc").is_err());
        assert!(parse_op("*inf").is_err());
        assert!(parse_op("/nan").is_err());

        assert_eq!(
            parse_enum("3=Running").unwrap(),
            EnumEntry {
                value: 3,
                text: "Running".into()
            }
        );
        assert!(parse_enum("Running").is_err());
    }

    #[test]
    fn contiguous_words_bracket_down_to_the_last_one() {
        let mut r = rule(CustomRepr::F64);
        r.address = 100;
        assert_eq!(r.continuation(100), None);
        assert_eq!(r.continuation(101), Some(Continuation::Middle));
        assert_eq!(r.continuation(102), Some(Continuation::Middle));
        assert_eq!(r.continuation(103), Some(Continuation::End));
        assert_eq!(r.continuation(104), None);
    }

    #[test]
    fn a_jump_ends_the_bracket_and_detaches_the_rest() {
        let mut r = rule(CustomRepr::F64);
        r.address = 100;
        r.next = vec![101, 200];
        assert_eq!(r.continuation(101), Some(Continuation::End));
        assert_eq!(r.continuation(200), Some(Continuation::Detached));
        assert_eq!(r.continuation(201), Some(Continuation::Detached));
    }

    #[test]
    fn continuation_markers_are_ascii_and_name_the_owner() {
        assert_eq!(Continuation::Middle.marker(100), "|");
        assert_eq!(Continuation::End.marker(100), "`- 100");
        assert_eq!(Continuation::Detached.marker("9C40"), "-> 9C40");
    }

    #[test]
    fn detaching_keeps_the_owner_and_leaves_values_alone() {
        let part = CustomCell::PartOf {
            owner: 100,
            continuation: Continuation::Middle,
        };
        assert_eq!(
            part.detached(),
            CustomCell::PartOf {
                owner: 100,
                continuation: Continuation::Detached,
            }
        );
        let value = CustomCell::Value("1.5 V".to_string());
        assert_eq!(value.clone().detached(), value);
    }

    fn ascii_rule(registers: u8) -> CustomRule {
        CustomRule {
            repr: CustomRepr::Ascii,
            registers: Some(registers),
            ..Default::default()
        }
    }

    fn words(text: &[u8]) -> Vec<u16> {
        text.chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair.get(1).copied().unwrap_or(0)]))
            .collect()
    }

    #[test]
    fn ascii_reads_high_byte_first_and_stops_at_the_first_nul() {
        let r = ascii_rule(4);
        assert_eq!(r.evaluate(&words(b"MTUI\0\0zz"), WordOrder::ABCD), "MTUI");
        assert_eq!(r.evaluate(&words(b"  SN 42 "), WordOrder::ABCD), "SN 42");
    }

    #[test]
    fn ascii_honors_only_the_byte_swap_of_the_word_order() {
        let r = ascii_rule(2);
        let swapped = words(b"TMIU");
        for device in WordOrder::ALL {
            assert_eq!(r.evaluate(&swapped, device), "TMIU", "{device:?}");
        }
        for (own, text) in [
            (WordOrder::ABCD, "TMIU"),
            (WordOrder::BADC, "MTUI"),
            (WordOrder::CDAB, "TMIU"),
            (WordOrder::DCBA, "MTUI"),
        ] {
            let mut r = ascii_rule(2);
            r.word_order = Some(own);
            assert_eq!(r.evaluate(&swapped, WordOrder::ABCD), text, "{own:?}");
            assert_eq!(r.evaluate(&swapped, WordOrder::BADC), text, "{own:?}");
        }
    }

    #[test]
    fn ascii_masks_control_bytes_and_wraps_with_prefix_and_suffix() {
        let mut r = ascii_rule(2);
        r.prefix = "fw ".to_string();
        r.suffix = "!".to_string();
        assert_eq!(r.evaluate(&[0x4101, 0x4243], WordOrder::ABCD), "fw A.BC!");
    }

    #[test]
    fn ascii_needs_every_register_and_is_never_numeric() {
        let r = ascii_rule(3);
        assert_eq!(r.evaluate(&words(b"ABCD"), WordOrder::ABCD), "");
        assert_eq!(r.numeric(&words(b"ABCDEF"), WordOrder::ABCD), None);
        assert_eq!(r.raw(&words(b"AB"), WordOrder::ABCD), None);
    }

    #[test]
    fn ascii_spans_its_register_count_and_clamps_it() {
        let mut r = ascii_rule(3);
        r.address = 200;
        assert_eq!(r.word_addresses().collect::<Vec<_>>(), vec![200, 201, 202]);
        assert_eq!(r.continuation(202), Some(Continuation::End));
        assert_eq!(ascii_rule(0).register_count(), 1);
        assert_eq!(ascii_rule(200).register_count(), 125);
        let mut numeric = rule(CustomRepr::U32);
        numeric.registers = Some(10);
        assert_eq!(numeric.register_count(), 2);
    }

    #[test]
    fn ascii_rules_round_trip_with_their_register_count() {
        let json = serde_json::to_string(&ascii_rule(8)).unwrap();
        assert_eq!(json, r#"{"repr":"ascii","registers":8}"#);
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ascii_rule(8));
        assert!(
            !serde_json::to_string(&rule(CustomRepr::U16))
                .unwrap()
                .contains("registers")
        );
    }

    type Placed = (RegisterCell, CustomRule);

    fn placed(kind: RegisterType, address: u16, repr: CustomRepr, next: &[u16]) -> Placed {
        let mut r = rule(repr);
        r.address = address;
        r.next = next.to_vec();
        ((kind, address), r)
    }

    #[test]
    fn indexed_rules_find_the_owner_of_a_continuation_word() {
        let rules = CustomRules::from(BTreeMap::from([
            placed(RegisterType::Holding, 100, CustomRepr::F64, &[]),
            placed(RegisterType::Holding, 200, CustomRepr::U32, &[520]),
            placed(RegisterType::Holding, 300, CustomRepr::U16, &[]),
        ]));
        let part = |address| rules.part_of((RegisterType::Holding, address));
        assert_eq!(part(100), None, "the owner is not its own continuation");
        assert_eq!(part(101), Some((100, Continuation::Middle)));
        assert_eq!(part(103), Some((100, Continuation::End)));
        assert_eq!(part(104), None);
        assert_eq!(part(520), Some((200, Continuation::Detached)));
        assert_eq!(part(201), None, "the jump skips the next address");
        assert_eq!(part(301), None, "single register rules own nothing else");
        assert_eq!(rules.part_of((RegisterType::Input, 101)), None);
    }

    #[test]
    fn overlapping_rules_resolve_to_the_lowest_owner() {
        let rules = CustomRules::from(BTreeMap::from([
            placed(RegisterType::Holding, 102, CustomRepr::U32, &[]),
            placed(RegisterType::Holding, 100, CustomRepr::F64, &[]),
        ]));
        assert_eq!(
            rules.part_of((RegisterType::Holding, 103)),
            Some((100, Continuation::End))
        );
        assert_eq!(
            rules.part_of((RegisterType::Holding, 102)),
            Some((100, Continuation::Middle)),
            "a word can be both an owner and part of another rule"
        );
    }

    #[test]
    fn the_index_follows_every_edit() {
        let holding = |address| (RegisterType::Holding, address);
        let mut rules = CustomRules::default();
        assert_eq!(rules.part_of(holding(101)), None);

        let (cell, r) = placed(RegisterType::Holding, 100, CustomRepr::U32, &[]);
        rules.insert(cell, r);
        assert_eq!(rules.part_of(holding(101)), Some((100, Continuation::End)));

        let (cell, r) = placed(RegisterType::Holding, 100, CustomRepr::U16, &[]);
        rules.insert(cell, r);
        assert_eq!(
            rules.part_of(holding(101)),
            None,
            "replaced by a narrower rule"
        );

        rules.extend([
            placed(RegisterType::Holding, 100, CustomRepr::U32, &[]),
            placed(RegisterType::Holding, 200, CustomRepr::U32, &[]),
        ]);
        assert_eq!(rules.part_of(holding(101)), Some((100, Continuation::End)));
        assert_eq!(rules.part_of(holding(201)), Some((200, Continuation::End)));

        assert!(rules.remove(&holding(100)).is_some());
        assert_eq!(rules.part_of(holding(101)), None);
        assert_eq!(rules.part_of(holding(201)), Some((200, Continuation::End)));
        assert!(rules.remove(&holding(100)).is_none());

        rules.clear();
        assert_eq!(rules.part_of(holding(201)), None);
        assert!(rules.is_empty());
    }
}
