use serde::{Deserialize, Serialize};

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
    pub(super) fn apply(self, value: f64) -> f64 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::tests::rule;
    use crate::custom::{CustomRepr, CustomRule, EnumEntry, parse_enum};
    use crate::modbus::WordOrder;

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
    fn div_by_zero_is_safe() {
        let mut r = rule(CustomRepr::U16);
        r.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 0.0,
        }];
        assert_eq!(r.evaluate(&[10], WordOrder::ABCD), UNINTERPRETABLE);
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
