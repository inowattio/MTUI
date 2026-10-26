use crate::config::Registers;
use crate::custom::{CustomOp, CustomRepr, CustomRule, OpKind};
use crate::modbus::WordOrder;
use crate::register::{RegisterCell, RegisterType};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_norway::Value;
use std::collections::BTreeMap;

#[derive(Default, Deserialize)]
#[serde(default)]
struct Hub {
    sensors: Vec<Value>,
    binary_sensors: Vec<Value>,
    switches: Vec<Value>,
    lights: Vec<Value>,
    fans: Vec<Value>,
    climates: Vec<Value>,
    covers: Vec<Value>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Table {
    #[serde(alias = "holdings")]
    Holding,
    Input,
    #[serde(alias = "coils")]
    Coil,
    DiscreteInput,
}

impl From<Table> for RegisterType {
    fn from(table: Table) -> Self {
        match table {
            Table::Holding => Self::Holding,
            Table::Input => Self::Input,
            Table::Coil => Self::Coil,
            Table::DiscreteInput => Self::Discrete,
        }
    }
}

#[derive(Deserialize)]
struct Reading {
    name: String,
    address: u16,
    input_type: Option<Table>,
    data_type: Option<String>,
    count: Option<u8>,
    scale: Option<f64>,
    offset: Option<f64>,
    precision: Option<u8>,
    swap: Option<String>,
}

#[derive(Deserialize)]
struct Sensor {
    #[serde(flatten)]
    reading: Reading,
    unit_of_measurement: Option<String>,
    #[serde(alias = "slave_count")]
    virtual_count: Option<u16>,
}

#[derive(Deserialize)]
struct BinarySensor {
    name: String,
    address: u16,
    input_type: Option<Table>,
    #[serde(alias = "slave_count")]
    virtual_count: Option<u16>,
}

#[derive(Deserialize)]
struct Verify {
    address: Option<u16>,
    input_type: Option<Table>,
}

#[derive(Deserialize)]
struct Toggle {
    name: String,
    address: u16,
    write_type: Option<Table>,
    verify: Option<Verify>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Registers16 {
    One(u16),
    Many(Vec<u16>),
}

#[derive(Deserialize)]
struct Climate {
    #[serde(flatten)]
    reading: Reading,
    target_temp_register: Option<Registers16>,
    temperature_unit: Option<String>,
}

#[derive(Deserialize)]
struct Cover {
    name: String,
    address: u16,
    input_type: Option<Table>,
    status_register: Option<u16>,
    status_register_type: Option<Table>,
}

fn repr(data_type: Option<&str>) -> Option<CustomRepr> {
    Some(match data_type.unwrap_or("int16") {
        "int16" | "int" | "integer" => CustomRepr::I16,
        "uint16" | "uint" => CustomRepr::U16,
        "int32" => CustomRepr::I32,
        "uint32" => CustomRepr::U32,
        "int64" => CustomRepr::I64,
        "uint64" => CustomRepr::U64,
        "float16" => CustomRepr::F16,
        "float32" | "float" => CustomRepr::F32,
        "float64" => CustomRepr::F64,
        "string" => CustomRepr::Ascii,
        _ => return None,
    })
}

fn word_order(swap: Option<&str>) -> WordOrder {
    match swap {
        Some("byte") => WordOrder::BADC,
        Some("word") => WordOrder::CDAB,
        Some("word_byte") => WordOrder::DCBA,
        _ => WordOrder::ABCD,
    }
}

impl Reading {
    fn cell(&self) -> RegisterCell {
        (
            self.input_type.map_or(RegisterType::Holding, Into::into),
            self.address,
        )
    }

    fn rule(&self, unit: Option<&str>) -> Option<CustomRule> {
        let repr = repr(self.data_type.as_deref())?;
        let ascii = repr == CustomRepr::Ascii;
        let ops = [
            self.scale
                .filter(|&v| (v - 1.0).abs() > f64::EPSILON)
                .map(|v| CustomOp { op: OpKind::Mul, v }),
            self.offset
                .filter(|&v| v != 0.0)
                .map(|v| CustomOp { op: OpKind::Add, v }),
        ];
        Some(CustomRule {
            address: self.address,
            repr,
            registers: if ascii { self.count } else { None },
            ops: ops.into_iter().flatten().collect(),
            decimals: if ascii { None } else { self.precision },
            suffix: unit.map(|unit| format!(" {unit}")).unwrap_or_default(),
            word_order: (ascii || repr.register_count() > 1)
                .then(|| word_order(self.swap.as_deref())),
            ..Default::default()
        })
    }
}

#[derive(Default)]
struct Import {
    labels: BTreeMap<RegisterCell, String>,
    rules: BTreeMap<RegisterCell, CustomRule>,
}

fn entities<T: DeserializeOwned>(values: Vec<Value>) -> impl Iterator<Item = T> {
    values
        .into_iter()
        .filter_map(|value| serde_norway::from_value(value).ok())
}

fn repeated(name: &str, count: Option<u16>) -> impl Iterator<Item = (u16, String)> + '_ {
    (0..=count.unwrap_or(0)).map(move |i| match i {
        0 => (0, name.to_string()),
        i => (i, format!("{name} {i}")),
    })
}

impl Import {
    fn label(&mut self, cell: RegisterCell, name: impl Into<String>) {
        self.labels.insert(cell, name.into());
    }

    fn reading(&mut self, reading: &Reading, name: String, unit: Option<&str>) {
        let cell = reading.cell();
        self.label(cell, name);
        if let Some(rule) = reading.rule(unit) {
            self.rules.insert(cell, rule);
        }
    }

    fn sensor(&mut self, sensor: Sensor) {
        let Sensor {
            mut reading,
            unit_of_measurement,
            virtual_count,
        } = sensor;
        let width = reading
            .rule(None)
            .map_or(1, |rule| rule.register_count() as u16);
        let start = reading.address;
        let name = std::mem::take(&mut reading.name);
        for (i, name) in repeated(&name, virtual_count) {
            reading.address = start.wrapping_add(i.wrapping_mul(width));
            self.reading(&reading, name, unit_of_measurement.as_deref());
        }
    }

    fn binary_sensor(&mut self, sensor: &BinarySensor) {
        let kind = sensor.input_type.map_or(RegisterType::Coil, Into::into);
        for (i, name) in repeated(&sensor.name, sensor.virtual_count) {
            self.label((kind, sensor.address.wrapping_add(i)), name);
        }
    }

    fn toggle(&mut self, toggle: &Toggle) {
        let write = toggle.write_type.unwrap_or(Table::Holding);
        let cell = (write.into(), toggle.address);
        self.label(cell, &toggle.name);
        if let Some(verify) = &toggle.verify {
            let state = (
                verify.input_type.unwrap_or(write).into(),
                verify.address.unwrap_or(toggle.address),
            );
            if state != cell {
                self.label(state, format!("{} state", toggle.name));
            }
        }
    }

    fn climate(&mut self, climate: &Climate) {
        let name = &climate.reading.name;
        let unit = climate
            .temperature_unit
            .as_deref()
            .map_or("°C", |unit| if unit == "F" { "°F" } else { "°C" });
        self.reading(&climate.reading, format!("{name} temperature"), Some(unit));
        let targets = match &climate.target_temp_register {
            Some(Registers16::One(address)) => vec![*address],
            Some(Registers16::Many(addresses)) => addresses.clone(),
            None => Vec::new(),
        };
        for address in targets {
            self.label((RegisterType::Holding, address), format!("{name} target"));
        }
    }

    fn cover(&mut self, cover: &Cover) {
        let kind = cover.input_type.map_or(RegisterType::Holding, Into::into);
        self.label((kind, cover.address), &cover.name);
        if let Some(address) = cover.status_register {
            let kind = cover
                .status_register_type
                .map_or(RegisterType::Holding, Into::into);
            self.label((kind, address), format!("{} status", cover.name));
        }
    }

    fn hub(&mut self, hub: Hub) {
        for sensor in entities(hub.sensors) {
            self.sensor(sensor);
        }
        for sensor in entities::<BinarySensor>(hub.binary_sensors) {
            self.binary_sensor(&sensor);
        }
        for toggle in entities::<Toggle>([hub.switches, hub.lights, hub.fans].concat()) {
            self.toggle(&toggle);
        }
        for climate in entities::<Climate>(hub.climates) {
            self.climate(&climate);
        }
        for cover in entities::<Cover>(hub.covers) {
            self.cover(&cover);
        }
    }
}

pub fn parse(text: &str) -> Option<Registers> {
    let value = match serde_norway::from_str(text).ok()? {
        Value::Mapping(mut root) => root.remove("modbus").unwrap_or(Value::Mapping(root)),
        other => other,
    };
    let hubs = match value {
        Value::Sequence(hubs) => hubs,
        hub => vec![hub],
    };
    let mut import = Import::default();
    for hub in entities(hubs) {
        import.hub(hub);
    }
    let registers = Registers::from_views(&[], &import.labels, &import.rules);
    (registers.total() > 0).then_some(registers)
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::custom::{CustomOp, CustomRepr, OpKind};
    use crate::modbus::WordOrder;
    use crate::register::RegisterType::{Coil, Discrete, Holding, Input};

    #[test]
    fn sensors_become_labels_and_rules() {
        let registers = parse(
            "
modbus:
  - name: hub
    type: tcp
    host: !secret modbus_host
    port: 502
    sensors:
      - name: Voltage
        address: 0x10
        input_type: input
        data_type: uint16
        scale: 0.1
        offset: -5
        precision: 1
        unit_of_measurement: V
      - name: Power
        address: 20
        data_type: float32
        swap: word
      - name: Model
        address: 100
        data_type: string
        count: 6
        swap: byte
      - name: Blob
        address: 200
        data_type: custom
        structure: '>3h'
",
        )
        .expect("importable");

        let voltage = &registers.inputs[0];
        assert_eq!(voltage.address, 0x10);
        assert_eq!(voltage.label.as_deref(), Some("Voltage"));
        let rule = voltage.custom.as_ref().expect("rule");
        assert_eq!(rule.repr, CustomRepr::U16);
        assert_eq!(
            rule.ops,
            [
                CustomOp {
                    op: OpKind::Mul,
                    v: 0.1
                },
                CustomOp {
                    op: OpKind::Add,
                    v: -5.0
                }
            ]
        );
        assert_eq!(rule.decimals, Some(1));
        assert_eq!(rule.suffix, " V");
        assert_eq!(rule.word_order, None);

        let [power, model, blob] = &registers.holdings[..] else {
            panic!("three holdings: {:?}", registers.holdings);
        };
        let power = power.custom.as_ref().expect("rule");
        assert_eq!(power.repr, CustomRepr::F32);
        assert_eq!(power.word_order, Some(WordOrder::CDAB));
        let model = model.custom.as_ref().expect("rule");
        assert_eq!(model.repr, CustomRepr::Ascii);
        assert_eq!(model.registers, Some(6));
        assert_eq!(model.word_order, Some(WordOrder::BADC));
        assert_eq!(blob.label.as_deref(), Some("Blob"));
        assert!(blob.custom.is_none());
    }

    #[test]
    fn virtual_counts_repeat_by_the_register_width() {
        let registers = parse(
            "
- name: hub
  sensors:
    - name: Cell
      address: 10
      data_type: int32
      virtual_count: 2
  binary_sensors:
    - name: Alarm
      address: 4
      input_type: discrete_input
      slave_count: 1
",
        )
        .expect("importable");
        let holdings: Vec<_> = registers
            .holdings
            .iter()
            .map(|e| (e.address, e.label.as_deref().unwrap()))
            .collect();
        assert_eq!(holdings, [(10, "Cell"), (12, "Cell 1"), (14, "Cell 2")]);
        let discretes: Vec<_> = registers
            .discretes
            .iter()
            .map(|e| (e.address, e.label.as_deref().unwrap()))
            .collect();
        assert_eq!(discretes, [(4, "Alarm"), (5, "Alarm 1")]);
    }

    #[test]
    fn writable_entities_label_their_write_and_state_registers() {
        let registers = parse(
            "
name: hub
switches:
  - name: Pump
    address: 3
    write_type: coil
    verify:
      address: 7
      input_type: discrete_input
lights:
  - name: Lamp
    address: 5
    verify:
fans:
  - name: Fan
    address: 6
    write_type: holdings
climates:
  - name: Room
    address: 30
    input_type: input
    data_type: int16
    scale: 0.1
    temperature_unit: F
    target_temp_register: [40, 41]
covers:
  - name: Blind
    address: 50
    status_register: 51
    status_register_type: input
",
        )
        .expect("importable");
        let labels: Vec<_> = registers
            .entries()
            .map(|((kind, address), e)| (kind, address, e.label.clone().unwrap()))
            .collect();
        assert_eq!(
            labels,
            [
                (Holding, 5, "Lamp".to_string()),
                (Holding, 6, "Fan".to_string()),
                (Holding, 40, "Room target".to_string()),
                (Holding, 41, "Room target".to_string()),
                (Holding, 50, "Blind".to_string()),
                (Input, 30, "Room temperature".to_string()),
                (Input, 51, "Blind status".to_string()),
                (Coil, 3, "Pump".to_string()),
                (Discrete, 7, "Pump state".to_string()),
            ]
        );
        let room = registers.inputs[0].custom.as_ref().expect("rule");
        assert_eq!(room.repr, CustomRepr::I16);
        assert_eq!(room.suffix, " °F");
    }

    #[test]
    fn invalid_entities_are_skipped_and_empty_input_is_rejected() {
        let registers = parse(
            "
sensors:
  - name: Broken
  - name: Fine
    address: 1
",
        )
        .expect("importable");
        assert_eq!(registers.holdings.len(), 1);
        for text in [
            "",
            "just text",
            "modbus: []",
            "{\"version\": 1}",
            "sensors: []",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
    }
}
