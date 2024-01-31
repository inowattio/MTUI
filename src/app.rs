use std::error;
use crate::modbus::{DeviceConfig, ModbusDevice};

const MAX_LINES: usize = 10;

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub position: usize,
    pub displaying_holding: bool,
    pub rendered_data: String,
    pub device: ModbusDevice,
}

impl App {
    pub fn new() -> Self {
        Self {
            device: ModbusDevice::new(DeviceConfig {
                tty_path: "/dev/ttyUSB0".to_string(),
                baud_rate: 9600,
                slave_id: 1,
            }).unwrap(),
            running: true,
            displaying_holding: true,
            position: 0,
            rendered_data: String::new(),
        }
    }

    pub fn displaying_type(&self) -> String {
        if self.displaying_holding {
            String::from("Holding")
        } else {
            String::from("Input")
        }
    }

    pub fn tick(&self) {}

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn refresh(&mut self) {
        let _from = self.position;
        let _to = self.position + MAX_LINES;

        let data = if self.displaying_holding {
            self.device.read_holding_registers(self.position as u16, (MAX_LINES + 1) as u16)
        } else {
            self.device.read_input_registers(self.position as u16, (MAX_LINES + 1) as u16)
        }.unwrap();

        let mut rendered_data = format!("{0: >5}: {1: <5} u32\n", "index", "u16");
        for i in 0..MAX_LINES + 1 {
            let byte = *data.get(i).unwrap_or(&0) as u8;
            let next = *data.get(i + 1).unwrap_or(&0) as u8;
            let word = ((next as u16) << 8) | byte as u16;
            rendered_data.extend(format!("{0: >5}: {byte: <5} {word}\n", self.position + i).chars());
        }

        self.rendered_data = rendered_data;
    }

    pub fn toggle_type(&mut self) {
        self.displaying_holding = !self.displaying_holding;
    }

    pub fn up(&mut self) {
        if let Some(res) = self.position.checked_sub(1) {
            self.position = res;
        }
    }

    pub fn down(&mut self) {
        if let Some(res) = self.position.checked_add(1) {
            self.position = res;
        }
    }
}
