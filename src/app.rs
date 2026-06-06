use crate::config::{Config, Label, Labels};
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    no_data_rows, ConnectionStatus, DumpParams, JumpParams, LabelParams, ReadParams, State,
    StateTransition, WriteParams,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    window_start: u16,
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
    /// Truthful device reachability, derived from the latest read result.
    pub connection: ConnectionStatus,
    /// Monotonic tick counter used to advance the loading spinner.
    pub frame: u64,
    background_task: Option<BackgroundTask>,
    previous_values: BTreeMap<RegisterCell, u16>,
    labels: BTreeMap<RegisterCell, String>,
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

impl From<Labels> for BTreeMap<RegisterCell, String> {
    fn from(value: Labels) -> Self {
        let mut map = BTreeMap::new();

        for label in value.holdings {
            map.insert((RegisterType::Holding, label.address), label.text);
        }

        for label in value.inputs {
            map.insert((RegisterType::Input, label.address), label.text);
        }

        map
    }
}

impl From<&BTreeMap<RegisterCell, String>> for Labels {
    fn from(map: &BTreeMap<RegisterCell, String>) -> Self {
        let mut holdings = Vec::new();
        let mut inputs = Vec::new();

        for ((kind, address), text) in map {
            let label = Label {
                address: *address,
                text: text.clone(),
            };
            match kind {
                RegisterType::Holding => holdings.push(label),
                RegisterType::Input => inputs.push(label),
            }
        }

        Self { holdings, inputs }
    }
}

fn save_config(config: &Config) -> Result<(), String> {
    let content = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(CONFIG_PATH, content).map_err(|e| e.to_string())
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
            labels: config.labels.clone().into(),
            state: State::Read(ReadParams {
                position: config.startup.address,
                window_start: config.startup.address,
                register_type: config.startup.register_type,
                ..Default::default()
            }),
            config,
            device,
            running: true,
            connection: ConnectionStatus::Unknown,
            frame: 0,
            background_task: None,
            previous_values: BTreeMap::new(),
        }
    }

    pub fn get_current_position(&self) -> u16 {
        match &self.state {
            State::Read(p) => p.position,
            State::Jump(p) => p.to,
            State::Write(p) => p.position,
            State::Dump(p) => p.position,
            State::Label(p) => p.position,
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
                window_start: position,
                register_type,
                ..Default::default()
            }),
            StateTransition::Help => State::Help,
            StateTransition::Label => {
                let text = self
                    .labels
                    .get(&(register_type, position))
                    .cloned()
                    .unwrap_or_default();
                State::Label(LabelParams {
                    position,
                    register_type,
                    text,
                    result: None,
                })
            }
        };
    }

    pub fn label_input(&mut self, c: char) {
        if let State::Label(p) = &mut self.state {
            p.result = None;
            p.text.push(c);
        }
    }

    pub fn label_backspace(&mut self) {
        if let State::Label(p) = &mut self.state {
            p.result = None;
            p.text.pop();
        }
    }

    pub fn cancel_label(&mut self) {
        let State::Label(p) = &self.state else { return };
        let position = p.position;
        let register_type = p.register_type;
        self.state = State::Read(ReadParams {
            position,
            window_start: position,
            register_type,
            ..Default::default()
        });
    }

    pub fn commit_label(&mut self) {
        let (position, register_type, text) = {
            let State::Label(p) = &self.state else { return };
            (p.position, p.register_type, p.text.clone())
        };

        let key = (register_type, position);
        let mut new_labels = self.labels.clone();
        if text.is_empty() {
            new_labels.remove(&key);
        } else {
            new_labels.insert(key, text);
        }

        let mut new_config = self.config.clone();
        new_config.labels = (&new_labels).into();

        if let Err(e) = save_config(&new_config) {
            if let State::Label(p) = &mut self.state {
                p.result = Some(format!("Save failed: {e}"));
            }
            return;
        }

        self.labels = new_labels;
        self.config = new_config;
        self.state = State::Read(ReadParams {
            position,
            window_start: position,
            register_type,
            ..Default::default()
        });
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
        let mut read_now = false;

        match &mut self.state {
            State::Read(_) => read_now = true,
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
            State::Label(_) => {}
        }

        if start_dump_run {
            self.start_dump_batch();
        }
        if read_now {
            // Enter commits a new read point at the cursor.
            self.refresh(true).await;
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
        self.frame = self.frame.wrapping_add(1);
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        // Auto-refresh re-reads the last committed window in place (no cursor
        // move); Enter is what moves the read point. The tick also drives dumps.
        let should_refresh = matches!(
            &self.state,
            State::Read(p)
                if self.config.auto_update_interval_seconds
                    .is_some_and(|seconds| p.refresh_timer.elapsed().as_secs() > seconds)
        );

        if should_refresh {
            self.refresh(false).await;
        } else if matches!(&self.state, State::Dump(p) if p.started) {
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

    pub async fn refresh(&mut self, anchor_to_cursor: bool) {
        if self.background_task.is_some() {
            return;
        }

        let (window_start, register_type) = if let State::Read(p) = &mut self.state {
            p.refresh_timer = Instant::now();
            p.loading = true;
            // `anchor_to_cursor` (Enter) moves the read point to the cursor; a
            // plain refresh (timer / refresh key) re-reads the existing window so
            // the cursor and view stay put and only the values update.
            if anchor_to_cursor {
                p.window_start = p.position;
            }
            (p.window_start, p.register_type)
        } else {
            return;
        };
        self.connection = ConnectionStatus::Reading;

        let device = self.device.clone();
        let pinned_registers = self.pinned_registers.clone();
        let amount = self.config.registers_batch;

        self.background_task = Some(BackgroundTask::Refresh(tokio::spawn(async move {
            let read_start = Instant::now();
            let main_data = Self::aquire_data_with(&device, amount, window_start, register_type)
                .await
                .map_err(|e| e.to_string());
            let read_duration = read_start.elapsed();
            let pinned_data = Self::aquire_pinned_data_with(&device, &pinned_registers)
                .await
                .map_err(|e| e.to_string());

            RefreshTaskResult {
                window_start,
                register_type,
                main_data,
                pinned_data,
                read_duration,
            }
        })));
    }

    fn apply_refresh_result(&mut self, result: RefreshTaskResult) {
        // Match on the fetched window + register type, not the cursor: the data
        // belongs to `window_start` regardless of where the cursor has since moved.
        if !matches!(
            &self.state,
            State::Read(params)
                if params.window_start == result.window_start
                    && params.register_type == result.register_type
        ) {
            return;
        }

        let changed_flags = |cells: &Result<Vec<RegisterCellValue>, String>| match cells {
            Ok(data) => data
                .iter()
                .map(|&((kind, address), value)| {
                    matches!(self.previous_values.get(&(kind, address)), Some(&prev) if prev != value)
                })
                .collect::<Vec<bool>>(),
            Err(_) => Vec::new(),
        };
        let main_changed = changed_flags(&result.main_data);
        let pinned_changed = changed_flags(&result.pinned_data);

        let mut new_previous = BTreeMap::new();
        for data in [&result.main_data, &result.pinned_data].into_iter().flatten() {
            for &((kind, address), value) in data {
                new_previous.insert((kind, address), value);
            }
        }

        let labels = &self.labels;
        let labeler = |cell: RegisterCellValue| {
            let ((c_kind, c_address), _) = cell;
            labels.get(&(c_kind, c_address)).cloned()
        };

        let ascii_string = match &result.main_data {
            Ok(data) => self.interpreter.ascii_string(data),
            Err(_) => String::new(),
        };

        let pinned_ascii_string = match &result.pinned_data {
            Ok(data) => self.interpreter.ascii_string(data),
            Err(_) => String::new(),
        };

        let (main_rows, connection) = match result.main_data {
            Ok(data) => (
                self.interpreter.run(data, result.window_start, labeler),
                ConnectionStatus::Connected,
            ),
            Err(e) => (vec![e.clone()], ConnectionStatus::Error(e)),
        };

        let pinned_rows = match result.pinned_data {
            Ok(data) => {
                let mut rows = Vec::new();
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

                    rows.extend(self.interpreter.run(batch, address, |c| {
                        let ((kind, c_address), _) = c;
                        labels.get(&(kind, c_address)).cloned()
                    }));
                }

                rows
            }
            Err(e) => vec![e.to_string()],
        };

        self.previous_values = new_previous;

        if let State::Read(params) = &mut self.state {
            params.main_rows = main_rows;
            params.pinned_rows = pinned_rows;
            params.ascii_string = ascii_string;
            params.pinned_ascii_string = pinned_ascii_string;
            params.main_changed = main_changed;
            params.pinned_changed = pinned_changed;
            params.window_start = result.window_start;
            params.read_duration = Some(result.read_duration);
            params.loading = false;
        }
        self.connection = connection;
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
                    let message = e.to_string();
                    if let State::Read(params) = &mut self.state {
                        params.main_rows = vec![message.clone()];
                        params.main_changed = Vec::new();
                        params.loading = false;
                    }
                    self.connection = ConnectionStatus::Error(message);
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
                p.main_rows = no_data_rows();
                p.read_duration = None;
                p.ascii_string = String::new();
                p.main_changed = Vec::new();
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
