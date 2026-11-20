use super::Device;
use crate::config::Registers;
use crate::custom::{
    ASCII_REGISTERS_MAX, BitEntry, CustomOp, CustomRepr, CustomRule, CustomScale, EnumEntry,
    OpKind, ScaleMode, ScaleRepr,
};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::Deserialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub const MARKER: [u16; 2] = [0x5375, 0x6E53];
pub const BASE_ADDRESSES: [u16; 3] = [40000, 0, 50000];
pub const END_MODEL: u16 = 0xFFFF;
pub const DEFAULT_START: u16 = 40002;

const ADDRESS_SPACE: usize = 0x10000;
const HEADER_REGISTERS: usize = 2;
const NOT_IMPLEMENTED: &str = "Not implemented";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: u16,
    pub label: String,
    pub description: String,
    pub registers: usize,
}

#[derive(Deserialize)]
struct Model {
    id: u16,
    group: Group,
    label: Option<String>,
    desc: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum GroupKind {
    Group,
    Sync,
}

#[derive(Deserialize)]
struct Group {
    name: String,
    #[serde(rename = "type")]
    _kind: GroupKind,
    label: Option<String>,
    desc: Option<String>,
    count: Option<Value>,
    #[serde(default)]
    points: Vec<Point>,
    #[serde(default)]
    groups: Vec<Self>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Repeat {
    Once,
    Fixed(usize),
    Variable,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Sf {
    Exponent(i32),
    Point(String),
}

#[derive(Deserialize)]
struct Symbol {
    name: String,
    label: Option<String>,
    value: Value,
}

#[derive(Deserialize)]
struct Point {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    size: u16,
    sf: Option<Sf>,
    units: Option<String>,
    label: Option<String>,
    #[serde(default)]
    symbols: Vec<Symbol>,
}

type Scope<'a> = BTreeMap<&'a str, (u16, &'a Point)>;

fn text(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}

fn humanize(name: &str) -> String {
    let words = name.replace(['_', '-'], " ");
    let mut chars = words.chars();
    chars
        .next()
        .map_or_default(|first| first.to_uppercase().chain(chars).collect::<String>())
}

fn repr(kind: &str) -> Option<CustomRepr> {
    Some(match kind {
        "uint16" | "acc16" | "enum16" | "bitfield16" | "count" | "raw16" => CustomRepr::U16,
        "int16" | "sunssf" => CustomRepr::I16,
        "uint32" | "acc32" | "enum32" | "bitfield32" => CustomRepr::U32,
        "int32" => CustomRepr::I32,
        "float32" => CustomRepr::F32,
        "uint64" | "acc64" | "bitfield64" => CustomRepr::U64,
        "int64" => CustomRepr::I64,
        "float64" => CustomRepr::F64,
        "string" => CustomRepr::Ascii,
        _ => return None,
    })
}

fn exponent_op(exponent: i32) -> Option<CustomOp> {
    let factor = 10f64.powi(exponent.saturating_abs());
    match exponent.cmp(&0) {
        Ordering::Greater => Some(CustomOp {
            op: OpKind::Mul,
            v: factor,
        }),
        Ordering::Less => Some(CustomOp {
            op: OpKind::Div,
            v: factor,
        }),
        Ordering::Equal => None,
    }
}

fn unimplemented(repr: CustomRepr) -> EnumEntry {
    let width = repr.register_count() * 16;
    EnumEntry {
        value: i64::try_from((1u128 << width) - 1).unwrap_or(i64::MAX),
        text: NOT_IMPLEMENTED.to_string(),
    }
}

fn exponent(scopes: &[Scope<'_>], name: &str) -> Option<CustomScale> {
    let (register, point) = scopes.iter().rev().find_map(|scope| scope.get(name))?;
    let repr = match point.kind.as_str() {
        "sunssf" | "int16" => ScaleRepr::Sunspec,
        "uint16" | "count" => ScaleRepr::Plain(CustomRepr::U16),
        "int32" => ScaleRepr::Plain(CustomRepr::I32),
        "uint32" => ScaleRepr::Plain(CustomRepr::U32),
        "int64" => ScaleRepr::Plain(CustomRepr::I64),
        "uint64" => ScaleRepr::Plain(CustomRepr::U64),
        _ => return None,
    };
    (repr.register_count() == usize::from(point.size)).then_some(CustomScale {
        register: *register,
        mode: ScaleMode::Pow10,
        repr,
    })
}

impl Symbol {
    fn text(&self) -> String {
        text(self.label.as_deref())
            .unwrap_or(&self.name)
            .to_string()
    }
}

impl Point {
    fn unit(&self) -> Option<&str> {
        text(self.units.as_deref())
    }

    fn label(&self, prefix: &str) -> String {
        let name = text(self.label.as_deref()).unwrap_or(&self.name);
        let name = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix} {name}")
        };
        match self.unit() {
            Some(unit) => format!("{name} [{unit}]"),
            None => name,
        }
    }

    fn enum_map(&self, repr: CustomRepr) -> Vec<EnumEntry> {
        let mut entries: Vec<EnumEntry> = self
            .symbols
            .iter()
            .filter_map(|symbol| {
                Some(EnumEntry {
                    value: symbol.value.as_i64()?,
                    text: symbol.text(),
                })
            })
            .collect();
        let sentinel = unimplemented(repr);
        if entries.iter().all(|entry| entry.value != sentinel.value) {
            entries.push(sentinel);
        }
        entries
    }

    fn bits(&self, repr: CustomRepr) -> Vec<BitEntry> {
        let width = repr.register_count() * 16;
        self.symbols
            .iter()
            .filter_map(|symbol| {
                let bit = u8::try_from(symbol.value.as_u64()?).ok()?;
                (usize::from(bit) < width).then(|| BitEntry {
                    bit,
                    name: symbol.text(),
                })
            })
            .collect()
    }

    fn rule(&self, address: u16, scopes: &[Scope<'_>]) -> Option<CustomRule> {
        let repr = repr(&self.kind).filter(|&repr| {
            repr == CustomRepr::Ascii || repr.register_count() == usize::from(self.size)
        })?;
        let text = repr == CustomRepr::Ascii;
        let registers = if text {
            Some(u8::try_from(self.size.min(ASCII_REGISTERS_MAX.into())).ok()?)
        } else {
            None
        };
        let (scale, op) = match self.sf.as_ref().filter(|_| !text) {
            Some(Sf::Point(name)) => (Some(exponent(scopes, name)?), None),
            Some(&Sf::Exponent(value)) => (None, exponent_op(value)),
            None => (None, None),
        };
        let enumerated = self.kind.starts_with("enum");
        let bitfield = self.kind.starts_with("bitfield");
        Some(CustomRule {
            address,
            repr,
            registers,
            scale,
            ops: op.into_iter().collect(),
            enum_map: if enumerated {
                self.enum_map(repr)
            } else if bitfield {
                vec![unimplemented(repr)]
            } else {
                Vec::new()
            },
            bits: if bitfield {
                self.bits(repr)
            } else {
                Vec::new()
            },
            suffix: self.unit().map_or_default(|unit| format!(" {unit}")),
            word_order: Some(WordOrder::ABCD),
            ..Default::default()
        })
    }
}

impl Group {
    fn repeat(&self) -> Repeat {
        let Some(count) = &self.count else {
            return Repeat::Once;
        };
        match count.as_u64() {
            Some(0) | None => Repeat::Variable,
            Some(count) => Repeat::Fixed(usize::try_from(count).unwrap_or(usize::MAX)),
        }
    }

    fn repetitions(&self, variable: usize) -> usize {
        match self.repeat() {
            Repeat::Once => 1,
            Repeat::Fixed(count) => count,
            Repeat::Variable => variable,
        }
    }

    fn variable_groups(&self) -> usize {
        self.groups
            .iter()
            .map(|group| usize::from(group.repeat() == Repeat::Variable) + group.variable_groups())
            .sum()
    }

    fn registers(&self, variable: usize) -> usize {
        let points = self.points.iter().fold(0usize, |total, point| {
            total.saturating_add(point.size.into())
        });
        self.groups.iter().fold(points, |total, group| {
            total.saturating_add(
                group
                    .repetitions(variable)
                    .saturating_mul(group.registers(variable)),
            )
        })
    }

    fn title(&self) -> String {
        text(self.label.as_deref()).map_or_else(|| humanize(&self.name), str::to_string)
    }

    fn prefix(&self, parent: &str, repetition: usize) -> String {
        let title = match self.repeat() {
            Repeat::Once => self.title(),
            Repeat::Fixed(_) | Repeat::Variable => format!("{} {repetition}", self.title()),
        };
        if parent.is_empty() {
            title
        } else {
            format!("{parent} {title}")
        }
    }
}

impl Model {
    fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text.trim()).ok()
    }

    fn label(&self) -> String {
        text(self.label.as_deref())
            .or_else(|| text(self.group.label.as_deref()))
            .unwrap_or(&self.group.name)
            .to_string()
    }

    fn description(&self) -> String {
        text(self.desc.as_deref())
            .or_else(|| text(self.group.desc.as_deref()))
            .unwrap_or_default()
            .to_string()
    }

    fn variable(&self, length: Option<u16>) -> usize {
        let Some(length) = length.filter(|_| self.group.variable_groups() == 1) else {
            return 1;
        };
        let fixed = self.group.registers(0);
        let group = self.group.registers(1).saturating_sub(fixed);
        (usize::from(length) + HEADER_REGISTERS)
            .saturating_sub(fixed)
            .checked_div(group)
            .unwrap_or(1)
    }
}

struct Import<'a> {
    variable: usize,
    limit: usize,
    scopes: Vec<Scope<'a>>,
    labels: BTreeMap<RegisterCell, String>,
    rules: BTreeMap<RegisterCell, CustomRule>,
    replaced: usize,
}

impl<'a> Import<'a> {
    const fn new(variable: usize, limit: usize) -> Self {
        Self {
            variable,
            limit,
            scopes: Vec::new(),
            labels: BTreeMap::new(),
            rules: BTreeMap::new(),
            replaced: 0,
        }
    }

    fn label(&mut self, cell: RegisterCell, label: String) {
        if self.labels.insert(cell, label).is_some() {
            self.replaced += 1;
            self.rules.remove(&cell);
        }
    }

    fn point(&mut self, point: &Point, address: u16, prefix: &str) {
        if point.kind == "pad" {
            return;
        }
        let cell = (RegisterType::Holding, address);
        self.label(cell, point.label(prefix));
        if let Some(rule) = point.rule(address, &self.scopes) {
            self.rules.insert(cell, rule);
        }
    }

    fn group(&mut self, group: &'a Group, mut cursor: usize, prefix: &str) -> Option<usize> {
        let mut scope = Scope::new();
        let mut points = Vec::new();
        for point in group.points.iter().filter(|point| point.size > 0) {
            let end = cursor + usize::from(point.size);
            if end <= self.limit {
                let address = u16::try_from(cursor).ok()?;
                scope.entry(point.name.as_str()).or_insert((address, point));
                points.push((address, point));
            }
            cursor = end;
        }
        self.scopes.push(scope);
        for (address, point) in points {
            self.point(point, address, prefix);
        }
        for child in &group.groups {
            if child.registers(self.variable) == 0 {
                continue;
            }
            for repetition in 1..=child.repetitions(self.variable) {
                if cursor >= self.limit {
                    break;
                }
                cursor = self.group(child, cursor, &child.prefix(prefix, repetition))?;
            }
        }
        self.scopes.pop();
        Some(cursor)
    }

    fn into_registers(self) -> Registers {
        let pinned: Vec<RegisterCell> = self.labels.keys().copied().collect();
        Registers::from_views(&pinned, &self.labels, &self.rules)
    }
}

pub fn model_info(text: &str) -> Option<ModelInfo> {
    let model = Model::parse(text)?;
    Some(ModelInfo {
        id: model.id,
        label: model.label(),
        description: model.description(),
        registers: model.group.registers(1),
    })
}

pub fn parse(text: &str, start: u16, length: Option<u16>) -> Option<Device> {
    let model = Model::parse(text)?;
    let variable = model.variable(length);
    let end = usize::from(start).saturating_add(model.group.registers(variable));
    if end > ADDRESS_SPACE {
        return None;
    }
    let limit = length.map_or(end, |length| {
        end.min(usize::from(start) + HEADER_REGISTERS + usize::from(length))
    });
    let mut import = Import::new(variable, limit);
    import.group(&model.group, start.into(), "")?;
    let replaced = import.replaced;
    let registers = import.into_registers();
    (registers.total() > 0).then(|| Device {
        name: format!("SunSpec {} {}", model.id, model.label()),
        registers,
        replaced,
    })
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_START, MARKER, ModelInfo, model_info, parse};
    use crate::config::{RegisterEntry, Registers};
    use crate::constants::UNINTERPRETABLE;
    use crate::custom::{CustomOp, CustomRepr, CustomScale, OpKind, ScaleMode, ScaleRepr};
    use crate::modbus::WordOrder;
    use serde_json::{Value, json};

    const MODEL_103: &str = r#"{"id": 103, "group":
{"label": "Inverter (Three Phase)", "desc": "Include this model for three phase inverter monitoring", "name": "inverter_three_phase", "type": "group", "points": [
    {"label": "Model ID", "name": "ID", "size": 1, "static": "S", "type": "uint16", "value": 103},
    {"label": "Model Length", "name": "L", "size": 1, "static": "S", "type": "uint16"},
    {"label": "Amps", "name": "A", "sf": "A_SF", "size": 1, "type": "uint16", "units": "A"},
    {"label": "Amps PhaseA", "name": "AphA", "sf": "A_SF", "size": 1, "type": "uint16", "units": "A"},
    {"label": "Amps PhaseB", "name": "AphB", "sf": "A_SF", "size": 1, "type": "uint16", "units": "A"},
    {"label": "Amps PhaseC", "name": "AphC", "sf": "A_SF", "size": 1, "type": "uint16", "units": "A"},
    {"name": "A_SF", "size": 1, "type": "sunssf"},
    {"label": "Watts", "name": "W", "sf": "W_SF", "size": 1, "type": "int16", "units": "W"},
    {"name": "W_SF", "size": 1, "type": "sunssf"},
    {"label": "WattHours", "name": "WH", "sf": "WH_SF", "size": 2, "type": "acc32", "units": "Wh"},
    {"name": "WH_SF", "size": 1, "type": "sunssf"},
    {"label": "Operating State", "name": "St", "size": 1, "symbols": [{"name": "OFF", "value": 1}, {"name": "SLEEPING", "value": 2}, {"name": "STARTING", "value": 3}, {"name": "MPPT", "value": 4}, {"name": "THROTTLED", "value": 5}, {"name": "SHUTTING_DOWN", "value": 6}, {"name": "FAULT", "value": 7}, {"name": "STANDBY", "value": 8}], "type": "enum16"},
    {"label": "Event1", "name": "Evt1", "size": 2, "symbols": [{"name": "GROUND_FAULT", "value": 0}, {"name": "DC_OVER_VOLT", "value": 1}, {"name": "AC_DISCONNECT", "value": 2}, {"name": "DC_DISCONNECT", "value": 3}, {"name": "OVER_TEMP", "value": 7}, {"name": "HW_TEST_FAILURE", "value": 15}], "type": "bitfield32"}
]}
}"#;

    const MODEL_160: &str = r#"{"id": 160, "group":
{"label": "Multiple MPPT Inverter Extension Model", "name": "mppt", "type": "group", "points": [
    {"label": "Model ID", "name": "ID", "size": 1, "static": "S", "type": "uint16", "value": 160},
    {"label": "Model Length", "name": "L", "size": 1, "static": "S", "type": "uint16"},
    {"label": "Current Scale Factor", "name": "DCA_SF", "size": 1, "type": "sunssf"},
    {"label": "Voltage Scale Factor", "name": "DCV_SF", "size": 1, "type": "sunssf"},
    {"label": "Power Scale Factor", "name": "DCW_SF", "size": 1, "type": "sunssf"},
    {"label": "Energy Scale Factor", "name": "DCWH_SF", "size": 1, "type": "sunssf"},
    {"label": "Global Events", "name": "Evt", "size": 2, "symbols": [{"name": "GROUND_FAULT", "value": 0}, {"name": "INPUT_OVER_VOLTAGE", "value": 1}, {"name": "INPUT_OVER_CURRENT", "value": 22}], "type": "bitfield32"},
    {"label": "Number of Modules", "name": "N", "size": 1, "type": "count"},
    {"label": "Timestamp Period", "name": "TmsPer", "size": 1, "type": "uint16"}
], "groups": [
    {"count": 0, "name": "module", "type": "group", "points": [
        {"label": "Input ID", "name": "ID", "size": 1, "type": "uint16"},
        {"label": "Input ID String", "name": "IDStr", "size": 8, "type": "string"},
        {"label": "DC Current", "name": "DCA", "sf": "DCA_SF", "size": 1, "type": "uint16", "units": "A"},
        {"label": "DC Voltage", "name": "DCV", "sf": "DCV_SF", "size": 1, "type": "uint16", "units": "V"},
        {"label": "DC Power", "name": "DCW", "sf": "DCW_SF", "size": 1, "type": "uint16", "units": "W"},
        {"label": "Lifetime Energy", "name": "DCWH", "sf": "DCWH_SF", "size": 2, "type": "acc32", "units": "Wh"},
        {"label": "Timestamp", "name": "Tms", "size": 2, "type": "uint32", "units": "Secs"},
        {"label": "Temperature", "name": "Tmp", "size": 1, "type": "int16", "units": "C"},
        {"label": "Operating State", "name": "DCSt", "size": 1, "symbols": [{"name": "OFF", "value": 1}, {"name": "MPPT", "value": 4}, {"name": "FAULT", "value": 7}], "type": "enum16"},
        {"label": "Module Events", "name": "DCEvt", "size": 2, "symbols": [{"name": "GROUND_FAULT", "value": 0}, {"name": "ARC_DETECTION", "value": 15}, {"name": "INPUT_OVER_CURRENT", "value": 22}], "type": "bitfield32"}
    ]}
]}
}"#;

    const MODEL_705: &str = r#"{"id": 705, "group":
{"label": "DER Volt-Var", "name": "DERVoltVar", "type": "group", "points": [
    {"label": "Model ID", "name": "ID", "size": 1, "static": "S", "type": "uint16", "value": 705},
    {"label": "Model Length", "name": "L", "size": 1, "static": "S", "type": "uint16"},
    {"access": "RW", "label": "DER Volt-Var Module Enable", "name": "Ena", "size": 1, "symbols": [{"label": "Disabled", "name": "DISABLED", "value": 0}, {"label": "Enabled", "name": "ENABLED", "value": 1}], "type": "enum16"},
    {"label": "Number Of Points", "name": "NPt", "size": 1, "static": "S", "type": "uint16"},
    {"label": "Stored Curve Count", "name": "NCrv", "size": 1, "static": "S", "type": "uint16"},
    {"label": "Voltage Scale Factor", "name": "V_SF", "size": 1, "static": "S", "type": "sunssf"},
    {"label": "Var Scale Factor", "name": "DeptRef_SF", "size": 1, "static": "S", "type": "sunssf"}
], "groups": [
    {"count": "NCrv", "label": "Stored Curves", "name": "Crv", "type": "group", "points": [
        {"access": "RW", "label": "Active Points", "name": "ActPt", "size": 1, "type": "uint16"},
        {"access": "RW", "label": "Dependent Reference", "name": "DeptRef", "size": 1, "symbols": [{"label": "Percent Max Watts", "name": "W_MAX_PCT", "value": 0}, {"label": "Percent Max Vars", "name": "VAR_MAX_PCT", "value": 1}], "type": "enum16"},
        {"access": "RW", "label": "Vref Adjustment", "name": "VRef", "sf": "V_SF", "size": 1, "type": "uint16", "units": "VNomPct"}
    ], "groups": [
        {"count": "NPt", "label": "Stored Curve Points", "name": "Pt", "type": "group", "points": [
            {"access": "RW", "label": "Voltage Point", "name": "V", "sf": "V_SF", "size": 1, "type": "uint16", "units": "VNomPct"},
            {"access": "RW", "label": "Reactive Power Point", "name": "Var", "sf": "DeptRef_SF", "size": 1, "type": "int16", "units": "DeptRef"}
        ]}
    ]}
]}
}"#;

    fn entry(registers: &Registers, address: u16) -> &RegisterEntry {
        registers
            .holdings
            .iter()
            .find(|e| e.address == address)
            .unwrap_or_else(|| panic!("no register at {address}"))
    }

    fn label(registers: &Registers, address: u16) -> Option<&str> {
        entry(registers, address).label.as_deref()
    }

    fn model(points: Value) -> String {
        json!({"id": 64999, "group": {"name": "custom", "type": "group", "points": points}})
            .to_string()
    }

    #[test]
    fn marker_spells_suns() {
        let bytes: Vec<u8> = MARKER.iter().flat_map(|w| w.to_be_bytes()).collect();
        assert_eq!(bytes, b"SunS");
    }

    #[test]
    fn describes_the_real_inverter_and_mppt_models() {
        assert_eq!(
            model_info(MODEL_103),
            Some(ModelInfo {
                id: 103,
                label: "Inverter (Three Phase)".to_string(),
                description: "Include this model for three phase inverter monitoring".to_string(),
                registers: 15,
            })
        );
        let mppt = model_info(MODEL_160).unwrap();
        assert_eq!(
            (mppt.id, mppt.description.as_str(), mppt.registers),
            (160, "", 30)
        );
        assert_eq!(model_info(MODEL_705).unwrap().registers, 12);
    }

    #[test]
    fn imports_the_real_three_phase_inverter_model() {
        let start = DEFAULT_START + 68;
        let device = parse(MODEL_103, start, None).unwrap();
        assert_eq!(device.name, "SunSpec 103 Inverter (Three Phase)");
        assert_eq!(device.replaced, 0);
        let r = &device.registers;
        assert_eq!((r.pins(), r.labels(), r.holdings.len()), (13, 13, 13));
        assert!(r.holdings.iter().all(|e| e.pinned));
        assert_eq!(label(r, start), Some("Model ID"));
        assert_eq!(label(r, start + 2), Some("Amps [A]"));
        assert_eq!(label(r, start + 6), Some("A_SF"));

        let amps = entry(r, start + 2).custom.as_ref().unwrap();
        assert_eq!(
            amps.scale,
            Some(CustomScale {
                register: start + 6,
                mode: ScaleMode::Pow10,
                repr: ScaleRepr::Sunspec,
            })
        );
        assert_eq!(
            (amps.repr, amps.word_order, amps.suffix.as_str()),
            (CustomRepr::U16, Some(WordOrder::ABCD), " A")
        );
        assert_eq!(amps.evaluate(&[1234, 0xFFFE], WordOrder::DCBA), "12.34 A");
        assert_eq!(
            amps.evaluate(&[1234, 0x8000], WordOrder::ABCD),
            format!("{UNINTERPRETABLE} A")
        );

        let watts = entry(r, start + 7).custom.as_ref().unwrap();
        assert_eq!(
            watts.evaluate(&[(-15i16) as u16, 2], WordOrder::ABCD),
            "-1500 W"
        );
        let energy = entry(r, start + 9).custom.as_ref().unwrap();
        assert_eq!(energy.scale.map(|s| s.register), Some(start + 11));
        assert_eq!(
            energy.evaluate(&[1, 0, 0xFFFF], WordOrder::CDAB),
            "6553.6 Wh"
        );
        assert!(r.holdings.iter().all(|e| e.address != start + 10));

        let state = entry(r, start + 12).custom.as_ref().unwrap();
        assert_eq!(state.evaluate(&[4], WordOrder::ABCD), "MPPT");
        let events = entry(r, start + 13).custom.as_ref().unwrap();
        assert_eq!(events.repr, CustomRepr::U32);
        assert_eq!(
            events.evaluate(&[0, 0b1000_1001], WordOrder::ABCD),
            "GROUND_FAULT|DC_DISCONNECT|OVER_TEMP"
        );
    }

    #[test]
    fn repeats_the_real_mppt_module_group_by_the_model_length() {
        let start = 40122;
        let once = parse(MODEL_160, start, None).unwrap();
        assert_eq!(
            once.name,
            "SunSpec 160 Multiple MPPT Inverter Extension Model"
        );
        assert_eq!(once.registers.holdings.len(), 9 + 10);
        assert_eq!(
            label(&once.registers, start + 19),
            Some("Module 1 DC Current [A]")
        );

        let device = parse(MODEL_160, start, Some(8 + 3 * 20)).unwrap();
        let r = &device.registers;
        assert_eq!(r.holdings.len(), 9 + 3 * 10);
        assert_eq!(label(r, start + 10), Some("Module 1 Input ID"));
        assert_eq!(label(r, start + 39), Some("Module 2 DC Current [A]"));
        assert_eq!(label(r, start + 68), Some("Module 3 Module Events"));
        let current = entry(r, start + 59).custom.as_ref().unwrap();
        assert_eq!(current.scale.map(|s| s.register), Some(start + 2));
        assert_eq!(current.evaluate(&[57, 0xFFFF], WordOrder::ABCD), "5.7 A");

        let id = entry(r, start + 31).custom.as_ref().unwrap();
        assert_eq!((id.repr, id.registers), (CustomRepr::Ascii, Some(8)));
        assert_eq!(
            id.evaluate(&[0x4D50, 0x5054, 0x2032, 0, 0, 0, 0, 0], WordOrder::DCBA),
            "MPPT 2"
        );
        let state = entry(r, start + 47).custom.as_ref().unwrap();
        assert_eq!(state.evaluate(&[7], WordOrder::ABCD), "FAULT");

        let short = parse(MODEL_160, start, Some(4)).unwrap();
        assert_eq!(short.registers.holdings.len(), 6);
        let partial = parse(MODEL_160, start, Some(8 + 2 * 20 + 7)).unwrap();
        assert_eq!(partial.registers.holdings.len(), 9 + 2 * 10);
    }

    #[test]
    fn nested_repeating_groups_import_once_and_scale_from_outer_points() {
        let start = 40300;
        let device = parse(MODEL_705, start, Some(200)).unwrap();
        let r = &device.registers;
        assert_eq!(r.holdings.len(), 12);
        assert_eq!(label(r, start + 7), Some("Stored Curves 1 Active Points"));
        assert_eq!(
            label(r, start + 10),
            Some("Stored Curves 1 Stored Curve Points 1 Voltage Point [VNomPct]")
        );
        let enable = entry(r, start + 2).custom.as_ref().unwrap();
        assert_eq!(enable.evaluate(&[1], WordOrder::ABCD), "Enabled");
        let point = entry(r, start + 10).custom.as_ref().unwrap();
        assert_eq!(point.scale.map(|s| s.register), Some(start + 5));
        let var = entry(r, start + 11).custom.as_ref().unwrap();
        assert_eq!(var.scale.map(|s| s.register), Some(start + 6));
        assert_eq!(
            var.evaluate(&[(-250i16) as u16, 0xFFFF], WordOrder::ABCD),
            "-25 DeptRef"
        );
    }

    #[test]
    fn literal_scale_factors_become_operations_and_odd_types_stay_labels() {
        let device = parse(
            &model(json!([
                {"name": "ID", "type": "uint16", "size": 1},
                {"name": "L", "type": "uint16", "size": 1},
                {"name": "Lat", "label": "Latitude", "type": "int32", "size": 2, "sf": -7, "units": "Degrees"},
                {"name": "Big", "type": "uint32", "size": 2, "sf": 1},
                {"name": "Pad", "type": "pad", "size": 1},
                {"name": "Ip", "label": "IP Address", "type": "ipaddr", "size": 2},
                {"name": "Lost", "type": "int16", "size": 1, "sf": "Missing_SF"},
                {"name": "Wide", "type": "uint16", "size": 2},
                {"name": "Huge", "type": "string", "size": 150},
                {"name": "Mac", "type": "eui48", "size": 4},
                {"name": "F", "type": "float64", "size": 4},
            ])),
            0,
            None,
        )
        .unwrap();
        let r = &device.registers;
        let latitude = entry(r, 2).custom.as_ref().unwrap();
        assert_eq!(
            latitude.ops,
            [CustomOp {
                op: OpKind::Div,
                v: 1e7
            }]
        );
        assert_eq!(
            latitude.evaluate(&[0x1A13, 0xB860], WordOrder::ABCD),
            "43.75 Degrees"
        );
        let big = entry(r, 4).custom.as_ref().unwrap();
        assert_eq!(big.evaluate(&[0, 7], WordOrder::ABCD), "70");
        assert!(r.holdings.iter().all(|e| e.address != 6));
        assert_eq!(label(r, 7), Some("IP Address"));
        for address in [7, 9, 10, 162] {
            assert!(entry(r, address).custom.is_none(), "{address}");
        }
        assert_eq!(entry(r, 12).custom.as_ref().unwrap().registers, Some(125));
        assert_eq!(entry(r, 166).custom.as_ref().unwrap().repr, CustomRepr::F64);
        assert_eq!(device.name, "SunSpec 64999 custom");
        assert_eq!(model_info(&model(json!([]))).unwrap().label, "custom");
    }

    #[test]
    fn unimplemented_enums_and_bitfields_read_as_not_implemented() {
        let device = parse(MODEL_103, 0, None).unwrap();
        let r = &device.registers;
        let events = entry(r, 13).custom.as_ref().unwrap();
        assert_eq!(
            events.evaluate(&[0xFFFF, 0xFFFF], WordOrder::ABCD),
            "Not implemented"
        );
        assert_eq!(
            events.evaluate(&[0xFFFF, 0xFFFE], WordOrder::ABCD),
            "DC_OVER_VOLT|AC_DISCONNECT|DC_DISCONNECT|OVER_TEMP|HW_TEST_FAILURE"
        );
        let state = entry(r, 12).custom.as_ref().unwrap();
        assert_eq!(
            state.evaluate(&[0xFFFF], WordOrder::ABCD),
            "Not implemented"
        );

        let custom = parse(
            &model(json!([
                {"name": "Flags", "type": "bitfield16", "size": 1, "symbols": [{"name": "ON", "value": 0}]},
                {"name": "Wide", "type": "bitfield64", "size": 4, "symbols": [{"name": "ON", "value": 0}]},
                {"name": "Mode", "type": "enum32", "size": 2, "symbols": [{"name": "ALL", "value": 4294967295_u64}]},
            ])),
            0,
            None,
        )
        .unwrap();
        let r = &custom.registers;
        let flags = entry(r, 0).custom.as_ref().unwrap();
        assert_eq!(
            flags.evaluate(&[0xFFFF], WordOrder::ABCD),
            "Not implemented"
        );
        assert_eq!(flags.evaluate(&[1], WordOrder::ABCD), "ON");
        let wide = entry(r, 1).custom.as_ref().unwrap();
        assert_eq!(
            wide.evaluate(&[0xFFFF; 4], WordOrder::ABCD),
            "Not implemented"
        );
        assert_eq!(wide.evaluate(&[0, 0, 0, 1], WordOrder::ABCD), "ON");
        let mode = entry(r, 5).custom.as_ref().unwrap();
        assert_eq!(mode.enum_map.len(), 1);
        assert_eq!(mode.evaluate(&[0xFFFF, 0xFFFF], WordOrder::ABCD), "ALL");
    }

    #[test]
    fn huge_counts_of_empty_groups_finish_without_importing_them() {
        let device = parse(
            &json!({"id": 1, "group": {"name": "g", "type": "group", "points": [
                {"name": "ID", "type": "uint16", "size": 1},
                {"name": "L", "type": "uint16", "size": 1},
            ], "groups": [
                {"name": "empty", "type": "group", "count": u64::MAX},
                {"name": "zero", "type": "group", "count": 4_000_000_000_u64, "points": [
                    {"name": "Nothing", "type": "uint16", "size": 0},
                ]},
                {"name": "outer", "type": "group", "count": 4_000_000_000_u64, "groups": [
                    {"name": "inner", "type": "group", "count": 0, "points": [
                        {"name": "X", "type": "uint16", "size": 1},
                    ]},
                ]},
            ]}})
            .to_string(),
            0,
            Some(0),
        )
        .unwrap();
        assert_eq!(device.registers.holdings.len(), 2);
    }

    #[test]
    fn a_known_model_length_stops_labels_at_the_end_of_the_model() {
        let text = model(json!([
            {"name": "ID", "type": "uint16", "size": 1},
            {"name": "L", "type": "uint16", "size": 1},
            {"name": "A", "type": "uint16", "size": 1, "sf": "A_SF"},
            {"name": "B", "type": "uint32", "size": 2},
            {"name": "A_SF", "type": "sunssf", "size": 1},
        ]));
        let device = parse(&text, 100, Some(2)).unwrap();
        let r = &device.registers;
        assert_eq!(r.holdings.len(), 3);
        assert_eq!(label(r, 102), Some("A"));
        assert!(entry(r, 102).custom.is_none());
        assert_eq!(
            parse(&text, 100, Some(4)).unwrap().registers.holdings.len(),
            5
        );
        assert_eq!(
            parse(&text, 100, Some(40))
                .unwrap()
                .registers
                .holdings
                .len(),
            5
        );

        let mppt = parse(MODEL_160, 0, Some(4)).unwrap();
        assert!(mppt.registers.holdings.iter().all(|e| e.address < 6));
    }

    #[test]
    fn rejects_other_formats_and_overflowing_addresses() {
        assert!(parse(MODEL_103, 65521, None).is_some());
        assert!(parse(MODEL_103, 65522, None).is_none());
        assert!(parse(MODEL_160, 65000, Some(4000)).is_none());
        assert!(parse(r#"{"mbmap": 2, "registers": []}"#, 0, None).is_none());
        assert!(parse("modbus:\n  - name: hub\n", 0, None).is_none());
        assert!(
            parse(
                r#"{"id": 1, "group": {"name": "g", "type": "table"}}"#,
                0,
                None
            )
            .is_none()
        );
        assert!(
            parse(
                &model(json!([{"name": "Pad", "type": "pad", "size": 1}])),
                0,
                None
            )
            .is_none()
        );
        assert!(model_info(r#"{"id": 70000, "group": {"name": "g", "type": "group"}}"#).is_none());
    }
}
