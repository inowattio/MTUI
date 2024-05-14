use std::error;
use crate::modbus::{DeviceConfig, ModbusDevice};

const MAX_LINES: usize = 10;

#[derive(Copy, Clone, Debug)]
pub enum FocusType {
    Jump,
    Write
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub position: usize,
    pub focus: Option<FocusType>,
    pub input_number: Option<i32>,
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
            focus: None,
            input_number: None,
            running: true,
            displaying_holding: true,
            position: 0,
            rendered_data: String::new(),
        }
    }

    pub fn switch_focus_to(&mut self, focus: FocusType) {
        self.focus = Some(focus);
    }

    pub fn do_action(&mut self) {
        match self.focus {
            None => self.position += 20,
            Some(focus) => if let Some(number) = self.input_number {
                match focus {
                    FocusType::Jump => self.position = number as usize,
                    FocusType::Write => {
                        let _ = self.device.write_register(self.position as u16, number as u16);
                    }
                };
                self.quit();
            } else {
                self.quit();
            }
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
        if self.focus.is_some() {
            self.focus = None;
        } else {
            self.running = false;
        }
    }

    pub fn refresh(&mut self) {
        let data = if self.displaying_holding {
            self.device.read_holding_registers(self.position as u16, (MAX_LINES + 1) as u16)
        } else {
            self.device.read_input_registers(self.position as u16, (MAX_LINES + 1) as u16)
        };

        let mut rendered_data = format!("{0: >5}: {1: <5} {2: <10} {3: <2}\n", "index", "u16", "u32", "_ascii_");

        match data {
            Ok(data) => {
                for i in 0..MAX_LINES + 1 {
                    let byte = *data.get(i).unwrap_or(&0);
                    let next = *data.get(i + 1).unwrap_or(&0);
                    let word = (byte as u32) << 16 | (next as u32);
                    let as_ascii = format!("_{}_", String::from_utf8_lossy(&[byte as u8, next as u8]));
                    rendered_data.extend(format!("{0: >5}: {byte: <5} {word: <10} {as_ascii: <2}\n", self.position + i).chars());
                }
            }
            Err(e) => {
                rendered_data.extend(e.to_string().chars());
            }
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
