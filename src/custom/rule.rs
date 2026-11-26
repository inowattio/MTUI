use super::{
    ASCII_REGISTERS_MAX, BitEntry, Continuation, CustomOp, CustomRepr, CustomScale, EnumEntry,
    compact,
};
use crate::constants::UNINTERPRETABLE;
use crate::modbus::WordOrder;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

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
        let scale = self.scale()?;
        scale.repr.decode(
            words.get(self.register_count()..)?,
            scale.word_order.or(self.word_order).unwrap_or(order),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::tests::{bit, rule};
    use crate::custom::{OpKind, ScaleMode, parse_op};

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
}
