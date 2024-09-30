use std::error;
use std::fs::File;
use std::io::BufReader;
use crate::modbus::ModbusDevice;

const MAX_LINES: usize = 1;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum State {
    #[default]
    Read,
    Jump,
    Write
}

#[derive(Default, Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigureTab {
    #[default]
    Wireless,
    Wired,
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
pub struct App {
    pub running: bool,
    pub position: usize,
    pub state: State,
    pub input_number: Option<i32>,
    pub displaying_holding: bool,
    pub rendered_data: String,
    pub device: ModbusDevice,
}

impl App {
    pub async fn new() -> Self {
        let file = File::open("config.json").unwrap();
        let reader = BufReader::new(file);
        let config = serde_json::from_reader(reader).unwrap();

        Self {
            device: ModbusDevice::new(&config).await.unwrap(),
            state: State::default(),
            input_number: None,
            running: true,
            displaying_holding: true,
            position: 0,
            rendered_data: String::new(),
        }
    }

    pub fn switch_focus_to(&mut self, focus: State) {
        self.state = focus;
    }

    pub async fn do_action(&mut self) {
        match self.state {
            State::Read => self.position += 20,
            State::Jump => if let Some(number) = self.input_number {
                self.position = number as usize
            }
            State::Write => if let Some(number) = self.input_number {
                self.device.write_register(self.position as u16, number as u16).await.unwrap();
            }
        }

        if self.state == State::Write || self.state == State::Jump {
            self.quit();
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
        match self.state {
            State::Read => self.running = false,
            _ => self.state = State::Read,
        }
    }

    pub async fn refresh(&mut self) {
        const AMOUNT: usize = MAX_LINES + 1;

        let data = if self.displaying_holding {
            self.device.holdings::<AMOUNT>(self.position as u16).await
        } else {
            self.device.inputs::<AMOUNT>(self.position as u16).await
        };

        let mut rendered_data = format!("{0: >5}: {1: <5} {2: <10} {3: <10} {4: <2}\n", "index", "u16", "u32", "i32", "_ascii_");

        match data {
            Ok(data) => {
                for i in 0..MAX_LINES + 1 {
                    let byte = *data.get(i).unwrap_or(&0);
                    let next = *data.get(i + 1).unwrap_or(&0);
                    let word = (byte as u32) << 16 | (next as u32);
                    let iword = word as i32;
                    let as_ascii = format!("_{}_", String::from_utf8_lossy(&[byte as u8, next as u8]));
                    rendered_data.extend(format!("{0: >5}: {byte: <5} {word: <10} {iword: <10} {as_ascii: <2}\n", self.position + i).chars());
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
