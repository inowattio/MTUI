use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio_modbus::client::{rtu, tcp, Context, Reader, Writer};
use tokio_modbus::slave::Slave;
use tokio_serial::{available_ports, DataBits, Parity, SerialPortType, SerialStream, StopBits};
use anyhow::{Error, Result};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub enum Interface {
    Wired(InterfaceWiredParams),
    Wireless(InterfaceWirelessParams)
}

#[derive(Clone, Debug)]
pub struct InterfaceWiredParams {
    pub path: String,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

#[derive(Clone, Debug)]
pub struct InterfaceWirelessParams {
    pub ip: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
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
                .map_err(|e| anyhow::Error::from(e))
        }
    };
}

#[derive(Clone, Debug)]
pub struct ModbusDevice {
    context: Arc<Mutex<Context>>,
    config: DeviceConfig,
}

pub enum InterfaceScan {
    Wired,
    Wireless
}

impl ModbusDevice {
    pub async fn new(config: &DeviceConfig) -> Result<Self> {
        let timeout_connect = Duration::from_millis(config.timeout_connect_ms);
        let slave = Slave(config.slave_id);

        let context = match &config.interface {
            Interface::Wired(interface) => {
                let builder = tokio_serial::new(&interface.path, interface.baud_rate)
                    .timeout(timeout_connect)
                    .data_bits(interface.data_bits)
                    .parity(interface.parity)
                    .stop_bits(interface.stop_bits);

                let port = SerialStream::open(&builder)?;
                rtu::attach_slave(port, slave)
            }
            Interface::Wireless(interface) => {
                let socket_addr = SocketAddr::V4(SocketAddrV4::new(
                    Ipv4Addr::from_str(&interface.ip)?,
                    interface.port,
                ));
                let connection = tcp::connect_slave(socket_addr, slave);
                let con = timeout(connection, timeout_connect, Duration::default())
                    .await??;

                tokio::time::sleep(Duration::from_secs(2)).await;
                // TODO: hmmm

                con
            }
        };

        Ok(Self {
            context: Arc::new(Mutex::new(context)),
            config: config.clone(),
        })
    }

    pub async fn scan_wireless() -> Result<Vec<DeviceConfig>> {
        let mut configs = Vec::new();

        const ADDRESSES: [(&str, u16); 1] = [
            ("192.168.200.1", 6607), // inverter
            //"192.168.1.128:502",  // dongle
        ];
        const SLAVE_IDS: [u8; 1] = [0];

        for (address, port) in ADDRESSES {
            for slave_id in SLAVE_IDS {
                configs.push(DeviceConfig {
                    interface: Interface::Wireless(InterfaceWirelessParams {
                        ip: address.to_string(),
                        port,
                    }),
                    slave_id,
                    timeout_connect_ms: 3000,
                    timeout_command_ms: 1000,
                    time_between_commands_ms: 5,
                })
            }
        }

        Ok(configs)
    }

    pub async fn scan_wired() -> Result<Vec<DeviceConfig>> {
        let mut configs = Vec::new();

        const DISCOVER_BAUDS: [u32; 1] = [9600];
        const SLAVE_IDS: [u8; 3] = [1, 2, 3];

        let ports = available_ports()?
            .into_iter()
            .filter(|p| {
                matches!(
                    p.port_type,
                    SerialPortType::UsbPort(_) | SerialPortType::Unknown
                )
            });

        for device in ports {
            for baud_rate in DISCOVER_BAUDS {
                for slave_id in SLAVE_IDS {
                    configs.push(DeviceConfig {
                        interface: Interface::Wired(InterfaceWiredParams {
                            path: device.port_name.clone(),
                            baud_rate,
                            data_bits: DataBits::Eight,
                            parity: Parity::None,
                            stop_bits: StopBits::One,
                        }),
                        slave_id,
                        timeout_connect_ms: 850,
                        timeout_command_ms: 850,
                        time_between_commands_ms: 5,
                    });
                }
            }
        }

        Ok(configs)
    }

    pub async fn scan(interface: InterfaceScan) -> Result<Vec<DeviceConfig>> {
        let configs = match interface {
            InterfaceScan::Wired => Self::scan_wired().await,
            InterfaceScan::Wireless => Self::scan_wireless().await,
        }?;

        let mut devices = Vec::new();
        for config in configs {
            if Self::new(&config).await.is_ok() {
                devices.push(config);
            }
        }

        Ok(devices)
    }

    pub async fn coils(&self, address: u16, count: u16) -> Result<Vec<bool>> {
        timeout!(self, read_coils, (address, count))
    }

    pub async fn discretes(&self, address: u16, count: u16) -> Result<Vec<bool>> {
        timeout!(self, read_discrete_inputs, (address, count))
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
}
