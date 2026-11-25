use crate::config::Config;
use crate::custom::{CustomOp, CustomRepr, CustomRule, OpKind};
use crate::modbus::{DeviceConfig, Interface, Parity, WordOrder};
use crate::register::{RegisterCell, RegisterType};
use std::collections::BTreeSet;

const MOCK_HOST: &str = "127.0.0.1";
const MOCK_PORT: u16 = 502;
const FALLBACK_HUB: &str = "mtui";
const COMPACT_PRECISION: u8 = 3;

type Pairs = Vec<(&'static str, String)>;

struct Hub {
    name: String,
    connection: Connection,
    timeout: u64,
    message_wait_milliseconds: Option<u64>,
    sensors: Vec<Sensor>,
    binary_sensors: Vec<BinarySensor>,
}

enum Connection {
    Network {
        kind: &'static str,
        host: String,
        port: u16,
    },
    Serial {
        port: String,
        baudrate: u32,
        bytesize: u8,
        parity: &'static str,
        stopbits: u8,
    },
}

#[derive(Default)]
struct Reading {
    data_type: &'static str,
    count: Option<usize>,
    swap: Option<&'static str>,
    scale: Option<f64>,
    offset: Option<f64>,
    precision: Option<u8>,
    unit_of_measurement: Option<String>,
}

struct Sensor {
    name: String,
    slave: u8,
    address: u16,
    input_type: &'static str,
    reading: Reading,
}

struct BinarySensor {
    name: String,
    slave: u8,
    address: u16,
    input_type: &'static str,
}

fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control()
                || matches!(
                    c,
                    '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}'
                ) =>
            {
                out.push_str(&format!("\\u{:04X}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Connection {
    fn pairs(&self) -> Pairs {
        match self {
            Self::Network { kind, host, port } => vec![
                ("type", (*kind).to_string()),
                ("host", quoted(host)),
                ("port", port.to_string()),
            ],
            Self::Serial {
                port,
                baudrate,
                bytesize,
                parity,
                stopbits,
            } => vec![
                ("type", "serial".to_string()),
                ("port", quoted(port)),
                ("baudrate", baudrate.to_string()),
                ("bytesize", bytesize.to_string()),
                ("parity", (*parity).to_string()),
                ("stopbits", stopbits.to_string()),
                ("method", "rtu".to_string()),
            ],
        }
    }
}

impl Reading {
    fn pairs(&self) -> Pairs {
        [
            Some(("data_type", self.data_type.to_string())),
            self.count.map(|count| ("count", count.to_string())),
            self.swap.map(|swap| ("swap", swap.to_string())),
            self.scale.map(|scale| ("scale", format!("{scale:?}"))),
            self.offset.map(|offset| ("offset", format!("{offset:?}"))),
            self.precision
                .map(|precision| ("precision", precision.to_string())),
            self.unit_of_measurement
                .as_deref()
                .map(|unit| ("unit_of_measurement", quoted(unit))),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

impl Sensor {
    fn pairs(&self) -> Pairs {
        let mut pairs = vec![
            ("name", quoted(&self.name)),
            ("slave", self.slave.to_string()),
            ("address", self.address.to_string()),
            ("input_type", self.input_type.to_string()),
        ];
        pairs.extend(self.reading.pairs());
        pairs
    }
}

impl BinarySensor {
    fn pairs(&self) -> Pairs {
        vec![
            ("name", quoted(&self.name)),
            ("slave", self.slave.to_string()),
            ("address", self.address.to_string()),
            ("input_type", self.input_type.to_string()),
        ]
    }
}

fn item(lines: &mut Vec<String>, indent: &str, pairs: Pairs) {
    for (i, (key, value)) in pairs.into_iter().enumerate() {
        let lead = if i == 0 { "- " } else { "  " };
        lines.push(format!("{indent}{lead}{key}: {value}"));
    }
}

fn section(lines: &mut Vec<String>, key: &str, items: Vec<Pairs>) {
    if items.is_empty() {
        return;
    }
    lines.push(format!("  {key}:"));
    for pairs in items {
        item(lines, "  ", pairs);
    }
}

impl Hub {
    fn render(&self) -> String {
        let mut head = vec![("name", quoted(&self.name))];
        head.extend(self.connection.pairs());
        head.push(("timeout", self.timeout.to_string()));
        head.extend(
            self.message_wait_milliseconds
                .map(|wait| ("message_wait_milliseconds", wait.to_string())),
        );
        let mut lines = vec!["modbus:".to_string()];
        item(&mut lines, "", head);
        section(
            &mut lines,
            "sensors",
            self.sensors.iter().map(Sensor::pairs).collect(),
        );
        section(
            &mut lines,
            "binary_sensors",
            self.binary_sensors
                .iter()
                .map(BinarySensor::pairs)
                .collect(),
        );
        lines.push(String::new());
        lines.join("\n")
    }
}

fn connection(interface: &Interface) -> Connection {
    match interface {
        Interface::Tcp(params) => Connection::Network {
            kind: "tcp",
            host: params.ip.clone(),
            port: params.port,
        },
        Interface::RtuOverTcp(params) => Connection::Network {
            kind: "rtuovertcp",
            host: params.ip.clone(),
            port: params.port,
        },
        Interface::Serial(params) => Connection::Serial {
            port: params.path.clone(),
            baudrate: params.baud_rate,
            bytesize: params.data_bits.into(),
            parity: match params.parity {
                Parity::None => "N",
                Parity::Odd => "O",
                Parity::Even => "E",
            },
            stopbits: params.stop_bits.into(),
        },
        Interface::Mock => Connection::Network {
            kind: "tcp",
            host: MOCK_HOST.to_string(),
            port: MOCK_PORT,
        },
    }
}

const fn data_type(repr: CustomRepr) -> Option<&'static str> {
    Some(match repr {
        CustomRepr::U16 => "uint16",
        CustomRepr::I16 => "int16",
        CustomRepr::F16 => "float16",
        CustomRepr::U32 => "uint32",
        CustomRepr::I32 => "int32",
        CustomRepr::F32 => "float32",
        CustomRepr::U64 => "uint64",
        CustomRepr::I64 => "int64",
        CustomRepr::F64 => "float64",
        CustomRepr::Ascii => "string",
        CustomRepr::U128 | CustomRepr::I128 | CustomRepr::F128 => return None,
    })
}

const fn swap(order: WordOrder, words: bool) -> Option<&'static str> {
    match (order, words) {
        (WordOrder::BADC, _) | (WordOrder::DCBA, false) => Some("byte"),
        (WordOrder::CDAB, true) => Some("word"),
        (WordOrder::DCBA, true) => Some("word_byte"),
        (WordOrder::ABCD | WordOrder::CDAB, _) => None,
    }
}

fn contiguous(rule: &CustomRule) -> bool {
    rule.word_addresses()
        .zip(0u16..)
        .all(|(address, i)| address == rule.address.wrapping_add(i))
}

fn affine(ops: &[CustomOp]) -> Option<(f64, f64)> {
    ops.iter()
        .try_fold((1.0, 0.0), |(scale, offset), op| match op.op {
            OpKind::Add => Some((scale, offset + op.v)),
            OpKind::Sub => Some((scale, offset - op.v)),
            OpKind::Mul => Some((scale * op.v, offset * op.v)),
            OpKind::Div if op.v != 0.0 => Some((scale / op.v, offset / op.v)),
            OpKind::Pow if (op.v - 1.0).abs() <= f64::EPSILON => Some((scale, offset)),
            OpKind::Div | OpKind::Pow => None,
        })
        .filter(|(scale, offset)| scale.is_finite() && offset.is_finite() && *scale != 0.0)
}

fn raw() -> Reading {
    Reading {
        data_type: "uint16",
        ..Reading::default()
    }
}

fn reading(rule: &CustomRule, device: WordOrder) -> Reading {
    let Some(data_type) = data_type(rule.repr).filter(|_| contiguous(rule)) else {
        return raw();
    };
    let ascii = rule.repr == CustomRepr::Ascii;
    let mut reading = Reading {
        data_type,
        count: ascii.then(|| rule.register_count()),
        swap: swap(rule.order(device), rule.register_count() > 1 && !ascii),
        ..Reading::default()
    };
    let plain =
        !ascii && rule.scale().is_none() && rule.enum_map.is_empty() && rule.bits.is_empty();
    if let Some((scale, offset)) = affine(&rule.ops).filter(|_| plain) {
        let float = matches!(
            rule.repr,
            CustomRepr::F16 | CustomRepr::F32 | CustomRepr::F64
        );
        let fractional = scale.fract() != 0.0 || offset.fract() != 0.0;
        reading.scale = Some(scale).filter(|v| (v - 1.0).abs() > f64::EPSILON);
        reading.offset = Some(offset).filter(|&v| v != 0.0);
        reading.precision = rule
            .decimals
            .or_else(|| (float || fractional).then_some(COMPACT_PRECISION));
        reading.unit_of_measurement = Some(rule.suffix.trim())
            .filter(|unit| !unit.is_empty())
            .map(str::to_string);
    }
    reading
}

fn name(
    taken: &mut BTreeSet<String>,
    label: Option<&str>,
    (kind, address): RegisterCell,
) -> String {
    let base = label
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .map_or_else(|| format!("{} {address}", kind.name()), str::to_string);
    std::iter::once(base.clone())
        .chain((2u32..).map(|i| format!("{base} {i}")))
        .find(|candidate| taken.insert(candidate.clone()))
        .unwrap_or(base)
}

const fn input_type(kind: RegisterType) -> &'static str {
    match kind {
        RegisterType::Holding => "holding",
        RegisterType::Input => "input",
        RegisterType::Coil => "coil",
        RegisterType::Discrete => "discrete_input",
    }
}

fn hub(config: &Config) -> Hub {
    let DeviceConfig {
        interface,
        unit_id,
        request_timeout_ms,
        request_gap_ms,
        word_order,
        ..
    } = &config.device;
    let mut entries: Vec<_> = config
        .registers
        .entries()
        .filter(|(_, entry)| entry.label.is_some() || entry.custom.is_some())
        .collect();
    entries.sort_by_key(|&(cell, _)| cell);
    let (mut sensors, mut binary_sensors) = (Vec::new(), Vec::new());
    let (mut sensor_names, mut binary_names) = (BTreeSet::new(), BTreeSet::new());
    for (cell @ (kind, address), entry) in entries {
        let label = entry.label.as_deref();
        if kind.is_bit() {
            binary_sensors.push(BinarySensor {
                name: name(&mut binary_names, label, cell),
                slave: *unit_id,
                address,
                input_type: input_type(kind),
            });
        } else {
            sensors.push(Sensor {
                name: name(&mut sensor_names, label, cell),
                slave: *unit_id,
                address,
                input_type: input_type(kind),
                reading: entry
                    .custom
                    .as_ref()
                    .map_or_else(raw, |rule| reading(rule, *word_order)),
            });
        }
    }
    let name = config.name.trim();
    Hub {
        name: if name.is_empty() { FALLBACK_HUB } else { name }.to_string(),
        connection: connection(interface),
        timeout: request_timeout_ms.div_ceil(1000).max(1),
        message_wait_milliseconds: Some(*request_gap_ms).filter(|&gap| gap > 0),
        sensors,
        binary_sensors,
    }
}

pub fn export(config: &Config) -> String {
    hub(config).render()
}

#[cfg(test)]
mod tests {
    use super::export;
    use crate::config::{Config, RegisterEntry, Registers};
    use crate::custom::{
        BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry, OpKind,
    };
    use crate::import::home_assistant::parse;
    use crate::modbus::{
        DataBits, DeviceConfig, Interface, InterfaceSerialParams, InterfaceTcpParams, Parity,
        StopBits, WordOrder,
    };
    use serde_norway::Value;

    fn op(op: OpKind, v: f64) -> CustomOp {
        CustomOp { op, v }
    }

    fn rule(address: u16, repr: CustomRepr, word_order: Option<WordOrder>) -> CustomRule {
        CustomRule {
            address,
            repr,
            word_order,
            ..CustomRule::default()
        }
    }

    fn entry(address: u16, label: Option<&str>, custom: Option<CustomRule>) -> RegisterEntry {
        RegisterEntry {
            address,
            label: label.map(str::to_string),
            pinned: false,
            custom,
        }
    }

    fn config(device: DeviceConfig, registers: Registers) -> Config {
        Config {
            name: "inverter".to_string(),
            device,
            registers,
            ..Config::default()
        }
    }

    fn tcp(unit_id: u8, word_order: WordOrder) -> DeviceConfig {
        DeviceConfig {
            interface: Interface::Tcp(InterfaceTcpParams {
                ip: "10.0.0.5".to_string(),
                port: 1502,
            }),
            unit_id,
            word_order,
            ..DeviceConfig::default()
        }
    }

    fn round_trip(config: &Config) -> Registers {
        let mut devices = parse(&export(config)).expect("importable");
        assert_eq!(devices.len(), 1);
        let device = devices.remove(0);
        assert_eq!(
            device.name,
            format!("{}, unit {}", config.name, config.device.unit_id)
        );
        assert_eq!(device.replaced, 0);
        device.registers
    }

    fn hub(text: &str) -> Value {
        let root: Value = serde_norway::from_str(text).expect("yaml");
        root["modbus"][0].clone()
    }

    #[test]
    fn expressible_labels_and_rules_round_trip_exactly() {
        let voltage = CustomRule {
            ops: vec![op(OpKind::Mul, 0.1), op(OpKind::Add, -5.0)],
            decimals: Some(1),
            suffix: " V".to_string(),
            ..rule(0x10, CustomRepr::U16, Some(WordOrder::ABCD))
        };
        let power = CustomRule {
            suffix: " kW".to_string(),
            decimals: Some(2),
            ..rule(20, CustomRepr::F32, Some(WordOrder::CDAB))
        };
        let model = CustomRule {
            registers: Some(6),
            ..rule(100, CustomRepr::Ascii, Some(WordOrder::BADC))
        };
        let holdings = vec![
            entry(0x10, Some("Voltage"), Some(voltage)),
            entry(20, Some("Power"), Some(power)),
            entry(
                30,
                Some("Energy"),
                Some(rule(30, CustomRepr::U64, Some(WordOrder::DCBA))),
            ),
            entry(
                40,
                Some("Half"),
                Some(rule(40, CustomRepr::F16, Some(WordOrder::BADC))),
            ),
            entry(100, Some("Model"), Some(model)),
        ];
        let inputs = vec![
            entry(
                1,
                Some("Current"),
                Some(rule(1, CustomRepr::I32, Some(WordOrder::BADC))),
            ),
            entry(
                5,
                Some("Total"),
                Some(rule(5, CustomRepr::F64, Some(WordOrder::ABCD))),
            ),
            entry(
                9,
                Some("Signed"),
                Some(rule(9, CustomRepr::I16, Some(WordOrder::ABCD))),
            ),
        ];
        let registers = Registers {
            holdings,
            inputs,
            coils: vec![entry(3, Some("Relay"), None)],
            discretes: vec![entry(7, Some("Alarm"), None)],
        };
        let config = config(tcp(7, WordOrder::ABCD), registers.clone());
        let back = round_trip(&config);
        let mut expected = registers;
        for entry in expected.holdings.iter_mut().chain(&mut expected.inputs) {
            let rule = entry.custom.as_mut().unwrap();
            if matches!(rule.repr, CustomRepr::F16 | CustomRepr::F64) {
                assert_eq!(rule.decimals, None);
                rule.decimals = Some(3);
            }
        }
        let registers = expected;
        assert_eq!(back.holdings, registers.holdings);
        assert_eq!(back.inputs, registers.inputs);
        assert_eq!(back.coils, registers.coils);
        assert_eq!(back.discretes, registers.discretes);
    }

    #[test]
    fn word_orders_and_ops_round_trip_to_their_canonical_form() {
        let holdings = vec![
            entry(1, Some("Inherited"), Some(rule(1, CustomRepr::F32, None))),
            entry(
                3,
                Some("Short swapped"),
                Some(rule(3, CustomRepr::U16, Some(WordOrder::DCBA))),
            ),
            entry(
                4,
                Some("Short words"),
                Some(rule(4, CustomRepr::I16, Some(WordOrder::CDAB))),
            ),
            entry(
                5,
                Some("Text"),
                Some(CustomRule {
                    registers: Some(2),
                    ..rule(5, CustomRepr::Ascii, None)
                }),
            ),
            entry(
                7,
                Some("Text words"),
                Some(CustomRule {
                    registers: Some(2),
                    ..rule(7, CustomRepr::Ascii, Some(WordOrder::DCBA))
                }),
            ),
            entry(
                9,
                Some("Chained"),
                Some(CustomRule {
                    ops: vec![
                        op(OpKind::Add, 5.0),
                        op(OpKind::Mul, 2.0),
                        op(OpKind::Div, 4.0),
                        op(OpKind::Sub, 1.0),
                        op(OpKind::Pow, 1.0),
                    ],
                    suffix: "kWh ".to_string(),
                    prefix: "~".to_string(),
                    ..rule(9, CustomRepr::I16, Some(WordOrder::ABCD))
                }),
            ),
        ];
        let config = config(
            tcp(1, WordOrder::CDAB),
            Registers {
                holdings,
                ..Registers::default()
            },
        );
        let back = round_trip(&config);
        let rules: Vec<_> = back
            .holdings
            .iter()
            .map(|e| e.custom.clone().expect("rule"))
            .collect();
        let orders: Vec<_> = rules.iter().map(|r| (r.address, r.word_order)).collect();
        assert_eq!(
            orders,
            [
                (1, Some(WordOrder::CDAB)),
                (3, Some(WordOrder::BADC)),
                (4, Some(WordOrder::ABCD)),
                (5, Some(WordOrder::ABCD)),
                (7, Some(WordOrder::BADC)),
                (9, Some(WordOrder::ABCD)),
            ]
        );
        for (original, back) in config.registers.holdings.iter().zip(&back.holdings) {
            assert_eq!(original.label, back.label);
            let (original, back) = (
                original.custom.as_ref().unwrap(),
                back.custom.as_ref().unwrap(),
            );
            assert_eq!(original.repr, back.repr);
            assert_eq!(original.register_count(), back.register_count());
            let words = [0x4D54, 0x5549, 0x0102, 0x0304];
            assert_eq!(
                original.numeric(&words, WordOrder::CDAB),
                back.numeric(&words, WordOrder::ABCD),
                "{}",
                original.address
            );
            if original.repr == CustomRepr::Ascii {
                assert_eq!(
                    original.evaluate(&words, WordOrder::CDAB),
                    back.evaluate(&words, WordOrder::ABCD)
                );
            }
        }
        let decimals: Vec<_> = rules.iter().map(|r| (r.address, r.decimals)).collect();
        assert_eq!(
            decimals,
            [
                (1, Some(3)),
                (3, None),
                (4, None),
                (5, None),
                (7, None),
                (9, Some(3)),
            ]
        );
        let chained = &rules[5];
        assert_eq!(chained.ops, [op(OpKind::Mul, 0.5), op(OpKind::Add, 1.5)]);
        assert_eq!(chained.suffix, " kWh");
        assert_eq!(chained.prefix, "");
        assert_eq!(chained.evaluate(&[4], WordOrder::ABCD), "3.500 kWh");
        assert_eq!(
            config.registers.holdings[5]
                .custom
                .as_ref()
                .unwrap()
                .evaluate(&[4], WordOrder::ABCD),
            "~3.5kWh "
        );
    }

    #[test]
    fn inexpressible_rules_degrade_to_the_raw_sensor() {
        let scaled = CustomRule {
            scale: Some(CustomScale {
                register: 50,
                ..CustomScale::default()
            }),
            ops: vec![op(OpKind::Mul, 2.0)],
            suffix: " W".to_string(),
            decimals: Some(1),
            ..rule(10, CustomRepr::I16, Some(WordOrder::ABCD))
        };
        let enumerated = CustomRule {
            enum_map: vec![EnumEntry {
                value: 1,
                text: "on".to_string(),
            }],
            suffix: " mode".to_string(),
            ..rule(11, CustomRepr::U16, Some(WordOrder::BADC))
        };
        let flags = CustomRule {
            bits: vec![BitEntry {
                bit: 0,
                name: "fault".to_string(),
            }],
            ..rule(12, CustomRepr::U32, Some(WordOrder::CDAB))
        };
        let powered = CustomRule {
            ops: vec![op(OpKind::Pow, 2.0)],
            suffix: " m2".to_string(),
            ..rule(14, CustomRepr::U16, Some(WordOrder::ABCD))
        };
        let divided = CustomRule {
            ops: vec![op(OpKind::Div, 0.0)],
            ..rule(15, CustomRepr::U16, Some(WordOrder::ABCD))
        };
        let wide = CustomRule {
            ops: vec![op(OpKind::Mul, 3.0)],
            ..rule(16, CustomRepr::U128, Some(WordOrder::ABCD))
        };
        let zeroed = |address, ops| CustomRule {
            ops,
            suffix: " W".to_string(),
            decimals: Some(1),
            ..rule(address, CustomRepr::U16, Some(WordOrder::ABCD))
        };
        let scattered = CustomRule {
            next: vec![40],
            ..rule(30, CustomRepr::F32, Some(WordOrder::ABCD))
        };
        let holdings = vec![
            entry(10, Some("Scaled"), Some(scaled)),
            entry(11, Some("Mode"), Some(enumerated)),
            entry(12, Some("Flags"), Some(flags)),
            entry(14, Some("Area"), Some(powered)),
            entry(15, Some("Broken"), Some(divided)),
            entry(16, Some("Wide"), Some(wide)),
            entry(
                24,
                Some("Zero"),
                Some(zeroed(24, vec![op(OpKind::Mul, 0.0), op(OpKind::Add, 3.0)])),
            ),
            entry(
                25,
                Some("Negative zero"),
                Some(zeroed(25, vec![op(OpKind::Mul, -0.0)])),
            ),
            entry(
                26,
                Some("Underflow"),
                Some(zeroed(
                    26,
                    vec![op(OpKind::Mul, 1e-200), op(OpKind::Mul, 1e-200)],
                )),
            ),
            entry(30, Some("Scattered"), Some(scattered)),
        ];
        let config = config(
            tcp(1, WordOrder::ABCD),
            Registers {
                holdings,
                ..Registers::default()
            },
        );
        let text = export(&config);
        assert!(!text.contains("scale:"), "{text}");
        assert!(!text.contains("offset:"), "{text}");
        let back = round_trip(&config);
        let degraded: Vec<_> = back
            .holdings
            .iter()
            .map(|e| {
                let rule = e.custom.clone().expect("rule");
                (
                    e.address,
                    e.label.clone().unwrap(),
                    rule.repr,
                    rule.word_order,
                )
            })
            .collect();
        assert_eq!(
            degraded,
            [
                (
                    10,
                    "Scaled".to_string(),
                    CustomRepr::I16,
                    Some(WordOrder::ABCD)
                ),
                (
                    11,
                    "Mode".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::BADC)
                ),
                (
                    12,
                    "Flags".to_string(),
                    CustomRepr::U32,
                    Some(WordOrder::CDAB)
                ),
                (
                    14,
                    "Area".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    15,
                    "Broken".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    16,
                    "Wide".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    24,
                    "Zero".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    25,
                    "Negative zero".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    26,
                    "Underflow".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
                (
                    30,
                    "Scattered".to_string(),
                    CustomRepr::U16,
                    Some(WordOrder::ABCD)
                ),
            ]
        );
        for entry in &back.holdings {
            let rule = entry.custom.as_ref().unwrap();
            assert!(rule.ops.is_empty(), "{}", rule.address);
            assert!(rule.suffix.is_empty(), "{}", rule.address);
            assert_eq!(rule.decimals, None, "{}", rule.address);
            assert!(rule.scale.is_none() && rule.enum_map.is_empty() && rule.bits.is_empty());
            assert!(rule.next.is_empty());
        }
    }

    #[test]
    fn unlabeled_pinned_and_bit_entries_follow_the_format_limits() {
        let registers = Registers {
            holdings: vec![
                RegisterEntry {
                    pinned: true,
                    ..entry(1, None, None)
                },
                RegisterEntry {
                    pinned: true,
                    ..entry(2, Some("Setpoint"), None)
                },
                entry(
                    3,
                    None,
                    Some(rule(3, CustomRepr::I32, Some(WordOrder::ABCD))),
                ),
            ],
            inputs: vec![entry(4, Some("  "), Some(rule(4, CustomRepr::U16, None)))],
            coils: vec![
                RegisterEntry {
                    pinned: true,
                    ..entry(5, None, None)
                },
                entry(6, None, Some(rule(6, CustomRepr::F32, None))),
            ],
            discretes: vec![entry(8, Some("Door"), Some(rule(8, CustomRepr::U16, None)))],
        };
        let back = round_trip(&config(tcp(2, WordOrder::ABCD), registers));
        let labels: Vec<_> = back
            .entries()
            .map(|((kind, address), e)| {
                (
                    kind,
                    address,
                    e.label.clone().unwrap(),
                    e.pinned,
                    e.custom.as_ref().map(|r| r.repr),
                )
            })
            .collect();
        use crate::register::RegisterType::{Coil, Discrete, Holding, Input};
        assert_eq!(
            labels,
            [
                (
                    Holding,
                    2,
                    "Setpoint".to_string(),
                    false,
                    Some(CustomRepr::U16)
                ),
                (
                    Holding,
                    3,
                    "Holding 3".to_string(),
                    false,
                    Some(CustomRepr::I32)
                ),
                (
                    Input,
                    4,
                    "Input 4".to_string(),
                    false,
                    Some(CustomRepr::U16)
                ),
                (Coil, 6, "Coil 6".to_string(), false, None),
                (Discrete, 8, "Door".to_string(), false, None),
            ]
        );
    }

    #[test]
    fn duplicate_names_are_numbered_per_entity_kind() {
        let registers = Registers {
            holdings: vec![entry(1, Some("Temp"), None), entry(2, Some("Temp"), None)],
            inputs: vec![entry(1, Some("Temp"), None), entry(3, Some("Temp 2"), None)],
            coils: vec![entry(1, Some("Temp"), None)],
            discretes: vec![entry(1, Some("Temp"), None)],
        };
        let text = export(&config(tcp(1, WordOrder::ABCD), registers));
        let hub = hub(&text);
        let names = |key: &str| -> Vec<String> {
            hub[key]
                .as_sequence()
                .unwrap()
                .iter()
                .map(|e| e["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names("sensors"), ["Temp", "Temp 2", "Temp 3", "Temp 2 2"]);
        assert_eq!(names("binary_sensors"), ["Temp", "Temp 2"]);
    }

    #[test]
    fn connections_map_to_the_hub_type() {
        let serial = DeviceConfig {
            interface: Interface::Serial(InterfaceSerialParams {
                path: "/dev/ttyUSB0".to_string(),
                baud_rate: 19200,
                data_bits: DataBits::Seven,
                parity: Parity::Even,
                stop_bits: StopBits::Two,
            }),
            request_timeout_ms: 2500,
            request_gap_ms: 40,
            ..DeviceConfig::default()
        };
        let rtu_over_tcp = DeviceConfig {
            interface: Interface::RtuOverTcp(InterfaceTcpParams {
                ip: "gateway.local".to_string(),
                port: 4196,
            }),
            request_timeout_ms: 300,
            ..DeviceConfig::default()
        };
        let cases = [
            (
                tcp(1, WordOrder::ABCD),
                "inverter",
                "name: \"inverter\"\n  type: tcp\n  host: \"10.0.0.5\"\n  port: 1502\n  timeout: 2\n",
            ),
            (
                serial,
                "inverter",
                "name: \"inverter\"\n  type: serial\n  port: \"/dev/ttyUSB0\"\n  baudrate: 19200\n  bytesize: 7\n  parity: E\n  stopbits: 2\n  method: rtu\n  timeout: 3\n  message_wait_milliseconds: 40\n",
            ),
            (
                rtu_over_tcp,
                " ",
                "name: \"mtui\"\n  type: rtuovertcp\n  host: \"gateway.local\"\n  port: 4196\n  timeout: 1\n",
            ),
            (
                DeviceConfig::default(),
                "inverter",
                "name: \"inverter\"\n  type: tcp\n  host: \"127.0.0.1\"\n  port: 502\n  timeout: 2\n",
            ),
        ];
        for (device, name, expected) in cases {
            let config = Config {
                name: name.to_string(),
                ..config(device, Registers::default())
            };
            assert_eq!(export(&config), format!("modbus:\n- {expected}"));
        }
    }

    #[test]
    fn sensors_are_written_with_valid_home_assistant_keys() {
        let voltage = CustomRule {
            ops: vec![op(OpKind::Mul, 0.1)],
            decimals: Some(1),
            suffix: " V".to_string(),
            ..rule(16, CustomRepr::U16, None)
        };
        let registers = Registers {
            holdings: vec![entry(
                20,
                Some("Power"),
                Some(rule(20, CustomRepr::F32, None)),
            )],
            inputs: vec![
                entry(16, Some("Voltage"), Some(voltage)),
                entry(
                    30,
                    Some("Serial"),
                    Some(CustomRule {
                        registers: Some(4),
                        ..rule(30, CustomRepr::Ascii, Some(WordOrder::BADC))
                    }),
                ),
            ],
            coils: vec![],
            discretes: vec![entry(7, Some("Alarm"), None)],
        };
        let mut shuffled = registers.clone();
        shuffled.inputs.reverse();
        let text = export(&config(tcp(3, WordOrder::DCBA), registers));
        let expected = r#"  sensors:
  - name: "Power"
    slave: 3
    address: 20
    input_type: holding
    data_type: float32
    swap: word_byte
    precision: 3
  - name: "Voltage"
    slave: 3
    address: 16
    input_type: input
    data_type: uint16
    swap: byte
    scale: 0.1
    precision: 1
    unit_of_measurement: "V"
  - name: "Serial"
    slave: 3
    address: 30
    input_type: input
    data_type: string
    count: 4
    swap: byte
  binary_sensors:
  - name: "Alarm"
    slave: 3
    address: 7
    input_type: discrete_input
"#;
        assert!(text.ends_with(expected), "{text}");
        assert_eq!(text, export(&config(tcp(3, WordOrder::DCBA), shuffled)));
    }

    #[test]
    fn nothing_to_export_still_yields_the_hub() {
        let registers = Registers {
            holdings: vec![RegisterEntry {
                pinned: true,
                ..entry(1, None, None)
            }],
            ..Registers::default()
        };
        let text = export(&config(tcp(1, WordOrder::ABCD), registers));
        assert!(parse(&text).is_none());
        assert_eq!(hub(&text)["type"].as_str(), Some("tcp"));
        assert!(hub(&text).get("sensors").is_none());
    }

    #[test]
    fn free_text_is_always_double_quoted() {
        let labels = [
            "on",
            "yes",
            "OFF",
            "1:30",
            "1_000",
            "2024-01-01",
            "=",
            "<<",
            "~",
            "say \"hi\" \\ bye",
            "line\nbreak\ttab\u{7f}\u{85}\u{2028}end",
            "Grüße °C",
        ];
        let holdings: Vec<_> = (0u16..)
            .zip(labels)
            .map(|(address, label)| {
                entry(
                    address,
                    Some(label),
                    Some(CustomRule {
                        suffix: format!(" {label}"),
                        ..rule(address, CustomRepr::U16, Some(WordOrder::ABCD))
                    }),
                )
            })
            .collect();
        let registers = Registers {
            holdings,
            coils: vec![entry(0, Some("no"), None)],
            ..Registers::default()
        };
        let device = DeviceConfig {
            interface: Interface::Serial(InterfaceSerialParams {
                path: "COM3".to_string(),
                baud_rate: 9600,
                data_bits: DataBits::Eight,
                parity: Parity::None,
                stop_bits: StopBits::One,
            }),
            ..DeviceConfig::default()
        };
        let config = Config {
            name: "yes".to_string(),
            ..config(device, registers.clone())
        };
        let text = export(&config);
        for quoted in [
            "- name: \"yes\"\n",
            "  port: \"COM3\"\n",
            "  - name: \"on\"\n",
            "    unit_of_measurement: \"on\"\n",
            "  - name: \"OFF\"\n",
            "  - name: \"1:30\"\n",
            "  - name: \"1_000\"\n",
            "  - name: \"2024-01-01\"\n",
            "  - name: \"=\"\n",
            "  - name: \"<<\"\n",
            "    unit_of_measurement: \"<<\"\n",
            "  - name: \"~\"\n",
            "  - name: \"say \\\"hi\\\" \\\\ bye\"\n",
            "  - name: \"line\\u000Abreak\\u0009tab\\u007F\\u0085\\u2028end\"\n",
            "  - name: \"Grüße °C\"\n",
            "  - name: \"no\"\n",
        ] {
            assert!(text.contains(quoted), "{quoted}\n{text}");
        }
        assert!(text.lines().all(|line| !line.contains(['\t', '\u{7f}'])));
        let back = round_trip(&config);
        assert_eq!(back.holdings, registers.holdings);
        assert_eq!(back.coils, registers.coils);
    }

    #[test]
    fn precision_matches_what_mtui_shows_without_decimals() {
        let cases = [
            (CustomRepr::U16, vec![op(OpKind::Mul, 0.1)], None, Some(3)),
            (CustomRepr::I32, vec![op(OpKind::Add, 0.5)], None, Some(3)),
            (CustomRepr::U16, vec![op(OpKind::Div, 2.0)], None, Some(3)),
            (CustomRepr::U16, vec![op(OpKind::Mul, 10.0)], None, None),
            (CustomRepr::I16, vec![op(OpKind::Sub, 4.0)], None, None),
            (CustomRepr::U64, vec![], None, None),
            (CustomRepr::F16, vec![], None, Some(3)),
            (CustomRepr::F32, vec![op(OpKind::Mul, 2.0)], None, Some(3)),
            (CustomRepr::F64, vec![], None, Some(3)),
            (
                CustomRepr::U16,
                vec![op(OpKind::Mul, 0.1)],
                Some(1),
                Some(1),
            ),
            (CustomRepr::F32, vec![], Some(0), Some(0)),
            (CustomRepr::Ascii, vec![], None, None),
        ];
        let holdings: Vec<_> = (0u16..)
            .map(|i| i * 8)
            .zip(&cases)
            .map(|(address, (repr, ops, decimals, _))| {
                entry(
                    address,
                    None,
                    Some(CustomRule {
                        ops: ops.clone(),
                        decimals: *decimals,
                        ..rule(address, *repr, Some(WordOrder::ABCD))
                    }),
                )
            })
            .collect();
        let config = config(
            tcp(1, WordOrder::ABCD),
            Registers {
                holdings,
                ..Registers::default()
            },
        );
        let text = export(&config);
        let precisions: Vec<_> = hub(&text)["sensors"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|sensor| sensor.get("precision").and_then(Value::as_u64))
            .collect();
        let expected: Vec<_> = cases
            .iter()
            .map(|(_, _, _, precision)| precision.map(u64::from))
            .collect();
        assert_eq!(precisions, expected);
        let back = round_trip(&config);
        let decimals: Vec<_> = back
            .holdings
            .iter()
            .map(|e| e.custom.as_ref().unwrap().decimals)
            .collect();
        let expected: Vec<_> = cases.iter().map(|case| case.3).collect();
        assert_eq!(decimals, expected);
        let tenth = back.holdings[0].custom.as_ref().unwrap();
        assert_eq!(tenth.evaluate(&[2305], WordOrder::ABCD), "230.500");
        assert_eq!(
            config.registers.holdings[0]
                .custom
                .as_ref()
                .unwrap()
                .evaluate(&[2305], WordOrder::ABCD),
            "230.5"
        );
    }
}
