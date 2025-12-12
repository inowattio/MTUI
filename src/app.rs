use std::{error, fs};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use crate::interpretator::Interpretator;
use crate::modbus::{DeviceConfig, Interface, ModbusDevice};

const CONFIG_PATH: &str = "config.json";

#[derive(Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord
}

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<String>,
    pub value: Option<i32>,
    pub write_type: WriteType
}

#[derive(Debug, PartialEq, Default)]
pub struct DumpParams {
    pub started: bool,
    pub total_batches: Option<u16>,
    pub completed_batches: u16,
    pub start_position: u16,
    pub position: u16,
    pub header_written: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct JumpParams {
    pub from: u16,
    pub to: u16
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub header: String,
    pub main_data: String,
    pub pinned_data: String,
    pub refresh_timer: Instant,
}

impl Default for ReadParams {
    fn default() -> Self {
        Self {
            position: 0,
            header: "".to_string(),
            main_data: "".to_string(),
            pinned_data: "".to_string(),
            refresh_timer: Instant::now(),
        }
    }
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

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, Serialize, Deserialize)]
pub enum RegisterType {
    Holding,
    Input,
}

pub type RegisterCell = (RegisterType, u16);
pub type RegisterCellValue = (RegisterCell, u16);

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub running: bool,
    pub state: State,
    pub register_display_type: RegisterType,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: ModbusDevice,
    pub interpreter: Interpretator,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct PinnedRegisters {
    pub holdings: Vec<u16>,
    pub inputs: Vec<u16>,
}

impl From<PinnedRegisters> for Vec<RegisterCell> {
    fn from(value: PinnedRegisters) -> Self {
        let mut collection = Vec::new();

        for holding in value.holdings {
            collection.push((RegisterType::Holding, holding));
        }

        for input in value.inputs {
            collection.push((RegisterType::Input, input));
        }

        collection
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    pub device: DeviceConfig,
    pub interpretations: Interpretations,
    pub registers_batch: u16,
    pub auto_update_interval_seconds: Option<u64>,
    pub dump_file: String,
    pub pinned_defaults: PinnedRegisters,
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
            dump_file: "dump.txt".into(),
            pinned_defaults: Default::default(),
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

    fs::write(CONFIG_PATH, config_string).unwrap();
    println!("No config file found, dumped example.");
    std::process::exit(0)
}

fn fetch_config_or_exit() -> Config {
    let content = fs::read_to_string(CONFIG_PATH).inspect_err(|_| dump_example_config_and_exit()).unwrap();
    serde_json::from_str(&content).inspect_err(|e| println!("Could not parse config: {e}")).unwrap()
}

impl App {
    pub async fn new() -> Self {
        let config = fetch_config_or_exit();
        let device = ModbusDevice::new(&config.device).await.inspect_err(|e| println!("Could not initialize device: {e}")).unwrap();

        Self {
            interpreter: Interpretator::new(config.interpretations.clone()),
            pinned_registers: config.pinned_defaults.clone().into(),
            config,
            device,
            state: State::Read(Default::default()),
            running: true,
            register_display_type: RegisterType::Holding,
        }
    }

    pub fn get_current_position(&self) -> u16 {
        match &self.state {
            State::Read(p) => p.position,
            State::Jump(p) => p.to,
            State::Write(p) => p.position,
            State::Dump(p) => p.start_position,
            State::Help => 0,
        }
    }

    pub fn switch_focus_to(&mut self, focus: State) {
        let position = self.get_current_position();

        self.state = match focus {
            State::Dump(_) => State::Dump(DumpParams {
                start_position: position,
                ..Default::default()
            }),
            State::Write(_) => State::Write(WriteParams {
                position,
                ..Default::default()
            }),
            State::Jump(_) => State::Jump(JumpParams {
                from: position,
                ..Default::default()
            }),
            State::Read(_) => State::Read(ReadParams {
                position,
                ..Default::default()
            }),
            State::Help => State::Help,
        };
    }

    pub fn pin(&mut self) {
        let position = if let State::Read(p) = &self.state {
            p.position
        } else {
            return;
        };

        let selection = (self.register_display_type, position);

        if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
        } else {
            self.pinned_registers.push(selection);
        }

        self.pinned_registers.sort();
    }

    pub async fn do_action(&mut self) {
        let mut start_dump_run = false;

        match &mut self.state {
            State::Read(p) => p.position += self.config.registers_batch,
            State::Jump(_) => self.switch_focus_to(State::Read(Default::default())),
            State::Write(params) => if let Some(number) = params.value {
                let result = match params.write_type {
                    WriteType::Word => self.device.write_register(params.position, number as u16).await,
                    WriteType::DWord => self.device.write_register_word(params.position, number).await,
                };
                params.result = Some(format!("{result:#?}"));
            }
            State::Help => { },
            State::Dump(params) => {
                if params.started {
                    return;
                }

                let Some(total_batches) = params.total_batches else {
                    params.error = Some("Set a batch count before starting.".to_string());
                    return;
                };

                if total_batches < 1 {
                    params.error = Some("Batch count must be greater than 0.".to_string());
                    return;
                }

                params.started = true;
                params.completed_batches = 0;
                params.header_written = false;
                params.error = None;
                params.position = params.start_position;
                start_dump_run = true;
            },
        }

        if start_dump_run {
            if let Err(e) = self.perform_dump_batch().await {
                if let State::Dump(params) = &mut self.state {
                    params.started = false;
                    params.error = Some(e.to_string());
                }
            }
        }
    }

    async fn perform_dump_batch(&mut self) -> Result<(), anyhow::Error> {
        let (header_needed, starting_index, total_batches, dump_file) = {
            let State::Dump(params) = &mut self.state else {
                return Ok(());
            };

            let Some(total_batches) = params.total_batches else {
                return Ok(());
            };

            if params.completed_batches >= total_batches {
                params.started = false;
                return Ok(());
            }

            (!params.header_written, params.position, total_batches, self.config.dump_file.clone())
        };

        let mut file = OpenOptions::new().create(true).append(true).open(&dump_file).await?;

        if header_needed {
            file.write_all(self.interpreter.header().as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        let data = self.aquire_data().await?;
        file.write_all(self.interpreter.run(data, starting_index, |_| None).join("\n").as_bytes()).await?;
        file.write_all(b"\n").await?;

        if let State::Dump(params) = &mut self.state {
            params.header_written = true;
            params.completed_batches += 1;
            params.position = starting_index + self.config.registers_batch;

            if params.completed_batches >= total_batches {
                params.started = false;
            }
        }

        Ok(())
    }

    pub fn displaying_type(&self) -> &'static str {
        if self.register_display_type == RegisterType::Holding {
            "Holding"
        } else {
            "Input"
        }
    }

    pub async fn tick(&mut self) {
        let should_perform_dump = matches!(&self.state, State::Dump(p) if p.started);

        match &mut self.state {
            State::Read(p) => {
                let refresh_seconds = if let Some(refresh_seconds) = self.config.auto_update_interval_seconds {
                    refresh_seconds
                } else {
                    return;
                };

                if p.refresh_timer.elapsed().as_secs() > refresh_seconds {
                    self.refresh().await;
                }
            },
            State::Dump(_) => {
                if should_perform_dump {
                    if let Err(e) = self.perform_dump_batch().await {
                        if let State::Dump(p) = &mut self.state {
                            p.started = false;
                            p.error = Some(e.to_string());
                        }
                    }
                }
            },
            _ => {}
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
        let position = self.get_current_position();

        let values = if self.register_display_type == RegisterType::Holding {
            self.device.holdings(position, amount).await?
        } else {
            self.device.inputs(position, amount).await?
        };

        Ok(values.into_iter().enumerate().map(|(i, v)| ((self.register_display_type, position + i as u16), v)).collect())
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
        let position = if let State::Read(p) = &mut self.state {
            p.refresh_timer = Instant::now();
            p.position
        } else {
            return;
        };

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
            Ok(data) => self.interpreter.run(data, position, fav_checker).join("\n"),
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

    pub fn toggle_type(&mut self) {
        if let State::Dump(params) = &mut self.state {
            if params.started {
                return;
            }
        }

        if self.register_display_type == RegisterType::Holding {
            self.register_display_type = RegisterType::Input;
        } else {
            self.register_display_type = RegisterType::Holding;
        }
    }
}
