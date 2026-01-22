use std::{error, fs};
use std::time::Instant;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use crate::config::Config;
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{no_data_text, DumpParams, JumpParams, ReadParams, State, StateTransition, WriteParams};

#[derive(Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub running: bool,
    pub state: State,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: ModbusDevice,
    pub interpreter: Interpretor,
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
            interpreter: Interpretor::new(config.interpretations.clone()),
            pinned_registers: config.pinned_defaults.clone().into(),
            config,
            device,
            state: State::Read(Default::default()),
            running: true,
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

    pub fn switch_focus_to(&mut self, focus: StateTransition) {
        let position = self.get_current_position();
        let register_type = match &self.state {
            State::Read(p) => p.register_type,
            State::Dump(p) => p.register_type,
            _ => Default::default()
        };

        self.state = match focus {
            StateTransition::Dump => State::Dump(DumpParams {
                start_position: position,
                register_type,
                ..Default::default()
            }),
            StateTransition::Write => State::Write(WriteParams {
                position,
                ..Default::default()
            }),
            StateTransition::Jump => State::Jump(JumpParams {
                from: position,
                ..Default::default()
            }),
            StateTransition::Read => State::Read(ReadParams {
                position,
                register_type,
                ..Default::default()
            }),
            StateTransition::Help => State::Help,
        };
    }

    pub fn pin(&mut self) {
        let (position, register_display_type) = if let State::Read(p) = &self.state {
            (p.position, p.register_type)
        } else {
            return;
        };

        let selection = (register_display_type, position);

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
            State::Jump(_) => self.switch_focus_to(StateTransition::Read),
            State::Write(params) => if let Some(number) = params.value {
                let result = match params.write_type {
                    WriteType::Word => self.device.write_register(params.position, number as u16).await,
                    WriteType::DWord => self.device.write_register_word(params.position, number).await,
                };
                params.result = Some(format!("{result:#?}"));
            }
            State::Help => self.quit(),
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
        let (header_needed, starting_index, total_batches, dump_file, register_type) = {
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

            (!params.header_written, params.position, total_batches, self.config.dump_file.clone(), params.register_type)
        };

        let mut file = OpenOptions::new().create(true).append(true).open(&dump_file).await?;

        if header_needed {
            file.write_all(self.interpreter.header().as_bytes()).await?;
            file.write_all(b"\n").await?;
        }

        let data = self.aquire_data(register_type).await?;
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

    pub async fn aquire_data(&self, register_type: RegisterType) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let amount = self.config.registers_batch;
        let position = self.get_current_position();

        let values = if register_type == RegisterType::Holding {
            self.device.holdings(position, amount).await?
        } else {
            self.device.inputs(position, amount).await?
        };

        Ok(values.into_iter().enumerate().map(|(i, v)| ((register_type, position + i as u16), v)).collect())
    }

    pub async fn aquire_pinned_data(&self) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let regs = &self.pinned_registers;
        let mut collection = Vec::with_capacity(regs.len());

        let mut i = 0usize;
        while i < regs.len() {
            let (kind, start_addr_raw) = regs[i].clone();
            let start_addr = start_addr_raw;

            let mut run_len = 1usize;
            while i + run_len < regs.len() {
                let (next_kind, next_addr_raw) = regs[i + run_len].clone();
                let next_addr = next_addr_raw;

                if next_kind == kind && next_addr == start_addr + (run_len as u16) {
                    run_len += 1;
                } else {
                    break;
                }
            }

            let values = match kind {
                RegisterType::Holding => self.device.holdings(start_addr, run_len as u16).await?,
                RegisterType::Input => self.device.inputs(start_addr, run_len as u16).await?,
            };

            for j in 0..run_len {
                let cell = regs[i + j].clone();
                let value = values
                    .get(j)
                    .cloned()
                    .unwrap();

                collection.push((cell, value));
            }

            i += run_len;
        }

        Ok(collection)
    }

    pub async fn refresh(&mut self) {
        let (position, register_type) = if let State::Read(p) = &mut self.state {
            p.refresh_timer = Instant::now();
            (p.position, p.register_type)
        } else {
            return;
        };

        let data = self.aquire_data(register_type).await;

        let sfr = &self.pinned_registers;
        let fav_checker = |cell: RegisterCellValue| {
            let ((c_kind, c_address), _) = cell;
            let is_pinned = sfr.iter().any(|(kind, address)| *kind == c_kind && *address == c_address);
            if is_pinned {
                Some("Pinned".to_string())
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
                let mut lines = Vec::new();
                let mut skip = false;

                for i in 0..data.len() {
                    if skip {
                        skip = false;
                        continue;
                    }

                    let c = data[i];
                    let batch = match data.get(i + 1) {
                        None => vec![c],
                        Some(&n) => {
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

                    lines.push(line);
                }

                lines.join("\n")
            },
            Err(e) => e.to_string()
        };

        if let State::Read(params) = &mut self.state {
            params.main_data = main_data;
            params.pinned_data = pinned_data;
        }
    }

    pub fn toggle_type(&mut self) {
        match &mut self.state {
            State::Read(p) => {
                p.main_data = no_data_text();
                p.register_type.toggle()
            },
            State::Dump(p) => {
                if !p.started {
                    p.register_type.toggle()
                }
            },
            _ => {}
        }
    }
}
