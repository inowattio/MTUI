use super::{CustomRepr, compact};
use crate::modbus::WordOrder;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

    pub(super) fn apply(self, value: f64, factor: f64) -> f64 {
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

    pub(super) fn decode(self, words: &[u16], order: WordOrder) -> Option<f64> {
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_order: Option<WordOrder>,
}

impl CustomScale {
    pub fn addresses(self) -> impl Iterator<Item = u16> {
        (0..self.repr.register_count() as u16).map(move |i| self.register.wrapping_add(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::tests::rule;
    use crate::custom::{CustomRule, CustomRules, parse_enum, parse_op};
    use crate::register::RegisterType;
    use std::collections::BTreeMap;

    fn scaled_by(repr: CustomRepr, mode: ScaleMode, scale: ScaleRepr, register: u16) -> CustomRule {
        let mut r = rule(repr);
        r.address = 100;
        r.scale = Some(CustomScale {
            register,
            mode,
            repr: scale,
            ..Default::default()
        });
        r
    }

    const U16: ScaleRepr = ScaleRepr::Plain(CustomRepr::U16);
    const U32: ScaleRepr = ScaleRepr::Plain(CustomRepr::U32);
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

    fn scale_ordered(rule: Option<WordOrder>, scale: Option<WordOrder>) -> CustomRule {
        let mut r = scaled_by(CustomRepr::U16, ScaleMode::Mul, U32, 110);
        r.word_order = rule;
        r.scale.as_mut().unwrap().word_order = scale;
        r
    }

    #[test]
    fn a_scale_word_order_overrides_the_rule_and_the_device_order() {
        let r = scale_ordered(Some(WordOrder::BADC), Some(WordOrder::ABCD));
        assert_eq!(r.scale_factor(&[100, 0, 3], WordOrder::DCBA), Some(3.0));
        assert_eq!(r.evaluate(&[0x6400, 0, 3], WordOrder::DCBA), "300");

        let r = scale_ordered(Some(WordOrder::ABCD), Some(WordOrder::BADC));
        assert_eq!(
            r.scale_factor(&[100, 0, 0x0300], WordOrder::ABCD),
            Some(3.0)
        );
        assert_eq!(r.scale_factor(&[100, 0, 3], WordOrder::ABCD), Some(768.0));

        let r = scale_ordered(None, Some(WordOrder::CDAB));
        assert_eq!(r.scale_factor(&[100, 3, 0], WordOrder::DCBA), Some(3.0));

        let r = scale_ordered(Some(WordOrder::CDAB), None);
        assert_eq!(r.scale_factor(&[100, 3, 0], WordOrder::ABCD), Some(3.0));
        let r = scale_ordered(None, None);
        assert_eq!(
            r.scale_factor(&[100, 0x0300, 0], WordOrder::DCBA),
            Some(3.0)
        );

        let mut single = scaled_by(CustomRepr::U16, ScaleMode::Mul, U16, 110);
        single.word_order = Some(WordOrder::BADC);
        assert_eq!(single.evaluate(&[0x6400, 3], WordOrder::ABCD), "76800");
        single.scale.as_mut().unwrap().word_order = Some(WordOrder::ABCD);
        assert_eq!(single.evaluate(&[0x6400, 3], WordOrder::ABCD), "300");

        let mut sunspec = scaled_by(CustomRepr::U16, ScaleMode::Pow10, ScaleRepr::Sunspec, 110);
        assert_eq!(sunspec.evaluate(&[100, 0x0100], WordOrder::ABCD), "1e258");
        sunspec.scale.as_mut().unwrap().word_order = Some(WordOrder::BADC);
        assert_eq!(sunspec.evaluate(&[100, 0x0100], WordOrder::ABCD), "1000");
        assert_eq!(sunspec.numeric(&[100, 0x0080], WordOrder::ABCD), None);
    }

    #[test]
    fn a_scale_word_order_round_trips_and_is_omitted_when_unset() {
        let r = scale_ordered(None, Some(WordOrder::BADC));
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(
            json,
            r#"{"repr":"u16","scale":{"register":110,"mode":"mul","repr":"u32","word_order":"badc"}}"#
        );
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scale, r.scale);

        let unset = scale_ordered(Some(WordOrder::CDAB), None);
        let json = serde_json::to_string(&unset).unwrap();
        assert_eq!(
            json,
            r#"{"repr":"u16","scale":{"register":110,"mode":"mul","repr":"u32"},"word_order":"cdab"}"#
        );
        let back: CustomRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back.scale.unwrap().word_order, None);
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
                ..Default::default()
            })
        );
        assert!(
            serde_json::from_str::<CustomRule>(
                r#"{"repr":"u16","scale":{"register":3,"repr":"ascii"}}"#
            )
            .is_err()
        );
    }
}
