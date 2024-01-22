use std::error;
use crate::data::Data;
use crate::modbus::{DeviceConfig, ModbusDevice};

const MAX_LINES: usize = 10;

/// Application result type.
pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

/// Application.
#[derive(Debug)]
pub struct App {
    /// Is the application running?
    pub running: bool,
    /// counter
    pub position: usize,
    pub displaying_holding: bool,
    pub data: Data,
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
            data: Data::from_json_file("data.json"),
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

    /// Handles the tick event of the terminal.
    pub fn tick(&self) {}

    /// Set running to false to quit the application.
    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn refresh(&mut self) {
        let from_data = if self.displaying_holding {
            &self.data.holding
        } else {
            &self.data.input
        };

        let from = std::cmp::min(self.position, from_data.len().checked_sub(MAX_LINES).unwrap_or(0));
        let to = std::cmp::min(from + MAX_LINES, from_data.len());
        let slice = &from_data[from..to];

        self.rendered_data = slice
            .iter()                           // Create an iterator over the vector
            .map(|reg| {
                let value = if self.displaying_holding {
                    self.device.read_holding_register(reg.address)
                } else {
                    self.device.read_input_register(reg.address)
                };
                let as_string = value.map(|n| n.to_string()).unwrap_or("ERROR".to_string());
                format!("[{} - {}]: {}", reg.address, reg.name, as_string)
            })      // Convert each number to a String
            .collect::<Vec<String>>()         // Collect the strings into a Vec<String>
            .join("\n");
    }

    pub fn toggle_type(&mut self) {
        self.displaying_holding = !self.displaying_holding;
        self.position = 0;
    }

    pub fn up(&mut self) {
        if let Some(res) = self.position.checked_sub(1) {
            self.position = res;
        }
    }

    pub fn down(&mut self) {
        let from_data = if self.displaying_holding {
            &self.data.holding
        } else {
            &self.data.input
        };

        if let Some(res) = self.position.checked_add(1) {
            if res < from_data.len().checked_sub(MAX_LINES).unwrap_or(0) {
                self.position = res;
            }
        }
    }
}
