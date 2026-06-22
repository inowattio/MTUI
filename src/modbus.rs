use crate::compat;
use crate::mock::MockContext;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(not(target_arch = "wasm32"))]
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use tokio_modbus::client::{rtu, tcp};
use tokio_modbus::client::{Client, Context, Reader, Writer};
use tokio_modbus::prelude::{ReadCode, ReadDeviceIdentificationResponse, SlaveContext};
use tokio_modbus::slave::{Slave, SlaveId};
use tokio_modbus::{Request, Response};
#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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

#[cfg(not(target_arch = "wasm32"))]
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
    fn swaps(self) -> (bool, bool) {
        match self {
            Self::ABCD => (false, false),
            Self::BADC => (true, false),
            Self::CDAB => (false, true),
            Self::DCBA => (true, true),
        }
    }

    pub fn make_word(self, a: u16, b: u16) -> u32 {
        let (byte_swap, word_swap) = self.swaps();
        let (mut high, mut low) = if word_swap { (b, a) } else { (a, b) };
        if byte_swap {
            high = high.swap_bytes();
            low = low.swap_bytes();
        }
        ((high as u32) << 16) | (low as u32)
    }

    pub fn make_dword(self, a: u32, b: u32) -> u64 {
        let (byte_swap, word_swap) = self.swaps();
        let (mut high, mut low) = if word_swap { (b, a) } else { (a, b) };
        if byte_swap {
            high = high.swap_bytes();
            low = low.swap_bytes();
        }
        ((high as u64) << 32) | (low as u64)
    }

    pub fn split_word(self, data: u32) -> [u16; 2] {
        let (byte_swap, word_swap) = self.swaps();
        let (mut first, mut second) = ((data >> 16) as u16, data as u16);
        if word_swap {
            (first, second) = (second, first);
        }
        if byte_swap {
            first = first.swap_bytes();
            second = second.swap_bytes();
        }
        [first, second]
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

#[cfg(test)]
mod tests {
    use super::WordOrder;

    #[test]
    fn make_word_orders_bytes() {
        assert_eq!(WordOrder::ABCD.make_word(0x0102, 0x0304), 0x01020304);
        assert_eq!(WordOrder::BADC.make_word(0x0102, 0x0304), 0x02010403);
        assert_eq!(WordOrder::CDAB.make_word(0x0102, 0x0304), 0x03040102);
        assert_eq!(WordOrder::DCBA.make_word(0x0102, 0x0304), 0x04030201);
    }

    #[test]
    fn make_dword_orders_bytes() {
        let (a, b) = (0x0102_0304, 0x0506_0708);
        assert_eq!(WordOrder::ABCD.make_dword(a, b), 0x0102_0304_0506_0708);
        assert_eq!(WordOrder::BADC.make_dword(a, b), 0x0403_0201_0807_0605);
        assert_eq!(WordOrder::CDAB.make_dword(a, b), 0x0506_0708_0102_0304);
        assert_eq!(WordOrder::DCBA.make_dword(a, b), 0x0807_0605_0403_0201);
    }

    #[test]
    fn split_word_inverts_make_word() {
        for order in [
            WordOrder::ABCD,
            WordOrder::BADC,
            WordOrder::CDAB,
            WordOrder::DCBA,
        ] {
            let word = order.make_word(0x0102, 0x0304);
            assert_eq!(order.split_word(word), [0x0102, 0x0304], "{order:?}");
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct DeviceConfig {
    pub interface: Interface,
    pub slave_id: SlaveId,
    pub timeout_connect_ms: u64,
    pub timeout_command_ms: u64,
    pub time_between_commands_ms: u64,
    pub word_order: WordOrder,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            interface: Interface::Mock,
            slave_id: 0,
            timeout_connect_ms: 1000,
            timeout_command_ms: 2000,
            time_between_commands_ms: 0,
            word_order: WordOrder::default(),
        }
    }
}

async fn timeout<F, D>(future: F, timeout: Duration, between: Duration) -> Result<D>
where
    F: Future<Output = D>,
{
    let output = compat::timeout(timeout, future).await?;

    compat::sleep(between).await;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceIdAccess {
    #[default]
    Basic,
    Regular,
    Extended,
    Specific,
}

impl DeviceIdAccess {
    pub const ALL: [DeviceIdAccess; 3] = [
        DeviceIdAccess::Basic,
        DeviceIdAccess::Regular,
        DeviceIdAccess::Extended,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DeviceIdAccess::Basic => "Basic",
            DeviceIdAccess::Regular => "Regular",
            DeviceIdAccess::Extended => "Extended",
            DeviceIdAccess::Specific => "Specific",
        }
    }

    fn into_read_code(self) -> ReadCode {
        match self {
            DeviceIdAccess::Basic => ReadCode::Basic,
            DeviceIdAccess::Regular => ReadCode::Regular,
            DeviceIdAccess::Extended => ReadCode::Extended,
            DeviceIdAccess::Specific => ReadCode::Specific,
        }
    }
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
        #[cfg(target_arch = "wasm32")]
        let _ = (timeout_connect, slave);

        #[cfg(not(target_arch = "wasm32"))]
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
                timeout(connection, timeout_connect, Duration::default()).await??
            }
            Interface::Mock => MockContext::make(),
        };

        #[cfg(target_arch = "wasm32")]
        let context = MockContext::make();

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

    pub async fn coils(&self, address: u16, quantity: u16) -> Result<Vec<bool>> {
        timeout!(self, read_coils, (address, quantity))
    }

    pub async fn discretes(&self, address: u16, quantity: u16) -> Result<Vec<bool>> {
        timeout!(self, read_discrete_inputs, (address, quantity))
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

    pub async fn write_coil(&self, address: u16, data: bool) -> Result<()> {
        timeout!(self, write_single_coil, (address, data))
    }

    pub async fn write_coils(&self, address: u16, data: &[bool]) -> Result<()> {
        timeout!(self, write_multiple_coils, (address, data))
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

    pub async fn device_identification(
        &self,
        access: DeviceIdAccess,
        object_id: u8,
    ) -> Result<ReadDeviceIdentificationResponse> {
        timeout!(
            self,
            read_device_identification,
            (access.into_read_code(), object_id)
        )
    }

    pub async fn device_identity(&self, access: DeviceIdAccess) -> Result<Vec<(u8, String)>> {
        let mut objects: Vec<(u8, String)> = Vec::new();
        let mut next_id = 0u8;

        loop {
            let response = self.device_identification(access, next_id).await?;
            for object in &response.device_id_objects {
                let value = match object.value_as_str() {
                    Some(text) => text
                        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                        .to_string(),
                    None => object
                        .value
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                objects.push((object.id, value));
            }

            if !response.more_follows || response.next_object_id == next_id || objects.len() >= 256
            {
                break;
            }
            next_id = response.next_object_id;
        }

        Ok(objects)
    }

    pub async fn custom(&self, code: u8, data: &[u8]) -> Result<Vec<u8>> {
        let response = timeout!(self, call, (Request::Custom(code, data.into())))?;
        match response {
            Response::Custom(_, bytes) => Ok(bytes.to_vec()),
            _ => anyhow::bail!("Unexpected response type."),
        }
    }
}
