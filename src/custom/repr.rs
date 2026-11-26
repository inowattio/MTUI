use crate::interpretator::{f16_to_f32, f128_to_f64};
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::tests::{bit, rule};
    use crate::modbus::WordOrder;

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
}
