use crate::config::Config;
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    no_data_text, DumpParams, JumpParams, ReadParams, State, StateTransition, WriteParams,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use std::{error, fs};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum WriteType {
    #[default]
    Word,
    DWord,
}

pub type AppResult<T> = Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
enum BackgroundTask {
    Refresh(JoinHandle<RefreshTaskResult>),
    Write(JoinHandle<String>),
    DumpBatch(JoinHandle<DumpBatchTaskResult>),
}

#[derive(Debug)]
struct RefreshTaskResult {
    position: u16,
    register_type: RegisterType,
    main_data: Result<Vec<RegisterCellValue>, String>,
    pinned_data: Result<Vec<RegisterCellValue>, String>,
    read_duration: Duration,
}

#[derive(Debug)]
struct DumpBatchTaskResult {
    starting_index: u16,
    total_batches: u16,
    result: Result<Option<String>, String>,
}

#[derive(Debug)]
pub struct App {
    pub config: Config,
    pub running: bool,
    pub state: State,
    pub pinned_registers: Vec<RegisterCell>,
    pub device: ModbusDevice,
    pub interpreter: Interpretor,
    background_task: Option<BackgroundTask>,
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
    let content = fs::read_to_string(CONFIG_PATH)
        .inspect_err(|_| dump_example_config_and_exit())
        .unwrap();
    serde_json::from_str(&content)
        .inspect_err(|e| println!("Could not parse config: {e}"))
        .unwrap()
}

impl App {
    pub async fn new() -> Self {
        let config = fetch_config_or_exit();
        let device = ModbusDevice::new(&config.device)
            .await
            .inspect_err(|e| println!("Could not initialize device: {e}"))
            .unwrap();

        Self {
            interpreter: Interpretor::new(config.interpretations.clone(), config.device.word_order),
            pinned_registers: config.pinned_defaults.clone().into(),
            state: State::Read(ReadParams {
                position: config.startup.address,
                register_type: config.startup.register_type,
                ..Default::default()
            }),
            config,
            device,
            running: true,
            background_task: None,
        }
    }

    pub fn get_current_position(&self) -> u16 {
        match &self.state {
            State::Read(p) => p.position,
            State::Jump(p) => p.to,
            State::Write(p) => p.position,
            State::Dump(p) => p.position,
            State::Help => 0,
        }
    }

    pub fn switch_focus_to(&mut self, focus: StateTransition) {
        let position = self.get_current_position();
        let register_type = match &self.state {
            State::Read(p) => p.register_type,
            State::Dump(p) => p.register_type,
            State::Jump(p) => p.register_type,
            _ => Default::default(),
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
                register_type,
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
            State::Write(params) => {
                if let Some(number) = params.value {
                    if self.background_task.is_some() {
                        params.result = Some("Device is busy.".to_string());
                    } else {
                        let device = self.device.clone();
                        let position = params.position;
                        let write_type = params.write_type;

                        params.result = Some("Writing...".to_string());
                        self.background_task = Some(BackgroundTask::Write(tokio::spawn(async move {
                            let result = match write_type {
                                WriteType::Word => device.write_register(position, number as u16).await,
                                WriteType::DWord => device.write_register_word(position, number).await,
                            };

                            format!("{result:#?}")
                        })));
                    }
                }
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
            }
        }

        if start_dump_run {
            self.start_dump_batch();
        }
    }

    fn start_dump_batch(&mut self) {
        if self.background_task.is_some() {
            return;
        }

        let (header_needed, starting_index, total_batches, dump_file, register_type) = {
            let State::Dump(params) = &mut self.state else {
                return;
            };

            let Some(total_batches) = params.total_batches else {
                return;
            };

            if params.completed_batches >= total_batches {
                params.started = false;
                return;
            }

            (
                !params.header_written,
                params.position,
                total_batches,
                self.config.dump_file.clone(),
                params.register_type,
            )
        };

        let device = self.device.clone();
        let interpreter = self.interpreter.clone();
        let amount = self.config.registers_batch;

        self.background_task = Some(BackgroundTask::DumpBatch(tokio::spawn(async move {
            let result = async {
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&dump_file)
                    .await
                    .map_err(|e| e.to_string())?;

                if header_needed {
                    file.write_all(interpreter.header().as_bytes())
                        .await
                        .map_err(|e| e.to_string())?;
                    file.write_all(b"\n").await.map_err(|e| e.to_string())?;
                }

                let data_result =
                    Self::aquire_data_with(&device, amount, starting_index, register_type).await;
                let mut error_message = None;

                match data_result {
                    Ok(data) => {
                        file.write_all(
                            interpreter
                                .run(data, starting_index, |_| None)
                                .join("\n")
                                .as_bytes(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                        file.write_all(b"\n").await.map_err(|e| e.to_string())?;
                    }
                    Err(e) => {
                        error_message = Some(e.to_string());
                        let line = format!("{starting_index}: error");
                        file.write_all(line.as_bytes())
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                }

                Ok(error_message)
            }
            .await;

            DumpBatchTaskResult {
                starting_index,
                total_batches,
                result,
            }
        })));
    }

    pub async fn tick(&mut self) {
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        let should_perform_dump = matches!(&self.state, State::Dump(p) if p.started);
        let should_refresh = matches!(
            &self.state,
            State::Read(p)
                if self.config.auto_update_interval_seconds
                    .is_some_and(|seconds| p.refresh_timer.elapsed().as_secs() > seconds)
        );

        if should_refresh {
            self.refresh().await;
        } else if should_perform_dump {
            self.start_dump_batch();
        }
    }

    pub fn quit(&mut self) {
        match self.state {
            State::Read(_) => self.running = false,
            _ => self.state = State::Read(Default::default()),
        }
    }

    pub async fn aquire_data(
        &self,
        register_type: RegisterType,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        Self::aquire_data_with(
            &self.device,
            self.config.registers_batch,
            self.get_current_position(),
            register_type,
        )
        .await
    }

    async fn aquire_data_with(
        device: &ModbusDevice,
        amount: u16,
        position: u16,
        register_type: RegisterType,
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let values = if register_type == RegisterType::Holding {
            device.holdings(position, amount).await?
        } else {
            device.inputs(position, amount).await?
        };

        Ok(values
            .into_iter()
            .enumerate()
            .map(|(i, v)| ((register_type, position + i as u16), v))
            .collect())
    }

    pub async fn aquire_pinned_data(&self) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        Self::aquire_pinned_data_with(&self.device, &self.pinned_registers).await
    }

    async fn aquire_pinned_data_with(
        device: &ModbusDevice,
        regs: &[RegisterCell],
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
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
                RegisterType::Holding => device.holdings(start_addr, run_len as u16).await?,
                RegisterType::Input => device.inputs(start_addr, run_len as u16).await?,
            };

            for j in 0..run_len {
                let cell = regs[i + j].clone();
                let value = values.get(j).cloned().unwrap();

                collection.push((cell, value));
            }

            i += run_len;
        }

        Ok(collection)
    }

    pub async fn refresh(&mut self) {
        if self.background_task.is_some() {
            return;
        }

        let (position, register_type) = if let State::Read(p) = &mut self.state {
            p.refresh_timer = Instant::now();
            p.loading = true;
            (p.position, p.register_type)
        } else {
            return;
        };

        let device = self.device.clone();
        let pinned_registers = self.pinned_registers.clone();
        let amount = self.config.registers_batch;

        self.background_task = Some(BackgroundTask::Refresh(tokio::spawn(async move {
            let read_start = Instant::now();
            let main_data = Self::aquire_data_with(&device, amount, position, register_type)
                .await
                .map_err(|e| e.to_string());
            let read_duration = read_start.elapsed();
            let pinned_data = Self::aquire_pinned_data_with(&device, &pinned_registers)
                .await
                .map_err(|e| e.to_string());

            RefreshTaskResult {
                position,
                register_type,
                main_data,
                pinned_data,
                read_duration,
            }
        })));
    }

    fn apply_refresh_result(&mut self, result: RefreshTaskResult) {
        let position = result.position;
        if !matches!(
            &self.state,
            State::Read(params)
                if params.position == position && params.register_type == result.register_type
        ) {
            return;
        }

        let sfr = &self.pinned_registers;
        let fav_checker = |cell: RegisterCellValue| {
            let ((c_kind, c_address), _) = cell;
            let is_pinned = sfr
                .iter()
                .any(|(kind, address)| *kind == c_kind && *address == c_address);
            if is_pinned {
                Some("Pinned".to_string())
            } else {
                None
            }
        };

        let main_data = match result.main_data {
            Ok(data) => self.interpreter.run(data, position, fav_checker).join("\n"),
            Err(e) => e.to_string(),
        };

        let pinned_data = match result.pinned_data {
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

                    let line = self
                        .interpreter
                        .run(batch, address, |c| {
                            let ((kind, _), _) = c;
                            Some(
                                match kind {
                                    RegisterType::Holding => "Holding",
                                    RegisterType::Input => "Input",
                                }
                                .to_string(),
                            )
                        })
                        .join("\n");

                    lines.push(line);
                }

                lines.join("\n")
            }
            Err(e) => e.to_string(),
        };

        if let State::Read(params) = &mut self.state {
            params.main_data = main_data;
            params.pinned_data = pinned_data;
            params.read_duration = Some(result.read_duration);
            params.loading = false;
        }
    }

    pub async fn complete_background_task(&mut self) {
        let Some(task) = self.background_task.as_ref() else {
            return;
        };

        if !match task {
            BackgroundTask::Refresh(handle) => handle.is_finished(),
            BackgroundTask::Write(handle) => handle.is_finished(),
            BackgroundTask::DumpBatch(handle) => handle.is_finished(),
        } {
            return;
        }

        let task = self.background_task.take().unwrap();

        match task {
            BackgroundTask::Refresh(handle) => match handle.await {
                Ok(result) => self.apply_refresh_result(result),
                Err(e) => {
                    if let State::Read(params) = &mut self.state {
                        params.main_data = e.to_string();
                        params.loading = false;
                    }
                }
            },
            BackgroundTask::Write(handle) => {
                let result = handle.await.unwrap_or_else(|e| e.to_string());
                if let State::Write(params) = &mut self.state {
                    params.result = Some(result);
                }
            }
            BackgroundTask::DumpBatch(handle) => match handle.await {
                Ok(result) => self.apply_dump_batch_result(result),
                Err(e) => {
                    if let State::Dump(params) = &mut self.state {
                        params.started = false;
                        params.error = Some(e.to_string());
                    }
                }
            },
        }
    }

    fn apply_dump_batch_result(&mut self, result: DumpBatchTaskResult) {
        if let State::Dump(params) = &mut self.state {
            match result.result {
                Ok(error_message) => {
                    params.header_written = true;
                    params.completed_batches += 1;
                    params.position = result.starting_index + self.config.registers_batch;
                    params.error = error_message;

                    if params.completed_batches >= result.total_batches {
                        params.started = false;
                    }
                }
                Err(error) => {
                    params.started = false;
                    params.error = Some(error);
                }
            }
        }
    }

    pub fn toggle_type(&mut self) {
        match &mut self.state {
            State::Read(p) => {
                p.main_data = no_data_text();
                p.read_duration = None;
                p.register_type.toggle()
            }
            State::Dump(p) => {
                if !p.started {
                    p.register_type.toggle()
                }
            }
            _ => {}
        }
    }
}
