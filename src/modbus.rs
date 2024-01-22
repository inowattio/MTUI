use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio_modbus::client::{rtu, Context, Reader};
use tokio_modbus::slave::Slave;
use tokio_serial::{SerialPort, SerialStream};
use anyhow::Result;

const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub tty_path: String,
    pub baud_rate: u32,
    pub slave_id: u8,
}

#[derive(Debug)]
pub struct ModbusDevice {
    pub config: DeviceConfig,
    context: Context,
}

#[warn(dead_code)] // remove this later on...
impl ModbusDevice {
    pub fn new(config: DeviceConfig) -> Result<Self> {
        let slave = Slave(config.slave_id);
        let builder = tokio_serial::new(&config.tty_path, config.baud_rate).timeout(TIMEOUT);
        let mut port = SerialStream::open(&builder)?;
        port.set_timeout(TIMEOUT)?;
        let context = rtu::attach_slave(port, slave);

        let mut device = ModbusDevice { config, context };

        let _ = device.read_holding_register(0)?;
        Ok(device)
    }

    pub fn read_input_register(&mut self, address: u16) -> Result<u16> {
        let r = futures::executor::block_on(self
            .context
            .read_input_registers(address, 1))?;

        Ok(r[0])
    }

    pub fn read_holding_register(&mut self, address: u16) -> Result<u16> {
        let r = futures::executor::block_on(self
            .context
            .read_holding_registers(address, 1))?;

        Ok(r[0])
    }
}
