use crate::interpretator::f16_to_f32;
use crate::modbus::WordOrder;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
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
}

impl CustomRepr {
    pub const ALL: [CustomRepr; 9] = [
        CustomRepr::U16,
        CustomRepr::I16,
        CustomRepr::F16,
        CustomRepr::U32,
        CustomRepr::I32,
        CustomRepr::F32,
        CustomRepr::U64,
        CustomRepr::I64,
        CustomRepr::F64,
    ];

    pub const MAX_REGISTERS: usize = 4;

    pub fn register_count(self) -> usize {
        match self {
            CustomRepr::U16 | CustomRepr::I16 | CustomRepr::F16 => 1,
            CustomRepr::U32 | CustomRepr::I32 | CustomRepr::F32 => 2,
            CustomRepr::U64 | CustomRepr::I64 | CustomRepr::F64 => 4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CustomRepr::U16 => "u16",
            CustomRepr::I16 => "i16",
            CustomRepr::F16 => "f16",
            CustomRepr::U32 => "u32",
            CustomRepr::I32 => "i32",
            CustomRepr::F32 => "f32",
            CustomRepr::U64 => "u64",
            CustomRepr::I64 => "i64",
            CustomRepr::F64 => "f64",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum OpKind {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

impl OpKind {
    pub fn symbol(self) -> char {
        match self {
            OpKind::Add => '+',
            OpKind::Sub => '-',
            OpKind::Mul => '*',
            OpKind::Div => '/',
            OpKind::Pow => '^',
        }
    }

    fn from_symbol(c: char) -> Option<OpKind> {
        match c {
            '+' => Some(OpKind::Add),
            '-' => Some(OpKind::Sub),
            '*' | 'x' | 'X' => Some(OpKind::Mul),
            '/' => Some(OpKind::Div),
            '^' => Some(OpKind::Pow),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub struct CustomOp {
    pub op: OpKind,
    pub v: f64,
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
        format!("{}{:?}", self.op.symbol(), self.v)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct EnumEntry {
    pub value: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CustomRule {
    #[serde(rename = "i")]
    pub address: u16,
    pub repr: CustomRepr,
    #[serde(default)]
    pub ops: Vec<CustomOp>,
    #[serde(default, rename = "enum")]
    pub enum_map: Vec<EnumEntry>,
    #[serde(default)]
    pub decimals: Option<u8>,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub suffix: String,
}

impl CustomRule {
    fn base_value(&self, words: &[u16], order: WordOrder) -> Option<f64> {
        match self.repr {
            CustomRepr::U16 => words.first().map(|&w| w as f64),
            CustomRepr::I16 => words.first().map(|&w| w as i16 as f64),
            CustomRepr::F16 => words.first().map(|&w| f16_to_f32(w) as f64),
            CustomRepr::U32 | CustomRepr::I32 | CustomRepr::F32 => {
                let (&a, &b) = (words.first()?, words.get(1)?);
                let word = order.make_word(a, b);
                Some(match self.repr {
                    CustomRepr::U32 => word as f64,
                    CustomRepr::I32 => word as i32 as f64,
                    _ => f32::from_bits(word) as f64,
                })
            }
            CustomRepr::U64 | CustomRepr::I64 | CustomRepr::F64 => {
                let (&a, &b) = (words.first()?, words.get(1)?);
                let (&c, &d) = (words.get(2)?, words.get(3)?);
                let dword = order.make_dword(order.make_word(a, b), order.make_word(c, d));
                Some(match self.repr {
                    CustomRepr::U64 => dword as f64,
                    CustomRepr::I64 => dword as i64 as f64,
                    _ => f64::from_bits(dword),
                })
            }
        }
    }

    pub fn numeric(&self, words: &[u16], order: WordOrder) -> Option<f64> {
        let base = self.base_value(words, order)?;

        if base.is_finite() && !self.enum_map.is_empty() {
            let key = base as i64;
            if self.enum_map.iter().any(|e| e.value == key) {
                return None;
            }
        }

        let mut value = base;
        for op in &self.ops {
            value = op.apply(value);
        }
        value.is_finite().then_some(value)
    }

    pub fn evaluate(&self, words: &[u16], order: WordOrder) -> String {
        let Some(base) = self.base_value(words, order) else {
            return String::new();
        };

        if base.is_finite() && !self.enum_map.is_empty() {
            let key = base as i64;
            if let Some(entry) = self.enum_map.iter().find(|e| e.value == key) {
                return format!("{}{}{}", self.prefix, entry.text, self.suffix);
            }
        }

        let mut value = base;
        for op in &self.ops {
            value = op.apply(value);
        }

        let number = if !value.is_finite() {
            "--".to_string()
        } else {
            match self.decimals {
                Some(d) => format!("{value:.*}", d as usize),
                None => format!("{value}"),
            }
        };

        format!("{}{}{}", self.prefix, number, self.suffix)
    }
}

pub fn parse_op(input: &str) -> Result<CustomOp, String> {
    let trimmed = input.trim();
    let mut chars = trimmed.chars();
    let symbol = chars.next().ok_or("empty")?;
    let op = OpKind::from_symbol(symbol).ok_or("start with + - * / or ^")?;
    let rest = chars.as_str().trim();
    let v: f64 = rest.parse().map_err(|_| "invalid number".to_string())?;
    if !v.is_finite() {
        return Err("must be a finite number".to_string());
    }
    Ok(CustomOp { op, v })
}

pub fn parse_enum(input: &str) -> Result<EnumEntry, String> {
    let (value, text) = input.split_once('=').ok_or("use value=text")?;
    let value: i64 = value
        .trim()
        .parse()
        .map_err(|_| "invalid value".to_string())?;
    Ok(EnumEntry {
        value,
        text: text.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn qword_needs_four_words() {
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
        assert_eq!(r.evaluate(&[0, 0, 0, 0x3FF0], WordOrder::CDAB), "1");
    }

    #[test]
    fn max_registers_covers_all_reprs() {
        let widest = CustomRepr::ALL
            .iter()
            .map(|r| r.register_count())
            .max()
            .unwrap();
        assert_eq!(widest, CustomRepr::MAX_REGISTERS);
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

        assert_eq!(r.evaluate(&[0x7FC0, 0x0000], WordOrder::ABCD), "--");
    }

    #[test]
    fn div_by_zero_is_safe() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 0.0,
        }];
        assert_eq!(r.evaluate(&[10], WordOrder::ABCD), "--");
    }

    #[test]
    fn serde_round_trip() {
        let mut r = rule(CustomRepr::F32);
        r.address = 100;
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
        let json = serde_json::to_string(&r).unwrap();
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn deserialize_is_terse() {
        let r: CustomRule = serde_json::from_str(r#"{"i": 7, "repr": "U16"}"#).unwrap();
        assert_eq!(r.address, 7);
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
}
