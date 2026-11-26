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
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::tests::{bit, rule};
    use crate::custom::{CustomOp, CustomRepr, OpKind};
    use crate::modbus::WordOrder;

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
}
