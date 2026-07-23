use crate::compat;
use crate::mock::MockContext;
use crate::register::RegisterType;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
#[cfg(not(target_arch = "wasm32"))]
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
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

macro_rules! serial_enum {
    (
        $name:ident => $native:path, $err:literal,
        { $( $variant:ident = $code:literal => $nvariant:ident ),+ $(,)? }
    ) => {
        #[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
        pub enum $name {
            $( $variant ),+
        }

        #[cfg(not(target_arch = "wasm32"))]
        impl From<$name> for $native {
            fn from(val: $name) -> Self {
                match val {
                    $( $name::$variant => <$native>::$nvariant ),+
                }
            }
        }

        impl From<$name> for u8 {
            fn from(val: $name) -> Self {
                match val {
                    $( $name::$variant => $code ),+
                }
            }
        }

        impl TryFrom<u8> for $name {
            type Error = &'static str;

            fn try_from(value: u8) -> std::result::Result<Self, Self::Error> {
                Ok(match value {
                    $( $code => Self::$variant, )+
                    _ => Err($err)?,
                })
            }
        }
    };
}

serial_enum!(DataBits => tokio_serial::DataBits, "Failed to parse data bits", {
    Five = 5 => Five,
    Six = 6 => Six,
    Seven = 7 => Seven,
    Eight = 8 => Eight,
});

serial_enum!(Parity => tokio_serial::Parity, "Failed to parse parity", {
    None = 0 => None,
    Odd = 1 => Odd,
    Even = 2 => Even,
});

serial_enum!(StopBits => tokio_serial::StopBits, "Failed to parse stop bits", {
    One = 1 => One,
    Two = 2 => Two,
});

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

    fn ordered<T: num_traits::PrimInt>(self, a: T, b: T) -> (T, T) {
        let (byte_swap, word_swap) = self.swaps();
        let (mut high, mut low) = if word_swap { (b, a) } else { (a, b) };
        if byte_swap {
            high = high.swap_bytes();
            low = low.swap_bytes();
        }
        (high, low)
    }

    pub fn make_word(self, a: u16, b: u16) -> u32 {
        let (high, low) = self.ordered(a, b);
        ((high as u32) << 16) | (low as u32)
    }

    pub fn make_dword(self, a: u32, b: u32) -> u64 {
        let (high, low) = self.ordered(a, b);
        ((high as u64) << 32) | (low as u64)
    }

    pub fn split_word(self, data: u32) -> [u16; 2] {
        let (first, second) = self.ordered((data >> 16) as u16, data as u16);
        [first, second]
    }

    pub fn assemble(self, words: &[u16]) -> Option<u64> {
        Some(match words.len() {
            1 => u64::from(*words.first()?),
            2 => u64::from(self.make_word(*words.first()?, *words.get(1)?)),
            _ => self.make_dword(
                self.make_word(*words.first()?, *words.get(1)?),
                self.make_word(*words.get(2)?, *words.get(3)?),
            ),
        })
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
    #[cfg(not(target_arch = "wasm32"))]
    use super::{DeviceConfig, Interface, InterfaceNetworkParams, ModbusDevice, RegisterType};
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::Arc;
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::Duration;

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

    #[cfg(not(target_arch = "wasm32"))]
    async fn wait_for(counter: &AtomicUsize, target: usize) {
        for _ in 0..200 {
            if counter.load(Ordering::Relaxed) >= target {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!(
            "connection count never reached {target} (got {})",
            counter.load(Ordering::Relaxed)
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn poisoned_context_reconnects_before_next_command() {
        // A mock device stands in for a real transport. Simulate a prior
        // command having timed out and desynchronized the context.
        let device = ModbusDevice::new(&DeviceConfig::default()).await.unwrap();
        device.poisoned.store(true, Ordering::Relaxed);

        // The next command must rebuild the context, clear the flag, and still
        // return a value.
        let result = device.read_typed(None, RegisterType::Holding, 0, 1).await;
        assert!(result.is_ok(), "command after reconnect failed: {result:?}");
        assert!(
            !device.poisoned.load(Ordering::Relaxed),
            "reconnect must clear the poison flag"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn command_timeout_poisons_and_next_command_reconnects() {
        // A server that accepts TCP connections but never replies, so every
        // Modbus command times out. Counting accepted connections lets us
        // observe reconnects.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let counter = connections.clone();
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                counter.fetch_add(1, Ordering::Relaxed);
                held.push(stream); // keep the socket open but stay silent
            }
        });

        let config = DeviceConfig {
            interface: Interface::Network(InterfaceNetworkParams {
                ip: "127.0.0.1".to_string(),
                port: addr.port(),
            }),
            slave_id: 1,
            timeout_connect_ms: 500,
            timeout_command_ms: 100,
            time_between_commands_ms: 0,
            word_order: WordOrder::default(),
        };

        let device = ModbusDevice::new(&config)
            .await
            .expect("connect to loopback");
        wait_for(&connections, 1).await;
        assert!(!device.poisoned.load(Ordering::Relaxed));

        // Silent server: the command times out and poisons the context. No
        // reconnect happens yet, so the connection count stays at one.
        assert!(
            device
                .read_typed(None, RegisterType::Holding, 0, 1)
                .await
                .is_err(),
            "expected a timeout"
        );
        assert!(
            device.poisoned.load(Ordering::Relaxed),
            "a timed-out command must poison the context"
        );
        assert_eq!(
            connections.load(Ordering::Relaxed),
            1,
            "no reconnect expected yet"
        );

        // Poisoned: the next command must reconnect (a second TCP connection)
        // before touching the wire, discarding the desynchronized transport.
        assert!(device
            .read_typed(None, RegisterType::Holding, 0, 1)
            .await
            .is_err());
        wait_for(&connections, 2).await;
        assert_eq!(
            connections.load(Ordering::Relaxed),
            2,
            "poisoned context must reconnect before the next command"
        );
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

macro_rules! timeout_as {
    ($this:ident, $slave:expr, $action:ident, ($($arg:expr),* $(,)?), $desc:expr) => {
        {
            let mut hold = $this.context.lock().await;

            if $this.poisoned.load(Ordering::Relaxed) {
                log::info!("Reconnecting after timeout");
                let mut fresh = ModbusDevice::connect_context(&$this.config).await?;
                fresh.set_slave(Slave($this.default_slave.load(Ordering::Relaxed)));
                *hold = fresh;
                $this.poisoned.store(false, Ordering::Relaxed);
            }

            let timeout_command = Duration::from_millis($this.config.timeout_command_ms);
            let time_between = Duration::from_millis($this.config.time_between_commands_ms);
            let override_slave = $slave;
            if let Some(id) = override_slave {
                hold.set_slave(Slave(id));
            }
            let outcome =
                timeout(hold.$action($($arg),*), timeout_command, time_between).await;
            if override_slave.is_some() {
                hold.set_slave(Slave($this.default_slave.load(Ordering::Relaxed)));
            }
            let desc = match override_slave {
                Some(id) => format!("{} (slave {id})", $desc),
                None => $desc,
            };
            match outcome {
                Ok(Ok(Ok(value))) => {
                    match format!("{value:?}").as_str() {
                        "()" => log::info!("{desc} \u{b7} ok"),
                        shown => log::info!("{desc} \u{b7} ok \u{b7} {shown}"),
                    }
                    Ok(value)
                }
                Ok(Ok(Err(error))) => {
                    log::warn!("{desc} \u{b7} {error}");
                    Err(anyhow::Error::from(error))
                }
                Ok(Err(error)) => {
                    log::warn!("{desc} \u{b7} {error}");
                    Err(anyhow::Error::from(error))
                }
                Err(error) => {
                    $this.poisoned.store(true, Ordering::Relaxed);
                    log::warn!("{desc} \u{b7} timed out");
                    Err(error)
                }
            }
        }
    };
}

macro_rules! timeout {
    ($this:ident, $action:ident, ($($arg:expr),* $(,)?), $desc:expr) => {
        timeout_as!($this, None::<SlaveId>, $action, ($($arg),*), $desc)
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
    default_slave: Arc<AtomicU8>,
    poisoned: Arc<AtomicBool>,
}

impl Debug for ModbusDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ModbusDevice {{ config: {:?} }}", self.config)
    }
}

impl ModbusDevice {
    pub async fn new(config: &DeviceConfig) -> Result<Self> {
        let mut context = Self::connect_context(config).await?;
        context.set_slave(Slave(config.slave_id));

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            default_slave: Arc::new(AtomicU8::new(config.slave_id)),
            poisoned: Arc::new(AtomicBool::new(false)),
            config: config.clone(),
        })
    }

    async fn connect_context(config: &DeviceConfig) -> Result<Context> {
        let timeout_connect = Duration::from_millis(config.timeout_connect_ms);
        #[cfg(target_arch = "wasm32")]
        let _ = timeout_connect;

        #[cfg(not(target_arch = "wasm32"))]
        let context = match &config.interface {
            Interface::Wired(interface) => {
                let builder = tokio_serial::new(&interface.path, interface.baud_rate)
                    .timeout(timeout_connect)
                    .data_bits(interface.data_bits.into())
                    .parity(interface.parity.into())
                    .stop_bits(interface.stop_bits.into());

                let port = SerialStream::open(&builder)?;
                rtu::attach_slave(port, Slave(config.slave_id))
            }
            Interface::Network(interface) => {
                let socket_addr = SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from_str(&interface.ip)?,
                    interface.port,
                ));
                let connection = tcp::connect_slave(socket_addr, Slave(config.slave_id));
                timeout(connection, timeout_connect, Duration::default()).await??
            }
            Interface::Mock => MockContext::make(),
        };

        #[cfg(target_arch = "wasm32")]
        let context = MockContext::make();

        Ok(context)
    }

    pub async fn set_slave(&self, slave_id: SlaveId) {
        self.default_slave.store(slave_id, Ordering::Relaxed);
        self.context.lock().await.set_slave(Slave(slave_id));
    }

    pub async fn read_typed(
        &self,
        slave: Option<SlaveId>,
        register_type: RegisterType,
        address: u16,
        count: u16,
    ) -> Result<Vec<u16>> {
        let bits_to_words = |bits: Vec<bool>| bits.into_iter().map(u16::from).collect();
        let desc = format!("Read {register_type:?} @ {address} with {count} value(s)");
        match register_type {
            RegisterType::Holding => {
                timeout_as!(self, slave, read_holding_registers, (address, count), desc)
            }
            RegisterType::Input => {
                timeout_as!(self, slave, read_input_registers, (address, count), desc)
            }
            RegisterType::Coil => {
                timeout_as!(self, slave, read_coils, (address, count), desc).map(bits_to_words)
            }
            RegisterType::Discrete => {
                timeout_as!(self, slave, read_discrete_inputs, (address, count), desc)
                    .map(bits_to_words)
            }
        }
    }

    pub async fn write_typed(
        &self,
        slave: Option<SlaveId>,
        register_type: RegisterType,
        address: u16,
        values: &[u16],
    ) -> Result<()> {
        let desc = format!("Write {register_type:?} @ {address} with values {values:?}");
        match register_type {
            RegisterType::Coil => {
                let coils: Vec<bool> = values.iter().map(|&v| v != 0).collect();
                timeout_as!(self, slave, write_multiple_coils, (address, &coils), desc)
            }
            _ => timeout_as!(
                self,
                slave,
                write_multiple_registers,
                (address, values),
                desc
            ),
        }
    }

    pub fn set_word_order(&mut self, word_order: WordOrder) {
        self.config.word_order = word_order;
    }

    pub async fn write_register(&self, address: u16, data: u16) -> Result<()> {
        timeout!(
            self,
            write_single_register,
            (address, data),
            format!("Write Holding @ {address} with value {data}")
        )
    }

    pub async fn write_registers(&self, address: u16, data: &[u16]) -> Result<()> {
        timeout!(
            self,
            write_multiple_registers,
            (address, data),
            format!("Write Holding @ {address} with values {data:?}")
        )
    }

    pub async fn write_coil(&self, address: u16, data: bool) -> Result<()> {
        timeout!(
            self,
            write_single_coil,
            (address, data),
            format!("Write Coil @ {address} with value {data}")
        )
    }

    pub async fn write_register_word(&self, address: u16, data: i32) -> Result<()> {
        let words = self.config.word_order.split_word(data as u32);
        self.write_registers(address, &words).await
    }

    pub async fn device_identification(
        &self,
        access: DeviceIdAccess,
        object_id: u8,
    ) -> Result<ReadDeviceIdentificationResponse> {
        timeout!(
            self,
            read_device_identification,
            (access.into_read_code(), object_id),
            format!(
                "Read device identification {} object {object_id}",
                access.label()
            )
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
        let response = timeout!(
            self,
            call,
            (Request::Custom(code, data.into())),
            format!("Raw function 0x{code:02X} with data {data:?}")
        )?;
        match response {
            Response::Custom(_, bytes) => Ok(bytes.to_vec()),
            _ => anyhow::bail!("Unexpected response type."),
        }
    }
}
