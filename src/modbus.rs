use std::fmt::Debug;
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio_modbus::client::{rtu, tcp, Context, Reader, Writer};
use tokio_modbus::slave::Slave;
use tokio_serial::SerialStream;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
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
                // TODO: hmmm

                context
            }
            Interface::Mock => MockContext::make()
        };

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            config: config.clone(),
        })
    }

    pub async fn inputs(&self, address: u16, quantity: u16) -> Result<Vec<u16>> {
        timeout!(self, read_input_registers, (address, quantity))
    }

    pub async fn holdings(&self, address: u16, quantity: u16) -> Result<Vec<u16>> {
        timeout!(self, read_holding_registers, (address, quantity))
    }

    pub async fn write_register(&self, address: u16, data: u16) -> Result<()> {
        timeout!(self, write_single_register, (address, data))
    }
}
