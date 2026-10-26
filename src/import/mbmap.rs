use crate::config::Registers;
use crate::custom::{BitEntry, CustomOp, CustomRepr, CustomRule, EnumEntry, OpKind};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

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

#[derive(Clone, Copy, Deserialize)]
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
    bit: Option<Integer>,
    bits: Option<Integer>,
    scale: Option<f64>,
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

impl Register {
    fn unit(&self) -> Option<&str> {
        self.unit.as_deref().filter(|unit| !unit.is_empty())
    }

    fn label(&self) -> String {
        match self.unit() {
            Some(unit) => format!("{} [{unit}]", self.name),
            None => self.name.clone(),
        }
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

    fn rule(&self, register: &Register, address: u16) -> Option<CustomRule> {
        let repr = repr(register.data_type.as_deref())?;
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
        let word_swap = little(register.word_order.as_deref(), self.word_order.as_deref());
        Some(CustomRule {
            address,
            repr,
            ops: ops.into_iter().flatten().collect(),
            enum_map: self.enum_map(register),
            suffix: register
                .unit()
                .map(|unit| format!(" {unit}"))
                .unwrap_or_default(),
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

    fn register(&mut self, map: &DeviceMap, base: i64, register: &Register) {
        let Some(address) = register
            .address
            .value()
            .and_then(|address| address.checked_sub(base))
            .and_then(|address| u16::try_from(address).ok())
        else {
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
        } else if let Some(rule) = map.rule(register, address) {
            self.rules.insert(cell, rule);
        }
    }

    fn into_registers(self) -> Registers {
        let pinned: Vec<RegisterCell> = self.labels.keys().copied().collect();
        Registers::from_views(&pinned, &self.labels, &self.rules)
    }
}

pub fn parse(text: &str) -> Option<Registers> {
    let map: DeviceMap = serde_json::from_str(text.trim()).ok()?;
    if map.mbmap.value()? != 2 {
        return None;
    }
    let base = map
        .address_base
        .as_ref()
        .and_then(Integer::value)
        .unwrap_or(0);
    let mut import = Import::default();
    for register in map
        .registers
        .iter()
        .filter_map(|value| Register::deserialize(value).ok())
    {
        import.register(&map, base, &register);
    }
    let registers = import.into_registers();
    (registers.total() > 0).then_some(registers)
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::custom::CustomRepr;
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
    fn applies_address_base_and_skips_what_cannot_be_read() {
        let mut value = map(json!([
            {"name": "A", "area": "holding", "address": 1, "type": "uint16"},
            {"name": "Below base", "area": "holding", "address": 0, "type": "uint16"},
            {"name": "Serial", "area": "holding", "address": 10, "type": "string", "length": 4},
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
}
