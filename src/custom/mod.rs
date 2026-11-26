mod cell;
mod entries;
mod op;
mod repr;
mod rule;
mod rules;
mod scale;

pub use cell::*;
pub use entries::*;
pub use op::*;
pub use repr::*;
pub use rule::*;
pub use rules::*;
pub use scale::*;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modbus::WordOrder;

    pub(super) fn rule(repr: CustomRepr) -> CustomRule {
        CustomRule {
            repr,
            ..Default::default()
        }
    }

    pub(super) fn bit(bit: u8, name: &str) -> BitEntry {
        BitEntry {
            bit,
            name: name.to_string(),
        }
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
}
