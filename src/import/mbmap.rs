use crate::config::Registers;
use crate::custom::{
    ASCII_REGISTERS_MAX, BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry,
    OpKind, ScaleMode, ScaleRepr,
};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const VERSION: i64 = 2;

#[derive(Deserialize)]
struct Header {
    mbmap: Integer,
}

#[derive(Deserialize)]
struct DeviceMap {
    mbmap: Integer,
    address_base: Option<Integer>,
    byte_order: Option<String>,
    word_order: Option<String>,
    #[serde(default)]
    enums: Map<String, Value>,
    registers: Vec<Value>,
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

pub fn unsupported_version(text: &str) -> Option<i64> {
    let header: Header = serde_json::from_str(text.trim()).ok()?;
    header.mbmap.value().filter(|&version| version != VERSION)
}

pub fn parse(text: &str) -> Option<Registers> {
    let map: DeviceMap = serde_json::from_str(text.trim()).ok()?;
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
    use super::{parse, unsupported_version};
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::{CustomRepr, CustomScale, ScaleMode, ScaleRepr};
    use crate::modbus::WordOrder;
    use serde_json::{Value, json};

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
}
