use crate::config::{Column, Config, Label, Labels};
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    no_data_rows, ConnectionStatus, DumpParams, LabelParams, ReadPanel, ReadParams, SaveParams,
    SearchParams, State, StateTransition, WriteParams,
};
use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use std::{error, fs};
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
}

#[derive(Debug)]
struct RefreshTaskResult {
    window_start: u16,
    register_type: RegisterType,
    main_data: Result<Vec<RegisterCellValue>, String>,
    pinned_data: Result<Vec<RegisterCellValue>, String>,
    read_duration: Duration,
}

/// The raw data of the last successful read, kept so the visible rows can be
/// re-formatted instantly when the interpretation columns change at runtime.
#[derive(Debug)]
struct LastRead {
    window_start: u16,
    main_data: Vec<RegisterCellValue>,
    pinned_data: Vec<RegisterCellValue>,
    read_at: DateTime<Local>,
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
    /// Number of register rows the Read table can show; written by the draw layer
    /// each frame and used to size the read window to exactly the visible area.
    pub visible_rows: Cell<u16>,
    background_task: Option<BackgroundTask>,
    previous_values: BTreeMap<RegisterCell, u16>,
    read_log: BTreeMap<RegisterCell, (u16, DateTime<Utc>)>,
    last_read: Option<LastRead>,
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
        let initial_rows = config.registers_batch.max(1);

        Self {
            interpreter: Interpretor::new(config.interpretations.clone(), config.device.word_order),
            pinned_registers: config.pinned_registers.clone().into(),
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
            visible_rows: Cell::new(initial_rows),
            background_task: None,
            previous_values: BTreeMap::new(),
            read_log: BTreeMap::new(),
            last_read: None,
        }
    }

    pub fn get_current_position(&self) -> u16 {
        match &self.state {
            State::Read(p) => p.position,
            State::Write(p) => p.position,
            State::Label(p) => p.position,
            State::Help | State::Save(_) | State::Search(_) | State::Dump(_) => 0,
        }
    }

    pub fn switch_focus_to(&mut self, focus: StateTransition) {
        let position = self.get_current_position();
        let register_type = match &self.state {
            State::Read(p) => p.register_type,
            _ => Default::default(),
        };

        self.state = match focus {
            StateTransition::Write => State::Write(WriteParams {
                position,
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
                let (label_type, label_pos) = match &self.state {
                    State::Read(p) if p.panel == ReadPanel::Pinned => self
                        .pinned_registers
                        .get(p.pinned_index as usize)
                        .map(|&(kind, address)| (kind, address))
                        .unwrap_or((register_type, position)),
                    _ => (register_type, position),
                };
                let text = self
                    .labels
                    .get(&(label_type, label_pos))
                    .cloned()
                    .unwrap_or_default();
                State::Label(LabelParams {
                    position: label_pos,
                    register_type: label_type,
                    text,
                    result: None,
                })
            }
            StateTransition::Save => State::Save(SaveParams::default()),
            StateTransition::Dump => State::Dump(DumpParams::default()),
            StateTransition::Search => {
                let matches = self
                    .labels
                    .iter()
                    .map(|(&cell, text)| (cell, text.clone()))
                    .collect();
                State::Search(SearchParams {
                    matches,
                    ..Default::default()
                })
            }
        };
    }

    pub fn search_input(&mut self, c: char) {
        if let State::Search(p) = &mut self.state {
            p.query.push(c);
        }
        self.recompute_search();
    }

    pub fn search_backspace(&mut self) {
        if let State::Search(p) = &mut self.state {
            p.query.pop();
        }
        self.recompute_search();
    }

    pub fn search_move(&mut self, down: bool) {
        let rows = self.visible_rows.get();
        if let State::Search(p) = &mut self.state {
            p.selected = if down {
                p.selected.saturating_add(1)
            } else {
                p.selected.saturating_sub(1)
            };
            p.scroll(rows);
        }
    }

    pub fn search_commit(&mut self) {
        let target = if let State::Search(p) = &self.state {
            p.matches.get(p.selected as usize).map(|(cell, _)| *cell)
        } else {
            return;
        };
        if let Some((register_type, position)) = target {
            self.state = State::Read(ReadParams {
                position,
                window_start: position,
                register_type,
                ..Default::default()
            });
        }
    }

    fn recompute_search(&mut self) {
        let query = if let State::Search(p) = &self.state {
            p.query.to_lowercase()
        } else {
            return;
        };
        let matches: Vec<_> = self
            .labels
            .iter()
            .filter(|(_, text)| query.is_empty() || text.to_lowercase().contains(&query))
            .map(|(&cell, text)| (cell, text.clone()))
            .collect();
        let rows = self.visible_rows.get();
        if let State::Search(p) = &mut self.state {
            p.matches = matches;
            p.selected = 0;
            p.top = 0;
            p.scroll(rows);
        }
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
        if text.is_empty() {
            self.labels.remove(&key);
        } else {
            self.labels.insert(key, text);
        }

        self.state = State::Read(ReadParams {
            position,
            window_start: position,
            register_type,
            ..Default::default()
        });
    }

    fn persist_config(&mut self) -> String {
        self.config.labels = (&self.labels).into();

        let mut pinned = PinnedRegisters::default();
        for (kind, address) in &self.pinned_registers {
            match kind {
                RegisterType::Holding => pinned.holdings.push(*address),
                RegisterType::Input => pinned.inputs.push(*address),
            }
        }
        self.config.pinned_registers = pinned;

        match save_config(&self.config) {
            Ok(()) => format!("Saved to {CONFIG_PATH}"),
            Err(e) => format!("Save failed: {e}"),
        }
    }

    pub fn read_count(&self) -> usize {
        self.read_log.len()
    }

    fn dump_read_log(&self) -> String {
        if self.read_log.is_empty() {
            return "Nothing read yet to dump.".to_string();
        }

        let filename = format!("dump_{}.txt", Local::now().format("%Y%m%d_%H%M%S"));

        let mut out = String::from("read_at\ttype\taddress\thex\tdecimal\tlabel\n");
        for (&(kind, address), &(value, read_at)) in &self.read_log {
            let label = self.labels.get(&(kind, address)).cloned().unwrap_or_default();
            out.push_str(&format!(
                "{}\t{kind:?}\t{address}\t{value:04X}\t{value}\t{label}\n",
                read_at.to_rfc3339_opts(SecondsFormat::Millis, true),
            ));
        }

        match fs::write(&filename, out) {
            Ok(()) => format!("Dumped {} registers to {filename}", self.read_log.len()),
            Err(e) => format!("Dump failed: {e}"),
        }
    }

    pub fn pin(&mut self) {
        let (panel, register_type, position, pinned_index) = if let State::Read(p) = &self.state {
            (p.panel, p.register_type, p.position, p.pinned_index)
        } else {
            return;
        };

        let selection = match panel {
            ReadPanel::Main => (register_type, position),
            ReadPanel::Pinned => match self.pinned_registers.get(pinned_index as usize) {
                Some(&cell) => cell,
                None => return,
            },
        };

        if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
        } else {
            self.pinned_registers.push(selection);
        }

        self.pinned_registers.sort();

        let rows = self.visible_rows.get();
        let len = self.pinned_registers.len() as u16;
        if let State::Read(p) = &mut self.state {
            p.scroll_pinned(rows, len);
        }
    }

    pub async fn do_action(&mut self) {
        let mut read_now = false;
        let mut save_now = false;
        let mut dump_now = false;

        match &mut self.state {
            State::Read(_) => read_now = true,
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
            State::Label(_) => {}
            State::Save(_) => save_now = true,
            State::Search(_) => {}
            State::Dump(_) => dump_now = true,
        }

        if read_now {
            self.refresh().await;
        }
        if save_now {
            let result = self.persist_config();
            if let State::Save(p) = &mut self.state {
                p.result = Some(result);
            }
        }
        if dump_now {
            let result = self.dump_read_log();
            if let State::Dump(p) = &mut self.state {
                p.result = Some(result);
            }
        }
    }

    pub async fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        let should_refresh = matches!(
            &self.state,
            State::Read(p)
                if self.config.auto_update_interval_seconds
                    .is_some_and(|seconds| p.refresh_timer.elapsed().as_secs() > seconds)
        );

        if should_refresh {
            self.refresh().await;
        }
    }

    pub fn quit(&mut self) {
        match &self.state {
            State::Read(_) => self.running = false,
            State::Write(params) => self.state = State::Read(ReadParams {
                window_start: params.position,
                data_start: params.position,
                position: params.position,
                ..Default::default()
            }),
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

        let (window_start, register_type) = if let State::Read(p) = &mut self.state {
            p.refresh_timer = Instant::now();
            p.loading = true;
            (p.window_start, p.register_type)
        } else {
            return;
        };
        self.connection = ConnectionStatus::Reading;

        let device = self.device.clone();
        let pinned_registers = self.pinned_registers.clone();
        let amount = self.visible_rows.get().max(1);

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
        if !matches!(
            &self.state,
            State::Read(params) if params.register_type == result.register_type
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
        let read_at = Utc::now();
        let read_at_local = read_at.with_timezone(&Local);
        for data in [&result.main_data, &result.pinned_data].into_iter().flatten() {
            for &((kind, address), value) in data {
                new_previous.insert((kind, address), value);
                self.read_log.insert((kind, address), (value, read_at));
            }
        }

        self.previous_values = new_previous;

        let ascii_string = match &result.main_data {
            Ok(data) => self.interpreter.ascii_string(data),
            Err(_) => String::new(),
        };

        let pinned_ascii_string = match &result.pinned_data {
            Ok(data) => self.interpreter.ascii_string(data),
            Err(_) => String::new(),
        };

        // Cache the raw read so the rows can be re-formatted instantly when the
        // visible columns change (see `rebuild_read_rows`).
        let connection = match &result.main_data {
            Ok(main_data) => {
                self.last_read = Some(LastRead {
                    window_start: result.window_start,
                    main_data: main_data.clone(),
                    pinned_data: result.pinned_data.clone().unwrap_or_default(),
                    read_at: read_at_local,
                });
                ConnectionStatus::Connected
            }
            Err(e) => ConnectionStatus::Error(e.clone()),
        };

        if let State::Read(params) = &mut self.state {
            params.ascii_string = ascii_string;
            params.pinned_ascii_string = pinned_ascii_string;
            params.main_changed = main_changed;
            params.pinned_changed = pinned_changed;
            params.read_duration = Some(result.read_duration);
            params.loading = false;
            if let Err(e) = &result.main_data {
                params.main_rows = vec![e.clone()];
                params.pinned_rows = Vec::new();
                params.data_start = result.window_start;
            }
        }

        if result.main_data.is_ok() {
            self.rebuild_read_rows();
        }
        self.connection = connection;
    }

    /// Re-format the Read rows from the cached last read using the current
    /// interpreter columns. Cheap, and used both after a read and after a
    /// runtime column toggle so the table updates without re-reading the device.
    fn rebuild_read_rows(&mut self) {
        let (window_start, main_data, pinned_data, read_at) = match &self.last_read {
            Some(lr) => (
                lr.window_start,
                lr.main_data.clone(),
                lr.pinned_data.clone(),
                lr.read_at,
            ),
            None => return,
        };

        let labels = &self.labels;
        let main_rows = self.interpreter.run(main_data, window_start, read_at, |cell| {
            let ((kind, address), _) = cell;
            labels.get(&(kind, address)).cloned()
        });
        let pinned_rows = Self::format_pinned(&self.interpreter, &pinned_data, read_at, labels);

        if let State::Read(params) = &mut self.state {
            params.main_rows = main_rows;
            params.pinned_rows = pinned_rows;
            params.data_start = window_start;
        }
    }

    /// Format the (scattered) pinned register list, grouping consecutive cells so
    /// multi-word interpretations read across pin boundaries.
    fn format_pinned(
        interpreter: &Interpretor,
        data: &[RegisterCellValue],
        read_at: DateTime<Local>,
        labels: &BTreeMap<RegisterCell, String>,
    ) -> Vec<String> {
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

            rows.extend(interpreter.run(batch, address, read_at, |cell| {
                let ((kind, c_address), _) = cell;
                labels.get(&(kind, c_address)).cloned()
            }));
        }

        rows
    }

    /// Toggle an interpretation column at runtime and re-render the cached rows.
    pub fn toggle_column(&mut self, column: Column) {
        self.interpreter.toggle(column);
        self.rebuild_read_rows();
    }

    pub fn label_text(&self, register_type: RegisterType, address: u16) -> Option<String> {
        self.labels.get(&(register_type, address)).cloned()
    }

    pub fn commit_jump(&mut self) {
        let rows = self.visible_rows.get();
        if let State::Read(p) = &mut self.state {
            if let Some(target) = p.jump.take() {
                p.position = target;
                p.scroll_to_cursor(rows);
            }
        }
    }

    pub async fn complete_background_task(&mut self) {
        let Some(task) = self.background_task.as_ref() else {
            return;
        };

        if !match task {
            BackgroundTask::Refresh(handle) => handle.is_finished(),
            BackgroundTask::Write(handle) => handle.is_finished(),
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
        }
    }

    pub fn toggle_type(&mut self) {
        if let State::Read(p) = &mut self.state {
            p.main_rows = no_data_rows();
            p.read_duration = None;
            p.ascii_string = String::new();
            p.main_changed = Vec::new();
            p.register_type.toggle();
        }
    }
}
