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
    pub header: String,
    pub main_data: String,
    pub pinned_data: String,
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

#[derive(Debug, Eq, PartialEq, Copy, Clone)]
pub enum RegisterType {
    Holding,
    Input,
}

pub type RegisterCell = (RegisterType, usize);
pub type RegisterCellValue = (RegisterCell, u16);

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub refresh_timer: Instant,
    pub running: bool,
    pub position: usize,
    pub state: State,
    pub register_display_type: RegisterType,
    pub pinned_registers: Vec<RegisterCell>,
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
            register_display_type: RegisterType::Holding,
            position: 0,
            refresh_timer: Instant::now(),
            pinned_registers: Vec::default(),
        }
    }

    pub fn switch_focus_to(&mut self, focus: State) {
        self.state = focus;
    }

    pub fn pin(&mut self) {
        if !matches!(self.state, State::Read(_)) {
            return;
        }

        let selection = (self.register_display_type, self.position);

        if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
        } else {
            self.pinned_registers.push(selection);
        }

        self.pinned_registers.sort_by(|(_, a), (_, b)| a.cmp(&b));
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
                    file.write_all(b"\n").await.unwrap();
                    params.started = true;
                }

                let data = self.aquire_data().await.unwrap();
                self.position += self.config.registers_batch as usize;
                file.write_all(self.interpreter.run(data, self.position, |_| None).join("\n").as_bytes()).await.unwrap();
                file.write_all(b"\n").await.unwrap();
            },
        }
    }

    pub fn displaying_type(&self) -> &'static str {
        if self.register_display_type == RegisterType::Holding {
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

    pub async fn aquire_data(&self) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let amount = self.config.registers_batch;

        let values = if self.register_display_type == RegisterType::Holding {
            self.device.holdings(self.position as u16, amount).await?
        } else {
            self.device.inputs(self.position as u16, amount).await?
        };

        Ok(values.into_iter().enumerate().map(|(i, v)| ((self.register_display_type, self.position + i), v)).collect())
    }

    pub async fn aquire_pinned_data(&self) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let mut collection = Vec::with_capacity(self.pinned_registers.len());

        for cell in &self.pinned_registers {
            let cell = cell.clone();
            let (kind, address) = cell;
            let address = address as u16;
            let value = match kind {
                RegisterType::Holding => self.device.holdings(address, 1).await?,
                RegisterType::Input => self.device.inputs(address, 1).await?
            }.into_iter().next().unwrap();

            collection.push((cell, value));
        }

        Ok(collection)
    }

    pub async fn refresh(&mut self) {
        let is_in_read = matches!(self.state, State::Read(_));
        if is_in_read {
            let data = self.aquire_data().await;
            let header = self.interpreter.header();

            let sfr = self.pinned_registers.clone();
            let fav_checker = |cell: RegisterCellValue| {
                let ((c_kind, c_address), _) = cell;
                let is_pinned = sfr.clone().into_iter().position(|(kind, address)| kind == c_kind && address == c_address).is_some();
                if is_pinned {
                    Some("Pinned".into())
                } else {
                    None
                }
            };

            let main_data = match data {
                Ok(data) => self.interpreter.run(data, self.position, fav_checker).join("\n"),
                Err(e) => e.to_string()
            };

            let data = self.aquire_pinned_data().await;
            let pinned_data = match data {
                Ok(data) => {
                    let mut t = String::new();
                    let mut skip = false;

                    for i in 0..data.len() {
                        if skip {
                            skip = false;
                            continue;
                        }

                        let c = *data.get(i).unwrap();
                        let batch = match data.get(i + 1).map(|v| v.clone()) {
                            None => vec![c],
                            Some(n) => {
                                let ((c_kind, c_address), _) = c;
                                let ((n_kind, n_address), _) = n;
                                if n_kind == c_kind && n_address == c_address + 1 {
                                    skip = true;
                                    vec![c, n]
                                } else {
                                    vec![c]
                                }
                            }
                        };
                        let ((_, address), _) = c;

                        let line = self.interpreter.run(batch, address, |c| {
                            let ((kind, _), _) = c;
                            Some(match kind {
                                RegisterType::Holding => "Holding",
                                RegisterType::Input => "Input",
                            }.to_string())
                        }).join("\n");

                        t.push_str(&format!("{line}\n"));
                    }

                    t
                },
                Err(e) => e.to_string()
            };

            if let State::Read(params) = &mut self.state {
                params.header = header;
                params.main_data = main_data;
                params.pinned_data = pinned_data;
            }
        }

        self.refresh_timer = Instant::now();
    }

    pub fn toggle_type(&mut self) {
        if self.register_display_type == RegisterType::Holding {
            self.register_display_type = RegisterType::Input;
        } else {
            self.register_display_type = RegisterType::Holding;
        }
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
