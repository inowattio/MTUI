use std::{error, fs};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use crate::interpretator::Interpretator;
use crate::modbus::{DeviceConfig, Interface, ModbusDevice};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum State {
    #[default]
    Read,
    Jump,
    Write,
    Help,
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
    pub config: Config,
    pub refresh_timer: Instant,
    pub running: bool,
    pub position: usize,
    pub state: State,
    pub input_number: Option<i32>,
    pub displaying_holding: bool,
    pub rendered_data: String,
    pub device: ModbusDevice,
    pub interpreter: Interpretator,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub device: DeviceConfig,
    pub interpretations: Interpretations,
    pub registers_batch: u16,
    pub auto_update_interval_seconds: Option<u64>
}

impl Default for Config {
    fn default() -> Self {
        Self {
            device: DeviceConfig {
                interface: Interface::Mock,
                slave_id: 0,
                timeout_connect_ms: 1000,
                timeout_command_ms: 2000,
                time_between_commands_ms: 3,
            },
            interpretations: Interpretations {
                u32: true,
                i32: true,
                f32: false,
                ascii: true,
                bits: false,
            },
            registers_batch: 4,
            auto_update_interval_seconds: Some(1),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Interpretations {
    pub u32: bool,
    pub i32: bool,
    pub f32: bool,
    pub ascii: bool,
    pub bits: bool,
}

fn dump_example_config_and_exit() {
    let example_config = Config::default();
    let config_string = serde_json::to_string_pretty(&example_config).unwrap();

    fs::write("config.json", config_string).unwrap();
    std::process::exit(0)
}

fn fetch_config_or_exit() -> Config {
    let content = fs::read_to_string("config.json").inspect_err(|_| dump_example_config_and_exit()).unwrap();
    serde_json::from_str(&content).inspect_err(|e| println!("Could not parse config: {e}")).unwrap()
}

impl App {
    pub async fn new() -> Self {
        let config = fetch_config_or_exit();
        let device = ModbusDevice::new(&config.device).await.unwrap();

        Self {
            interpreter: Interpretator::new(config.interpretations.clone()),
            config,
            device,
            state: State::default(),
            input_number: None,
            running: true,
            displaying_holding: true,
            position: 0,
            rendered_data: String::new(),
            refresh_timer: Instant::now(),
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
            State::Help => { },
        }

        if self.state != State::Jump {
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

    pub async fn tick(&mut self) {
        if let Some(refresh_seconds) = self.config.auto_update_interval_seconds {
            if self.refresh_timer.elapsed().as_secs() > refresh_seconds {
                self.refresh().await;
            }
        }
    }

    pub fn quit(&mut self) {
        match self.state {
            State::Read => self.running = false,
            _ => self.state = State::Read,
        }
    }

    pub async fn refresh(&mut self) {
        let amount = self.config.registers_batch;

        let data = if self.displaying_holding {
            self.device.holdings(self.position as u16, amount).await
        } else {
            self.device.inputs(self.position as u16, amount).await
        };

        self.rendered_data = match data {
            Ok(data) => self.interpreter.run(data, self.position),
            Err(e) => e.to_string()
        };

        self.refresh_timer = Instant::now();
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
