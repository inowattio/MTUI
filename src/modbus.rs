use crate::mock::MockContext;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_modbus::client::{rtu, tcp, Context, Reader, Writer};
use tokio_modbus::prelude::SlaveContext;
use tokio_modbus::slave::{Slave, SlaveId};
use tokio_serial::SerialStream;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Interface {
    Wired(InterfaceWiredParams),
    Network(InterfaceNetworkParams),
    Mock,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

impl From<DataBits> for tokio_serial::DataBits {
    fn from(val: DataBits) -> Self {
        match val {
            DataBits::Five => tokio_serial::DataBits::Five,
            DataBits::Six => tokio_serial::DataBits::Six,
            DataBits::Seven => tokio_serial::DataBits::Seven,
            DataBits::Eight => tokio_serial::DataBits::Eight,
        }
    }
}

impl From<DataBits> for u8 {
    fn from(val: DataBits) -> Self {
        match val {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        }
    }
}

impl TryFrom<u8> for DataBits {
    type Error = &'static str;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match value {
            5 => Self::Five,
            6 => Self::Six,
            7 => Self::Seven,
            8 => Self::Eight,
            _ => Err("Failed to parse parity")?,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
pub enum Parity {
    None,
    Odd,
    Even,
}

impl From<Parity> for tokio_serial::Parity {
    fn from(val: Parity) -> Self {
        match val {
            Parity::None => tokio_serial::Parity::None,
            Parity::Odd => tokio_serial::Parity::Odd,
            Parity::Even => tokio_serial::Parity::Even,
        }
    }
}

impl From<Parity> for u8 {
    fn from(val: Parity) -> Self {
        match val {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

impl TryFrom<u8> for Parity {
    type Error = &'static str;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::None,
            1 => Self::Odd,
            2 => Self::Even,
            _ => Err("Failed to parse parity")?,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
pub enum StopBits {
    One,
    Two,
}

impl From<StopBits> for tokio_serial::StopBits {
    fn from(val: StopBits) -> Self {
        match val {
            StopBits::One => tokio_serial::StopBits::One,
            StopBits::Two => tokio_serial::StopBits::Two,
        }
    }
}

impl From<StopBits> for u8 {
    fn from(val: StopBits) -> Self {
        match val {
            StopBits::One => 1,
            StopBits::Two => 2,
        }
    }
}

impl TryFrom<u8> for StopBits {
    type Error = &'static str;

    fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::One,
            2 => Self::Two,
            _ => Err("Failed to parse stop bits")?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InterfaceWiredParams {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InterfaceNetworkParams {
    pub ip: String,
    pub port: u16,
}

pub trait ModbusDataOrder: Clone + Send + Sync {
    fn make_word(a: u16, b: u16) -> u32;
    fn make_dword(a: u32, b: u32) -> u64;
    fn split_word(data: u32) -> [u16; 2];
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone)]
pub struct ABCD;
impl ModbusDataOrder for ABCD {
    fn make_word(a: u16, b: u16) -> u32 {
        ((a as u32) << 16) | (b as u32)
    }

    fn make_dword(a: u32, b: u32) -> u64 {
        ((a as u64) << 32) | (b as u64)
    }

    fn split_word(data: u32) -> [u16; 2] {
        [(data >> 16) as u16, (data & 0xFFFF) as u16]
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone)]
pub struct BADC;
impl ModbusDataOrder for BADC {
    fn make_word(a: u16, b: u16) -> u32 {
        ABCD::make_word(a.swap_bytes(), b.swap_bytes())
    }

    fn make_dword(a: u32, b: u32) -> u64 {
        ABCD::make_dword(a.swap_bytes(), b.swap_bytes())
    }

    fn split_word(data: u32) -> [u16; 2] {
        let [a, b] = ABCD::split_word(data);
        [a.swap_bytes(), b.swap_bytes()]
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone)]
pub struct CDAB;
impl ModbusDataOrder for CDAB {
    fn make_word(a: u16, b: u16) -> u32 {
        ABCD::make_word(b, a)
    }

    fn make_dword(a: u32, b: u32) -> u64 {
        ABCD::make_dword(b, a)
    }

    fn split_word(data: u32) -> [u16; 2] {
        let [a, b] = ABCD::split_word(data);
        [b, a]
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Clone)]
pub struct DCBA;
impl ModbusDataOrder for DCBA {
    fn make_word(a: u16, b: u16) -> u32 {
        ABCD::make_word(b.swap_bytes(), a.swap_bytes())
    }

    fn make_dword(a: u32, b: u32) -> u64 {
        ABCD::make_dword(b.swap_bytes(), a.swap_bytes())
    }

    fn split_word(data: u32) -> [u16; 2] {
        let [a, b] = ABCD::split_word(data);
        [b.swap_bytes(), a.swap_bytes()]
    }
}

#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
pub enum WordOrder {
    #[default]
    ABCD,
    BADC,
    CDAB,
    DCBA,
}

impl WordOrder {
    pub fn make_word(self, a: u16, b: u16) -> u32 {
        match self {
            Self::ABCD => ABCD::make_word(a, b),
            Self::BADC => BADC::make_word(a, b),
            Self::CDAB => CDAB::make_word(a, b),
            Self::DCBA => DCBA::make_word(a, b),
        }
    }

    pub fn make_dword(self, a: u32, b: u32) -> u64 {
        match self {
            Self::ABCD => ABCD::make_dword(a, b),
            Self::BADC => BADC::make_dword(a, b),
            Self::CDAB => CDAB::make_dword(a, b),
            Self::DCBA => DCBA::make_dword(a, b),
        }
    }

    pub fn split_word(self, data: u32) -> [u16; 2] {
        match self {
            Self::ABCD => ABCD::split_word(data),
            Self::BADC => BADC::split_word(data),
            Self::CDAB => CDAB::split_word(data),
            Self::DCBA => DCBA::split_word(data),
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::ABCD => Self::BADC,
            Self::BADC => Self::CDAB,
            Self::CDAB => Self::DCBA,
            Self::DCBA => Self::ABCD,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceConfig {
    pub interface: Interface,
    pub slave_id: tokio_modbus::slave::SlaveId,
    pub timeout_connect_ms: u64,
    pub timeout_command_ms: u64,
    pub time_between_commands_ms: u64,
    #[serde(default)]
    pub word_order: WordOrder,
}

async fn timeout<F, D>(future: F, timeout: Duration, between: Duration) -> Result<D>
where
    F: Future<Output = D> + Send,
    D: Send,
{
    let output = tokio::time::timeout(timeout, future).await?;

    tokio::time::sleep(between).await;

    Ok(output)
}

macro_rules! timeout {
    ($this:ident, $action:ident, ($($arg:expr),* $(,)?)) => {
        {
            let mut hold = $this.context.lock().await;
            let timeout_command = Duration::from_millis($this.config.timeout_command_ms);
            let time_between = Duration::from_millis($this.config.time_between_commands_ms);
            timeout(hold.$action($($arg),*), timeout_command, time_between).await
                .map_err(|e| anyhow::Error::from(e))?
                .map_err(|e| anyhow::Error::from(e))?
                .map_err(|e| anyhow::Error::from(e))
        }
    };
}

#[derive(Clone)]
pub struct ModbusDevice {
    context: Arc<Mutex<Context>>,
    config: DeviceConfig,
}

impl Debug for ModbusDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModbusDevice {{ config: {:?} }}", self.config)
    }
}

impl ModbusDevice {
    pub async fn new(config: &DeviceConfig) -> Result<Self> {
        let timeout_connect = Duration::from_millis(config.timeout_connect_ms);
        let slave = Slave(config.slave_id);

        let context = match &config.interface {
            Interface::Wired(interface) => {
                let builder = tokio_serial::new(&interface.path, interface.baud_rate)
                    .timeout(timeout_connect)
                    .data_bits(interface.data_bits.into())
                    .parity(interface.parity.into())
                    .stop_bits(interface.stop_bits.into());

                let port = SerialStream::open(&builder)?;
                rtu::attach_slave(port, slave)
            }
            Interface::Network(interface) => {
                let socket_addr = SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from_str(&interface.ip)?,
                    interface.port,
                ));
                let connection = tcp::connect_slave(socket_addr, slave);
                let context = timeout(connection, timeout_connect, Duration::default()).await??;

                tokio::time::sleep(Duration::from_secs(2)).await;

                context
            }
            Interface::Mock => MockContext::make(),
        };

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            config: config.clone(),
        })
    }

    pub async fn set_slave(&self, slave_id: SlaveId) {
        self.context.lock().await.set_slave(Slave(slave_id));
    }

    pub fn set_word_order(&mut self, word_order: WordOrder) {
        self.config.word_order = word_order;
    }

    pub async fn inputs(&self, address: u16, quantity: u16) -> Result<Vec<u16>> {
        timeout!(self, read_input_registers, (address, quantity))
    }

    pub async fn holdings(&self, address: u16, quantity: u16) -> Result<Vec<u16>> {
        timeout!(self, read_holding_registers, (address, quantity))
    }

    pub async fn input_word(&self, address: u16) -> Result<u32> {
        let data = self.inputs(address, 2).await?;
        anyhow::ensure!(data.len() == 2, "Expected 2 values.");
        Ok(self.config.word_order.make_word(data[0], data[1]))
    }

    pub async fn input_words(&self, address: u16, quantity: u16) -> Result<Vec<u32>> {
        let register_count = quantity
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("Input word quantity is too large."))?;
        let data = self.inputs(address, register_count).await?;
        anyhow::ensure!(
            data.len() == register_count as usize,
            "Expected {register_count} values."
        );
        Ok(data
            .chunks_exact(2)
            .map(|word| self.config.word_order.make_word(word[0], word[1]))
            .collect())
    }

    pub async fn holding_word(&self, address: u16) -> Result<u32> {
        let data = self.holdings(address, 2).await?;
        anyhow::ensure!(data.len() == 2, "Expected 2 values.");
        Ok(self.config.word_order.make_word(data[0], data[1]))
    }

    pub async fn holding_dword(&self, address: u16) -> Result<u64> {
        let data = self.holdings(address, 4).await?;
        anyhow::ensure!(data.len() == 4, "Expected 4 values.");
        let high = self.config.word_order.make_word(data[0], data[1]);
        let low = self.config.word_order.make_word(data[2], data[3]);
        Ok(self.config.word_order.make_dword(high, low))
    }

    pub async fn holding_words(&self, address: u16, quantity: u16) -> Result<Vec<u32>> {
        let register_count = quantity
            .checked_mul(2)
            .ok_or_else(|| anyhow::anyhow!("Holding word quantity is too large."))?;
        let data = self.holdings(address, register_count).await?;
        anyhow::ensure!(
            data.len() == register_count as usize,
            "Expected {register_count} values."
        );
        Ok(data
            .chunks_exact(2)
            .map(|word| self.config.word_order.make_word(word[0], word[1]))
            .collect())
    }

    pub async fn holding_dwords(&self, address: u16, quantity: u16) -> Result<Vec<u64>> {
        let register_count = quantity
            .checked_mul(4)
            .ok_or_else(|| anyhow::anyhow!("Holding dword quantity is too large."))?;
        let data = self.holdings(address, register_count).await?;
        anyhow::ensure!(
            data.len() == register_count as usize,
            "Expected {register_count} values."
        );
        Ok(data
            .chunks_exact(4)
            .map(|dword| {
                let high = self.config.word_order.make_word(dword[0], dword[1]);
                let low = self.config.word_order.make_word(dword[2], dword[3]);
                self.config.word_order.make_dword(high, low)
            })
            .collect())
    }

    pub async fn write_register(&self, address: u16, data: u16) -> Result<()> {
        timeout!(self, write_single_register, (address, data))
    }

    pub async fn write_registers(&self, address: u16, data: &[u16]) -> Result<()> {
        timeout!(self, write_multiple_registers, (address, data))
    }

    pub async fn write_register_word(&self, address: u16, data: i32) -> Result<()> {
        let words = self.config.word_order.split_word(data as u32);
        self.write_registers(address, &words).await
    }

    pub async fn write_register_words(&self, address: u16, data: &[i32]) -> Result<()> {
        for (i, item) in data.iter().enumerate() {
            self.write_register_word(address + (i * 2) as u16, *item)
                .await?;
        }

        Ok(())
    }
}
