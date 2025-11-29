use std::{error, fs};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use crate::interpretator::Interpretator;
use crate::modbus::{DeviceConfig, Interface, ModbusDevice};

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub result: Option<String>,
    pub value: Option<i32>,
}

#[derive(Debug, Default, PartialEq)]
pub struct DumpParams {
    pub started: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct JumpParams {
    pub position: Option<i32>
}

#[derive(Debug, Default, PartialEq)]
pub struct ReadParams {
    pub data: String,
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Jump(JumpParams),
    Write(WriteParams),
    Help,
    Dump(DumpParams),
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub refresh_timer: Instant,
    pub running: bool,
    pub position: usize,
    pub state: State,
    pub displaying_holding: bool,
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

impl Config {
    pub fn display_device(&self) -> String {
        match &self.device.interface {
            Interface::Mock => "Mock".to_string(),
            Interface::Wired(p) => format!("Wired {} ({})", p.path, p.baud_rate),
            Interface::Network(p) => format!("Network: {}:{}", p.ip, p.port),
        }
    }
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
    println!("No config file found, dumped example.");
    std::process::exit(0)
}

fn fetch_config_or_exit() -> Config {
    let content = fs::read_to_string("config.json").inspect_err(|_| dump_example_config_and_exit()).unwrap();
    serde_json::from_str(&content).inspect_err(|e| println!("Could not parse config: {e}")).unwrap()
}

impl App {
    pub async fn new() -> Self {
        let config = fetch_config_or_exit();
        let device = ModbusDevice::new(&config.device).await.inspect_err(|e| println!("Could not initialize device: {e}")).unwrap();

        Self {
            interpreter: Interpretator::new(config.interpretations.clone()),
            config,
            device,
            state: State::Read(Default::default()),
            running: true,
            displaying_holding: true,
            position: 0,
            refresh_timer: Instant::now(),
        }
    }

    pub fn switch_focus_to(&mut self, focus: State) {
        self.state = focus;
    }

    pub async fn do_action(&mut self) {
        match &mut self.state {
            State::Read(_) => self.position += self.config.registers_batch as usize,
            State::Jump(params) => if let Some(number) = params.position {
                self.position = number as usize;
                self.quit();
            }
            State::Write(params) => if let Some(number) = params.value {
                let result = self.device.write_register(self.position as u16, number as u16).await;
                params.result = Some(format!("{result:#?}"));
            }
            State::Help => { },
            State::Dump(params) => {
                let mut file = OpenOptions::new().create(true).append(true).open("dump.txt").await.unwrap();

                if !params.started {
                    file.write_all(self.interpreter.header().as_bytes()).await.unwrap();
                    params.started = true;
                }

                let data = self.aquire_data().await.unwrap();
                self.position += self.config.registers_batch as usize;
                file.write_all(self.interpreter.run(data, self.position, false).as_bytes()).await.unwrap();
            },
        }
    }

    pub fn displaying_type(&self) -> &'static str {
        if self.displaying_holding {
            "Holding"
        } else {
            "Input"
        }
    }

    pub async fn tick(&mut self) {
        if let Some(refresh_seconds) = self.config.auto_update_interval_seconds {
            if matches!(self.state, State::Read(_)) {
                if self.refresh_timer.elapsed().as_secs() > refresh_seconds {
                    self.refresh().await;
                }
            } else {
                self.refresh_timer = Instant::now();
            }
        }
    }

    pub fn quit(&mut self) {
        match self.state {
            State::Read(_) => self.running = false,
            _ => self.state = State::Read(Default::default()),
        }
    }

    pub async fn aquire_data(&self) -> Result<Vec<u16>, anyhow::Error> {
        let amount = self.config.registers_batch;

        if self.displaying_holding {
            self.device.holdings(self.position as u16, amount).await
        } else {
            self.device.inputs(self.position as u16, amount).await
        }
    }

    pub async fn refresh(&mut self) {
        let data = self.aquire_data().await;
        if let State::Read(params) = &mut self.state {
            params.data = match data {
                Ok(data) => self.interpreter.run(data, self.position, true),
                Err(e) => e.to_string()
            };
        }

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
