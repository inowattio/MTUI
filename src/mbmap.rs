//! Converts device maps in the .mbmap format, version 2
//! (<https://github.com/nobrega8/mbmap-library>), to importable registers.

use crate::config::{RegisterEntry, Registers};
use crate::custom::{BitEntry, CustomOp, CustomRepr, CustomRule, EnumEntry};
use crate::modbus::WordOrder;
use crate::register::RegisterType;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// An integer written as a JSON number or as a decimal or `0x` hex string.
fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
            Some(hex) => i64::from_str_radix(hex, 16).ok(),
            None => s.parse().ok(),
        },
        _ => None,
    }
}

fn area(name: &str) -> Option<RegisterType> {
    Some(match name {
        "holding" => RegisterType::Holding,
        "input" => RegisterType::Input,
        "coil" => RegisterType::Coil,
        "discrete" => RegisterType::Discrete,
        _ => return None,
    })
}

fn repr(kind: &str) -> Option<CustomRepr> {
    Some(match kind {
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

const fn word_order(byte_swap: bool, word_swap: bool) -> WordOrder {
    match (byte_swap, word_swap) {
        (false, false) => WordOrder::ABCD,
        (true, false) => WordOrder::BADC,
        (false, true) => WordOrder::CDAB,
        (true, true) => WordOrder::DCBA,
    }
}

/// A register's labels: an inline table, or the name of one in `enums`.
fn enum_entries(reg: &Map<String, Value>, root: &Map<String, Value>) -> Vec<EnumEntry> {
    let table = match reg.get("enum") {
        Some(Value::String(name)) => root.get("enums").and_then(|e| e.get(name)),
        other => other,
    };
    let Some(Value::Object(table)) = table else {
        return Vec::new();
    };
    table
        .iter()
        .filter_map(|(key, text)| {
            Some(EnumEntry {
                value: integer(&Value::String(key.clone()))?,
                text: text.as_str()?.to_string(),
            })
        })
        .collect()
}

/// The registers of a map, each labelled and pinned.
///
/// A rule decodes the type, byte and word order, scale, offset, unit and
/// labels. Bits of one register share a rule; registers that cannot be
/// decoded keep only their label. `None` when `value` is not a version 2 map.
pub fn to_registers(value: &Value) -> Option<Registers> {
    let root = value.as_object()?;
    if integer(root.get("mbmap")?)? != 2 {
        return None;
    }
    let base = root.get("address_base").and_then(integer).unwrap_or(0);
    let little = |reg: &Map<String, Value>, key: &str| {
        reg.get(key)
            .or_else(|| root.get(key))
            .and_then(Value::as_str)
            == Some("little")
    };

    let mut entries: BTreeMap<(RegisterType, u16), RegisterEntry> = BTreeMap::new();
    for reg in root
        .get("registers")?
        .as_array()?
        .iter()
        .filter_map(Value::as_object)
    {
        let (Some(name), Some(kind), Some(address)) = (
            reg.get("name").and_then(Value::as_str),
            reg.get("area").and_then(Value::as_str).and_then(area),
            reg.get("address").and_then(integer),
        ) else {
            continue;
        };
        let Ok(address) = u16::try_from(address - base) else {
            continue;
        };
        let entry = entries
            .entry((kind, address))
            .or_insert_with(|| RegisterEntry {
                address,
                pinned: true,
                ..Default::default()
            });
        let label = match reg.get("unit").and_then(Value::as_str) {
            Some(unit) if !unit.is_empty() => format!("{name} [{unit}]"),
            _ => name.to_string(),
        };
        entry.label = Some(match entry.label.take() {
            Some(previous) => format!("{previous}; {label}"),
            None => label,
        });
        if kind.is_bit() {
            continue;
        }

        let kind_name = reg.get("type").and_then(Value::as_str).unwrap_or_default();
        let bit = reg.get("bit").and_then(integer);
        let bits = reg.get("bits").and_then(integer).unwrap_or(1);
        if let Some(bit) = bit {
            // One bit of the register: named in a shared bit rule
            if bits == 1 && (0..16).contains(&bit) {
                let rule = entry.custom.get_or_insert_with(CustomRule::default);
                rule.bits.push(BitEntry {
                    bit: bit as u8,
                    name: name.to_string(),
                });
            }
            continue;
        }
        let Some(repr) = repr(kind_name) else {
            continue;
        };

        let mut ops = Vec::new();
        let scale = reg.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
        if scale != 1.0 {
            ops.push(CustomOp::try_from(format!("*{scale}")).ok()?);
        }
        let offset = reg.get("offset").and_then(Value::as_f64).unwrap_or(0.0);
        if offset != 0.0 {
            ops.push(CustomOp::try_from(format!("+{offset}")).ok()?);
        }
        entry.custom = Some(CustomRule {
            repr,
            ops,
            enum_map: enum_entries(reg, root),
            suffix: reg
                .get("unit")
                .and_then(Value::as_str)
                .filter(|u| !u.is_empty())
                .map(|u| format!(" {u}"))
                .unwrap_or_default(),
            word_order: Some(word_order(
                little(reg, "byte_order"),
                little(reg, "word_order"),
            )),
            ..Default::default()
        });
    }

    let mut registers = Registers::default();
    for ((kind, _), entry) in entries {
        match kind {
            RegisterType::Holding => registers.holdings.push(entry),
            RegisterType::Input => registers.inputs.push(entry),
            RegisterType::Coil => registers.coils.push(entry),
            RegisterType::Discrete => registers.discretes.push(entry),
        }
    }
    Some(registers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(registers: Value) -> Value {
        json!({"mbmap": 2, "vendor": "V", "model": "M", "word_order": "little", "registers": registers})
    }

    #[test]
    fn converts_types_scale_unit_and_order() {
        let r = to_registers(&map(json!([
            {"name": "Voltage", "area": "input", "address": "0x0010", "type": "float32", "unit": "V"},
            {"name": "Energy", "area": "holding", "address": 4, "type": "uint32", "scale": 0.1, "unit": "kWh", "word_order": "big"},
            {"name": "Temp", "area": "holding", "address": 6, "type": "int16", "offset": -40, "byte_order": "little"},
        ])))
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
        let r = to_registers(&value).unwrap();
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
        let r = to_registers(&value).unwrap();
        assert_eq!(
            r.holdings.iter().map(|e| e.address).collect::<Vec<_>>(),
            [0, 9]
        );
        assert!(r.holdings[1].custom.is_none());
        assert!(to_registers(&json!({"mbmap": 3, "registers": []})).is_none());
        assert!(to_registers(&json!({"holdings": []})).is_none());
    }
}
