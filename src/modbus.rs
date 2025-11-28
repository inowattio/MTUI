use std::borrow::Cow;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio_modbus::client::{rtu, tcp, Client, Context, Reader, Writer};
use tokio_modbus::slave::Slave;
use tokio_serial::SerialStream;
use anyhow::{Error, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_modbus::{Request, Response};
use crate::mock::MockContext;

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

impl Into<tokio_serial::DataBits> for DataBits {
    fn into(self) -> tokio_serial::DataBits {
        match self {
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

impl Into<tokio_serial::Parity> for Parity {
    fn into(self) -> tokio_serial::Parity {
        match self {
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

impl Into<tokio_serial::StopBits> for StopBits {
    fn into(self) -> tokio_serial::StopBits {
        match self {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceConfig {
    pub interface: Interface,
    pub slave_id: tokio_modbus::slave::SlaveId,
    pub timeout_connect_ms: u64,
    pub timeout_command_ms: u64,
    pub time_between_commands_ms: u64,
}

pub const fn combine_u16_to_u32(high: u16, low: u16) -> u32 {
    // Shift the 'high' value to the left by 16 bits and then combine it with the
    // 'low' value
    (high as u32) << 16 | (low as u32)
}

pub fn vec_to_string(data: &[u16]) -> String {
    let bytes: Vec<u8> = data
        .iter()
        .flat_map(|n| [(n >> 8) as u8, (n & 0xFF) as u8])
        .collect();

    String::from_utf8_lossy(&bytes).to_string()
}

async fn timeout<F, D>(future: F, timeout: Duration, between: Duration) -> Result<D>
where
    F: Future<Output = D> + Send,
    D: Send, {
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

#[derive(Clone, Debug)]
pub struct ModbusDevice {
    context: Arc<Mutex<Context>>,
    config: DeviceConfig,
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
                // TODO: hmmm

                context
            }
            Interface::Mock => MockContext::new()
        };

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            config: config.clone(),
        })
    }

    pub async fn inputs<const N: usize>(&self, address: u16) -> Result<[u16; N]> {
        timeout!(self, read_input_registers, (address, N as u16))?
            .try_into().map_err(|_| Error::msg("Nope"))
    }

    pub async fn input(&self, address: u16) -> Result<u16> {
        let data = timeout!(self, read_input_registers, (address, 1))?;
        Ok(data[0])
    }

    pub async fn input_word(&self, address: u16) -> Result<u32> {
        let [h, l] = self.inputs(address).await?;
        Ok(combine_u16_to_u32(h, l))
    }

    pub async fn input_words<const N: usize>(&self, address: u16) -> Result<[u32; N]> {
        let mut combined = [0u32; N];

        for (i, item) in combined.iter_mut().enumerate() {
            let [h, l] = self.inputs(address + (i * 2) as u16).await?;
            *item = combine_u16_to_u32(h, l);
        }

        Ok(combined)
    }

    pub async fn input_ascii<const N: usize>(&self, address: u16) -> Result<String> {
        let data = self.inputs::<N>(address).await?;
        Ok(vec_to_string(&data))
    }

    pub async fn holdings<const N: usize>(&self, address: u16) -> Result<[u16; N]> {
        timeout!(self, read_holding_registers, (address, N as u16))?
            .try_into().map_err(|_| Error::msg("Nope"))
    }

    pub async fn holding(&self, address: u16) -> Result<u16> {
        let data = timeout!(self, read_holding_registers, (address, 1))?;
        Ok(data[0])
    }

    pub async fn holding_word(&self, address: u16) -> Result<u32> {
        let [h, l] = self.holdings(address).await?;
        Ok(combine_u16_to_u32(h, l))
    }

    pub async fn holding_words<const N: usize>(&self, address: u16) -> Result<[u32; N]> {
        let mut combined = [0u32; N];

        for (i, item) in combined.iter_mut().enumerate() {
            let [h, l] = self.holdings(address + (i * 2) as u16).await?;
            *item = combine_u16_to_u32(h, l);
        }

        Ok(combined)
    }

    pub async fn holding_ascii<const N: usize>(&self, address: u16) -> Result<String> {
        let data = self.holdings::<N>(address).await?;
        Ok(vec_to_string(&data))
    }

    pub async fn write_coil(&self, address: u16, coil: bool) -> Result<()> {
        timeout!(self, write_single_coil, (address, coil))
    }

    pub async fn write_coils(&self, address: u16, coils: &[bool]) -> Result<()> {
        timeout!(self, write_multiple_coils, (address, coils))
    }

    pub async fn write_register(&self, address: u16, data: u16) -> Result<()> {
        timeout!(self, write_single_register, (address, data))
    }

    pub async fn write_registers(&self, address: u16, data: &[u16]) -> Result<()> {
        timeout!(self, write_multiple_registers, (address, data))
    }

    pub async fn custom_function(&self, code: u8, data: &[u8]) -> Result<Vec<u8>> {
        let mut hold = self.context.lock().await;
        let timeout_command = Duration::from_millis(self.config.timeout_command_ms);
        let time_between = Duration::from_millis(self.config.time_between_commands_ms);

        let response = timeout(hold.call(Request::Custom(code, Cow::Borrowed(data))), timeout_command, time_between).await???;

        if let Response::Custom(_, data) = response {
            Ok(data.to_vec())
        } else {
            Err(anyhow::Error::msg("unexpected response type"))
        }
    }
}
