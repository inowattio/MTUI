use crate::compat::{self, Instant};
use async_trait::async_trait;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::time::Duration;
use tokio_modbus::client::{Client, Context};
use tokio_modbus::prelude::{
    ConformityLevel, DeviceIdObject, ReadCode, ReadDeviceIdentificationResponse,
};
use tokio_modbus::slave::SlaveContext;
use tokio_modbus::{ExceptionCode, Request, Response, Slave, SlaveId};

/// A simulated RS-485 bus of power meters.
///
/// Slaves 0..=9 respond, each with its own serial number, phase offset and
/// power scale; any other slave id stays silent until the command times out,
/// like an absent device on a real bus.
///
/// Holding registers (everything else answers IllegalDataAddress):
///   0..=7      ASCII model name
///   8          firmware version (BCD)
///   9..=10     serial number (u32)
///   11         slave id
///   12..=13    uptime seconds (u32)
///   14         register map version, 15 phase count
///   50..=199   writable: 50 voltage x10, 51 current x100, 52 ripple x10,
///              53 noise on/off, 54 time scale %, rest scratch
///   200..=215  ASCII vendor string
///   216..=218  per-phase calibration gain x10000
///   250..=253  tariff rates x1000, 254 active tariff
///   300..=305  voltage min/max per phase x10
///   306..=308  current max per phase x100
///   320..=327  event log, 4 x (code, age seconds) pairs
///   1000..=1001 energy Wh (u32), 1002..=1003 on-time s (u32),
///   1004       accepted-write counter, 1005..=1006 energy (m10k)
///   1100       status word (bit0 run, bit1 grid, bit2 warn, bit15 heartbeat)
///   1101       alarm count
///
/// Input registers (0..=499 mapped):
///   0..=2      voltages L1-L3 x10, 3..=5 currents x100, 6 frequency x100,
///   7          temperature x10, 8..=9 active power kW (f32),
///   10..=11    power factor (f32), 12..=13 apparent power VA (u32),
///   14..=15    reactive power var (i32, signed), 16..=17 energy (m10k),
///   20..=23    total energy kWh (f64), 30 seconds, 31 sawtooth,
///   32         square wave, 33 noise, 34 random walk,
///   35..=37    THD per phase x100, 40..=42 per-phase active power W,
///   43..=45    per-phase reactive power var (i16), 46..=48 per-phase VA,
///   50..=52    line-to-line voltages x10, 53 neutral current x100,
///   60         demand kW x100, 61 peak demand kW x100,
///   62..=63    peak demand time, uptime seconds (u32),
///   100..=131  voltage waveform snapshot, 32 samples around 2048,
///   140..=155  harmonic spectrum, 16 bins (fundamental at 140),
///   200..=223  hourly energy profile kWh x10, 24 hours,
///   400..=431  counters ticking at (addr - 399) / 4 Hz
#[derive(Debug)]
pub struct MockContext {
    started: Instant,
    written: HashMap<u16, u16>,
    write_count: u16,
    slave_id: SlaveId,
}

const KNOWN_SLAVES: RangeInclusive<SlaveId> = 0..=9;
const WRITABLE: RangeInclusive<u16> = 50..=199;
const HOLDING_ZONES: [RangeInclusive<u16>; 2] = [0..=499, 1000..=1199];
const INPUT_ZONE: RangeInclusive<u16> = 0..=499;

const MODEL_NAME: &[u8; 16] = b"MTUI SIMULATOR  ";
const VENDOR_NAME: &[u8; 32] = b"      POWER BUS SIMULATOR!      ";
const THIRD_PHASE: f64 = 2.0944;

fn word(value: u32, index: u16) -> u16 {
    if index == 0 {
        (value >> 16) as u16
    } else {
        value as u16
    }
}

fn qword(value: u64, index: u16) -> u16 {
    (value >> (48 - 16 * index)) as u16
}

fn m10k(value: u32, index: u16) -> u16 {
    if index == 0 {
        ((value / 10_000) % 10_000) as u16
    } else {
        (value % 10_000) as u16
    }
}

impl MockContext {
    pub fn make() -> Context {
        let client: Box<dyn Client> = Box::new(Self {
            started: Instant::now(),
            written: HashMap::new(),
            write_count: 0,
            slave_id: 0,
        });
        client.into()
    }

    fn setpoint(&self, addr: u16) -> u16 {
        if let Some(&value) = self.written.get(&addr) {
            return value;
        }
        match addr {
            50 => 2300,
            51 => 1500,
            52 => 15,
            53 => 1,
            54 => 100,
            _ => 0,
        }
    }

    fn time(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * (self.setpoint(54) as f64 / 100.0)
    }

    fn phase(&self) -> f64 {
        self.slave_id as f64 * 0.7
    }

    fn noise(&self, t: f64, addr: u16) -> f64 {
        if self.setpoint(53) == 0 {
            return 0.0;
        }
        let mut x = (t * 10.0) as u64 ^ ((addr as u64) << 32) ^ ((self.slave_id as u64) << 48);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % 2001) as f64 / 1000.0 - 1.0
    }

    fn active_power_kw(&self, t: f64) -> f64 {
        let base = 3.2 + 0.4 * self.slave_id as f64;
        base + 0.8 * (t / 7.0 + self.phase()).sin() + 0.1 * self.noise(t, 8)
    }

    fn energy_wh(&self, t: f64) -> f64 {
        let base_kw = 3.2 + 0.4 * self.slave_id as f64;
        50_000.0 * (1.0 + self.slave_id as f64) + base_kw * 1000.0 * t / 3600.0
    }

    fn status_word(&self, t: f64) -> u16 {
        let mut status = 1; // running
        if (t + self.phase()) % 47.0 > 1.5 {
            status |= 1 << 1; // grid ok, with a short dropout every ~47s
        }
        if ((t / 19.0) as u64).is_multiple_of(2) {
            status |= 1 << 2; // slow warning toggle
        }
        if (t as u64).is_multiple_of(2) {
            status |= 1 << 15; // 1 Hz heartbeat
        }
        status
    }

    fn holding_value(&self, addr: u16, t: f64) -> u16 {
        if WRITABLE.contains(&addr) {
            return self.setpoint(addr);
        }
        match addr {
            0..=7 => {
                let i = addr as usize * 2;
                u16::from_be_bytes([MODEL_NAME[i], MODEL_NAME[i + 1]])
            }
            8 => 0x0142, // v1.42 in BCD
            9 | 10 => word(24_000_000 + 1_111 * self.slave_id as u32, addr - 9),
            11 => self.slave_id as u16,
            12 | 13 => word(t as u32, addr - 12),
            14 => 3, // register map version
            15 => 3, // phase count
            200..=215 => {
                let i = (addr - 200) as usize * 2;
                u16::from_be_bytes([VENDOR_NAME[i], VENDOR_NAME[i + 1]])
            }
            216..=218 => 10_000 + 7 * self.slave_id as u16 + 13 * (addr - 216),
            250..=253 => [120, 180, 90, 240][(addr - 250) as usize] + self.slave_id as u16,
            254 => ((t / 30.0) as u16) % 4,
            300..=302 => self
                .setpoint(50)
                .saturating_sub(self.setpoint(52) + 8 + 3 * (addr - 300)),
            303..=305 => self.setpoint(50) + self.setpoint(52) + 8 + 3 * (addr - 303),
            306..=308 => (self.setpoint(51) as f64 * 1.15) as u16 + 9 * (addr - 306),
            320..=327 => {
                let n = (addr - 320) / 2;
                if (addr - 320).is_multiple_of(2) {
                    [3, 7, 2, 9][n as usize] // event code
                } else {
                    (t as u16).wrapping_add(137 * (n + 1)) // age in seconds
                }
            }
            1000 | 1001 => word(self.energy_wh(t) as u32, addr - 1000),
            1002 | 1003 => word(t as u32, addr - 1002),
            1004 => self.write_count,
            1005 | 1006 => m10k(self.energy_wh(t) as u32, addr - 1005),
            1100 => self.status_word(t),
            1101 => (t / 97.0) as u16,
            _ => 0,
        }
    }

    fn input_value(&self, addr: u16, t: f64) -> u16 {
        let phase = self.phase();
        match addr {
            0..=2 => {
                let p = addr as f64 * THIRD_PHASE;
                let ripple = self.setpoint(52) as f64;
                let v = self.setpoint(50) as f64
                    + ripple * (t / 3.1 + p + phase).sin()
                    + 2.0 * self.noise(t, addr);
                v.max(0.0) as u16
            }
            3..=5 => {
                let p = (addr - 3) as f64 * THIRD_PHASE;
                let v = self.setpoint(51) as f64 * (0.9 + 0.1 * (t / 5.3 + p + phase).sin())
                    + 5.0 * self.noise(t, addr);
                v.max(0.0) as u16
            }
            6 => (5_000.0 + 3.0 * (t / 11.0).sin() + self.noise(t, 6)) as u16,
            7 => (285.0 + 30.0 * (t / 60.0 + phase).sin() + self.noise(t, 7)) as u16,
            8 | 9 => word((self.active_power_kw(t) as f32).to_bits(), addr - 8),
            10 | 11 => word(
                ((0.92 + 0.07 * (t / 13.0).sin()) as f32).to_bits(),
                addr - 10,
            ),
            12 | 13 => word((self.active_power_kw(t) * 1_080.0) as u32, addr - 12),
            14 | 15 => {
                let var = (self.active_power_kw(t) * 320.0 * (t / 23.0 + phase).sin()) as i32;
                word(var as u32, addr - 14)
            }
            16 | 17 => m10k(self.energy_wh(t) as u32, addr - 16),
            20..=23 => qword((self.energy_wh(t) / 1000.0).to_bits(), addr - 20),
            30 => t as u16,
            31 => ((t * 100.0) as u16) % 1000,
            32 => ((t / 5.0) as u64 % 2) as u16,
            33 => ((self.noise(t, 33) + 1.0) * 32_767.5) as u16,
            34 => {
                let walk = 2_048.0
                    + 1_024.0 * (t / 17.0 + phase).sin()
                    + 256.0 * self.noise((t / 4.0).floor() * 4.0, 34);
                walk as u16
            }
            35..=37 => {
                let p = (addr - 35) as f64 * THIRD_PHASE;
                let thd = 250.0 + 150.0 * (t / 9.0 + p + phase).sin() + 20.0 * self.noise(t, addr);
                thd.max(0.0) as u16
            }
            40..=42 => {
                let imbalance = 1.0 + 0.06 * (t / 8.0 + (addr - 40) as f64 + phase).sin();
                (self.active_power_kw(t) * 1_000.0 / 3.0 * imbalance) as u16
            }
            43..=45 => {
                let p = (addr - 43) as f64;
                let var = self.active_power_kw(t) * 110.0 * (t / 23.0 + p + phase).sin();
                var as i16 as u16
            }
            46..=48 => {
                let pf = 0.92 + 0.05 * (t / 13.0 + (addr - 46) as f64).sin();
                (self.active_power_kw(t) * 1_000.0 / 3.0 / pf) as u16
            }
            50..=52 => {
                let p = (addr - 50) as f64 * THIRD_PHASE;
                let ripple = self.setpoint(52) as f64;
                let v = self.setpoint(50) as f64 * 1.732 + ripple * (t / 3.7 + p + phase).sin();
                v.max(0.0) as u16
            }
            53 => (self.setpoint(51) as f64 * 0.03 + 4.0 * self.noise(t, 53).abs()) as u16,
            60 => {
                let demand = 3.2 + 0.4 * self.slave_id as f64 + 0.5 * (t / 45.0 + phase).sin();
                (demand * 100.0) as u16
            }
            61 => ((3.2 + 0.4 * self.slave_id as f64 + 0.9) * 100.0) as u16,
            62 | 63 => word((t / 2.0) as u32, addr - 62),
            100..=131 => {
                let k = (addr - 100) as f64 / 32.0;
                let fundamental = (std::f64::consts::TAU * k + t).sin();
                let third = 0.12 * (3.0 * (std::f64::consts::TAU * k + t)).sin();
                (2_048.0 + 1_800.0 * (fundamental + third) + 8.0 * self.noise(t, addr)) as u16
            }
            140..=155 => {
                let n = (addr - 139) as f64;
                let base = if (addr - 139) % 2 == 1 {
                    1_000.0 / n
                } else {
                    90.0 / n
                };
                (base * (1.0 + 0.08 * (t / 3.0 + n).sin())).max(0.0) as u16
            }
            200..=223 => {
                let h = (addr - 200) as f64;
                let morning = (1.0 - ((h - 8.0) / 3.0).powi(2)).max(0.0);
                let evening = (1.0 - ((h - 19.0) / 3.5).powi(2)).max(0.0);
                let kwh10 =
                    (25.0 + 70.0 * morning + 110.0 * evening) * (1.0 + 0.1 * self.slave_id as f64);
                (kwh10 + 2.0 * (t / 30.0 + h).sin()).max(0.0) as u16
            }
            400..=431 => ((t * (addr - 399) as f64 / 4.0) as u32 % 10_000) as u16,
            _ => 0,
        }
    }

    fn device_id_objects(&self) -> [(u8, Vec<u8>); 8] {
        [
            (0x00, b"POWER BUS SIMULATOR".to_vec()),
            (0x01, b"MTUI-SIM".to_vec()),
            (0x02, b"v1.42".to_vec()),
            (0x03, b"https://github.com/inowattio/mtui".to_vec()),
            (0x04, b"MTUI Simulator".to_vec()),
            (0x05, b"MTUI SIMULATOR".to_vec()),
            (
                0x06,
                format!("Power Bus Sim Slave {}", self.slave_id).into_bytes(),
            ),
            (0x07, b" TF2! ".to_vec()),
        ]
    }

    fn device_id_response(
        &self,
        read_code: ReadCode,
        object_id: u8,
    ) -> Result<Response, ExceptionCode> {
        let make = |(id, value): (u8, Vec<u8>)| DeviceIdObject {
            id,
            value: value.into(),
        };
        let objects: Vec<DeviceIdObject> = match read_code {
            ReadCode::Basic => self
                .device_id_objects()
                .into_iter()
                .filter(|(id, _)| *id <= 0x02)
                .map(make)
                .collect(),
            ReadCode::Regular | ReadCode::Extended => {
                self.device_id_objects().into_iter().map(make).collect()
            }
            ReadCode::Specific => match self
                .device_id_objects()
                .into_iter()
                .find(|(id, _)| *id == object_id)
            {
                Some(object) => vec![make(object)],
                None => return Err(ExceptionCode::IllegalDataAddress),
            },
        };

        Ok(Response::ReadDeviceIdentification(
            ReadDeviceIdentificationResponse {
                read_code,
                conformity_level: ConformityLevel::ExtendedIdentification,
                more_follows: false,
                next_object_id: 0,
                device_id_objects: objects,
            },
        ))
    }

    async fn simulate_latency(&self) {
        let jitter = (self.started.elapsed().as_micros() as u64).wrapping_mul(2_654_435_761) % 12;
        compat::sleep(Duration::from_millis(4 + jitter)).await;
    }
}

fn mapped(zones: &[RangeInclusive<u16>], addr: u16, count: u16) -> bool {
    (0..count).all(|i| {
        addr.checked_add(i)
            .is_some_and(|a| zones.iter().any(|zone| zone.contains(&a)))
    })
}

#[async_trait]
impl SlaveContext for MockContext {
    fn set_slave(&mut self, slave: Slave) {
        self.slave_id = slave.0
    }
}

#[async_trait]
impl Client for MockContext {
    async fn call(&mut self, request: Request<'_>) -> tokio_modbus::Result<Response> {
        if !KNOWN_SLAVES.contains(&self.slave_id) {
            std::future::pending::<()>().await;
            unreachable!();
        }

        self.simulate_latency().await;
        let t = self.time();

        match request {
            Request::ReadHoldingRegisters(addr, count) => {
                if !mapped(&HOLDING_ZONES, addr, count) {
                    return Ok(Err(ExceptionCode::IllegalDataAddress));
                }
                let regs = (0..count)
                    .map(|i| self.holding_value(addr + i, t))
                    .collect();
                Ok(Ok(Response::ReadHoldingRegisters(regs)))
            }

            Request::ReadInputRegisters(addr, count) => {
                if !mapped(&[INPUT_ZONE], addr, count) {
                    return Ok(Err(ExceptionCode::IllegalDataAddress));
                }
                let regs = (0..count).map(|i| self.input_value(addr + i, t)).collect();
                Ok(Ok(Response::ReadInputRegisters(regs)))
            }

            Request::WriteSingleRegister(addr, value) => {
                if !WRITABLE.contains(&addr) {
                    return Ok(Err(ExceptionCode::IllegalDataAddress));
                }
                self.written.insert(addr, value);
                self.write_count = self.write_count.wrapping_add(1);
                Ok(Ok(Response::WriteSingleRegister(addr, value)))
            }

            Request::WriteMultipleRegisters(addr, values) => {
                let quantity = values.len() as u16;
                let writable = (0..quantity)
                    .all(|i| addr.checked_add(i).is_some_and(|a| WRITABLE.contains(&a)));
                if !writable {
                    return Ok(Err(ExceptionCode::IllegalDataAddress));
                }
                for (i, value) in values.iter().enumerate() {
                    self.written.insert(addr + i as u16, *value);
                }
                self.write_count = self.write_count.wrapping_add(1);
                Ok(Ok(Response::WriteMultipleRegisters(addr, quantity)))
            }

            Request::ReadDeviceIdentification(read_code, object_id) => {
                Ok(self.device_id_response(read_code, object_id))
            }

            Request::Custom(code, data) => Ok(Ok(Response::Custom(code, data.into_owned().into()))),

            _ => Ok(Err(ExceptionCode::IllegalFunction)),
        }
    }

    async fn disconnect(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
