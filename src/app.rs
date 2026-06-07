use crate::config::{Column, Config, Label, Labels, Startup};
use crate::constants::CONFIG_PATH;
use crate::interpretator::Interpretor;
use crate::modbus::ModbusDevice;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{
    ConnectionStatus, DumpParams, LabelParams, Popup, PopupKind, ReadPanel,
    ReadParams, SaveParams, SearchParams, State, WriteParams,
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
    /// `None` for a panel that wasn't read this cycle (only the active panel is).
    main_data: Option<Result<Vec<RegisterCellValue>, String>>,
    pinned_data: Option<Result<Vec<RegisterCellValue>, String>>,
    read_duration: Duration,
}

#[derive(Debug)]
struct LastRead {
    pinned_data: Vec<RegisterCellValue>,
    pinned_read_at: DateTime<Local>,
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
    /// When true, the auto-refresh timer is suspended so values hold still for
    /// inspection. Manual reads (Enter / refresh key) still work.
    pub paused: bool,
    /// Labels/pins changed since the last save (drives the quit guard).
    pub dirty: bool,
    /// Number of register rows the Read table can show; written by the draw layer
    /// each frame and used to size the read window to exactly the visible area.
    pub visible_rows: Cell<u16>,
    background_task: Option<BackgroundTask>,
    previous_values: BTreeMap<RegisterCell, u16>,
    /// Per-cell "changed on its last read" flag, persisted alongside `read_log`
    /// so the highlight survives jumps the same way the values do.
    changed: BTreeMap<RegisterCell, bool>,
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
            paused: false,
            dirty: false,
            visible_rows: Cell::new(initial_rows),
            background_task: None,
            previous_values: BTreeMap::new(),
            changed: BTreeMap::new(),
            read_log: BTreeMap::new(),
            last_read: None,
        }
    }

    pub fn read(&self) -> &ReadParams {
        match &self.state {
            State::Read(p) => p,
        }
    }

    pub fn read_mut(&mut self) -> &mut ReadParams {
        match &mut self.state {
            State::Read(p) => p,
        }
    }

    /// Which popup (if any) is currently open.
    pub fn popup_kind(&self) -> Option<PopupKind> {
        self.read().popup.as_ref().map(Popup::kind)
    }

    pub fn close_popup(&mut self) {
        self.read_mut().popup = None;
    }

    pub fn open_help(&mut self) {
        self.read_mut().popup = Some(Popup::Help);
    }

    pub fn open_save(&mut self) {
        self.read_mut().popup = Some(Popup::Save(SaveParams::default()));
    }

    pub fn open_dump(&mut self) {
        self.read_mut().popup = Some(Popup::Dump(DumpParams::default()));
    }

    pub fn open_columns(&mut self) {
        self.read_mut().popup = Some(Popup::Columns(0));
    }

    pub fn open_write(&mut self) {
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        // On the Pinned panel, write the selected pin rather than the Main cursor.
        let (write_type, write_pos) = if panel == ReadPanel::Pinned {
            self.pinned_registers
                .get(pinned_index as usize)
                .map(|&(kind, address)| (kind, address))
                .unwrap_or((register_type, position))
        } else {
            (register_type, position)
        };

        // Input registers are read-only in Modbus; there's nothing to write.
        if write_type == RegisterType::Input {
            return;
        }

        let value = self
            .previous_values
            .get(&(write_type, write_pos))
            .map(|&v| v as i64);

        self.read_mut().popup = Some(Popup::Write(WriteParams {
            position: write_pos,
            value,
            ..Default::default()
        }));
    }

    pub fn open_slave(&mut self) {
        let current = self.config.device.slave_id as u16;
        self.read_mut().popup = Some(Popup::Slave(current));
    }

    /// Commit the slave-ID popup: retarget the live connection (no reconnect).
    pub async fn commit_slave(&mut self) {
        let id = match &self.read().popup {
            Some(Popup::Slave(value)) => Some((*value).min(u8::MAX as u16) as u8),
            _ => None,
        };
        if let Some(id) = id {
            self.device.set_slave(id).await;
            self.config.device.slave_id = id;
            self.read_mut().popup = None;
            self.refresh().await;
        }
    }

    /// Cycle the word order (ABCD → BADC → CDAB → DCBA) and re-render in place.
    pub fn toggle_word_order(&mut self) {
        let next = self.config.device.word_order.next();
        self.config.device.word_order = next;
        self.interpreter.set_word_order(next);
        self.device.set_word_order(next);
        self.rebuild_read_rows();
    }

    /// Quit, but guard against losing unsaved label/pin changes.
    pub fn request_quit(&mut self) {
        if self.dirty {
            self.read_mut().popup = Some(Popup::Quit);
        } else {
            self.running = false;
        }
    }

    pub fn open_search(&mut self) {
        self.read_mut().popup = Some(Popup::Search(SearchParams::default()));
        self.recompute_search();
    }

    pub fn open_label(&mut self) {
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        // On the Pinned panel, label the selected pin rather than the Main cursor.
        let (label_type, label_pos) = if panel == ReadPanel::Pinned {
            self.pinned_registers
                .get(pinned_index as usize)
                .map(|&(kind, address)| (kind, address))
                .unwrap_or((register_type, position))
        } else {
            (register_type, position)
        };
        let text = self
            .labels
            .get(&(label_type, label_pos))
            .cloned()
            .unwrap_or_default();
        self.read_mut().popup = Some(Popup::Label(LabelParams {
            position: label_pos,
            register_type: label_type,
            text,
            result: None,
        }));
    }

    pub fn search_input(&mut self, c: char) {
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.query.push(c);
        }
        self.recompute_search();
    }

    pub fn search_backspace(&mut self) {
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.query.pop();
        }
        self.recompute_search();
    }

    pub fn search_move(&mut self, down: bool) {
        let rows = self.visible_rows.get();
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.selected = if down {
                s.selected.saturating_add(1)
            } else {
                s.selected.saturating_sub(1)
            };
            s.scroll(rows);
        }
    }

    /// Jump the Read view to the selected matching label and close the popup.
    /// Jump to the selected search result. Returns whether a jump happened.
    pub fn search_commit(&mut self) -> bool {
        let target = match &self.read().popup {
            Some(Popup::Search(s)) => s.matches.get(s.selected as usize).map(|(cell, _)| *cell),
            _ => None,
        };
        let Some((register_type, position)) = target else {
            return false;
        };

        let p = self.read_mut();
        let type_changed = register_type != p.register_type;
        p.panel = ReadPanel::Main;
        p.position = position;
        p.register_type = register_type;
        p.window_start = position;
        // Keep the cached rows so the view doesn't blank on a jump; only drop
        // them when the register type changes (they'd belong to the other type).
        // Addresses outside the old window show placeholders until the refresh
        // below lands.
        if type_changed {
            p.main_rows = Vec::new();
            p.main_changed = Vec::new();
            p.data_start = position;
        }
        p.popup = None;
        // Re-render the new window from the read log so previously-read addresses
        // show immediately (no device read needed).
        self.rebuild_read_rows();
        true
    }

    fn recompute_search(&mut self) {
        let read = self.read();
        let query = match &read.popup {
            Some(Popup::Search(s)) => s.query.clone(),
            _ => return,
        };

        let (register_type, has_explicit_type) = match query.chars().next() {
            Some('h') | Some('H') => (RegisterType::Holding, true),
            Some('i') | Some('I') => (RegisterType::Input, true),
            _ => (read.register_type, false),
        };

        let mut matches: Vec<(RegisterCell, String)> = Vec::new();

        let numeric_query = if has_explicit_type {
            query.chars().skip(1).collect()
        } else {
            query.clone()
        };

        // If the query is a valid address, offer to jump straight to it.
        if let Ok(parsed_address) = numeric_query.trim().parse::<u32>() {
            let address = if parsed_address > u16::MAX as u32 {
                u16::MAX
            } else {
                parsed_address as u16
            };

            matches.push(((register_type, address), "jump to this address".to_string()));
        }

        // Then label matches (case-insensitive substring).
        let lower = query.to_lowercase();
        matches.extend(
            self.labels
                .iter()
                .filter(|(_, text)| lower.is_empty() || text.to_lowercase().contains(&lower))
                .map(|(&cell, text)| (cell, text.clone())),
        );

        let rows = self.visible_rows.get();
        if let Some(Popup::Search(s)) = &mut self.read_mut().popup {
            s.matches = matches;
            s.selected = 0;
            s.top = 0;
            s.scroll(rows);
        }
    }

    pub fn label_input(&mut self, c: char) {
        if let Some(Popup::Label(l)) = &mut self.read_mut().popup {
            l.result = None;
            l.text.push(c);
        }
    }

    pub fn label_backspace(&mut self) {
        if let Some(Popup::Label(l)) = &mut self.read_mut().popup {
            l.result = None;
            l.text.pop();
        }
    }

    pub fn commit_label(&mut self) {
        let (position, register_type, text) = match &self.read().popup {
            Some(Popup::Label(l)) => (l.position, l.register_type, l.text.clone()),
            _ => return,
        };

        let key = (register_type, position);
        if text.is_empty() {
            self.labels.remove(&key);
        } else {
            self.labels.insert(key, text);
        }
        self.dirty = true;

        self.read_mut().popup = None;
        // Re-render so the new label shows immediately in the label column.
        self.rebuild_read_rows();
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

        self.config.interpretations = self.interpreter.config();
        self.config.startup = Startup {
            address: self.read().position,
            register_type: self.read().register_type,
        };

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
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
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
        self.dirty = true;

        let rows = self.visible_rows.get();
        let len = self.pinned_registers.len() as u16;
        self.read_mut().scroll_pinned(rows, len);
    }

    /// Commit the Save popup: write config to disk and show the outcome.
    pub fn commit_save(&mut self) {
        let result = self.persist_config();
        if result.starts_with("Saved") {
            self.dirty = false;
        }
        if let Some(Popup::Save(s)) = &mut self.read_mut().popup {
            s.result = Some(result);
        }
    }

    /// Commit the Dump popup: write the read log to a file and show the outcome.
    pub fn commit_dump(&mut self) {
        let result = self.dump_read_log();
        if let Some(Popup::Dump(d)) = &mut self.read_mut().popup {
            d.result = Some(result);
        }
    }

    pub async fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.complete_background_task().await;
        if self.background_task.is_some() {
            return;
        }

        let should_refresh = !self.paused
            && matches!(
                &self.state,
                State::Read(p)
                    if self.config.auto_update_interval_seconds
                        .is_some_and(|seconds| p.refresh_timer.elapsed().as_secs() > seconds)
            );

        if should_refresh {
            self.refresh().await;
        }
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn quit(&mut self) {
        self.running = false;
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

    async fn aquire_pinned_data_with(
        device: &ModbusDevice,
        regs: &[RegisterCell],
    ) -> Result<Vec<RegisterCellValue>, anyhow::Error> {
        let mut collection = Vec::with_capacity(regs.len());

        let mut i = 0usize;
        while i < regs.len() {
            let (kind, start_addr_raw) = regs[i];
            let start_addr = start_addr_raw;

            let mut run_len = 1usize;
            while i + run_len < regs.len() {
                let (next_kind, next_addr_raw) = regs[i + run_len];
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
                let cell = regs[i + j];
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

        let (panel, window_start, register_type) = {
            let p = self.read_mut();
            p.refresh_timer = Instant::now();
            p.loading = true;
            (p.panel, p.window_start, p.register_type)
        };
        self.connection = ConnectionStatus::Reading;

        let device = self.device.clone();
        let pinned_registers = self.pinned_registers.clone();
        let amount = self.visible_rows.get().max(1);

        self.background_task = Some(BackgroundTask::Refresh(tokio::spawn(async move {
            let read_start = Instant::now();
            // Only read the panel the user is actually looking at.
            let (main_data, pinned_data) = match panel {
                ReadPanel::Main => {
                    let main = Self::aquire_data_with(&device, amount, window_start, register_type)
                        .await
                        .map_err(|e| e.to_string());
                    (Some(main), None)
                }
                ReadPanel::Pinned => {
                    let pinned = Self::aquire_pinned_data_with(&device, &pinned_registers)
                        .await
                        .map_err(|e| e.to_string());
                    (None, Some(pinned))
                }
            };
            let read_duration = read_start.elapsed();

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
        // Drop a stale Main read if the register type changed while it was in flight.
        // (A Pinned read carries its own per-cell types, so it isn't affected.)
        if result.main_data.is_some()
            && !matches!(
                &self.state,
                State::Read(params) if params.register_type == result.register_type
            )
        {
            self.read_mut().loading = false;
            return;
        }

        let read_at = Utc::now();
        let read_at_local = read_at.with_timezone(&Local);

        for data in [result.main_data.as_ref(), result.pinned_data.as_ref()]
            .into_iter()
            .flatten() // Option -> &Result
            .flatten() // &Result -> &Vec (Ok only)
        {
            for &(cell, value) in data {
                let did_change = matches!(self.previous_values.get(&cell), Some(&prev) if prev != value);
                self.changed.insert(cell, did_change);
                self.previous_values.insert(cell, value);
                self.read_log.insert(cell, (value, read_at));
            }
        }

        let main_ascii = match &result.main_data {
            Some(Ok(data)) => Some(self.interpreter.ascii_string(data)),
            _ => None,
        };
        let pinned_ascii = match &result.pinned_data {
            Some(Ok(data)) => Some(self.interpreter.ascii_string(data)),
            _ => None,
        };

        if let Some(Ok(data)) = &result.pinned_data {
            self.last_read = Some(LastRead {
                pinned_data: data.clone(),
                pinned_read_at: read_at_local,
            });
        }

        // Connection reflects whichever panel was read this cycle.
        let connection = match (&result.main_data, &result.pinned_data) {
            (Some(Ok(_)), _) | (_, Some(Ok(_))) => ConnectionStatus::Connected,
            (Some(Err(e)), _) | (_, Some(Err(e))) => ConnectionStatus::Error(e.clone()),
            _ => self.connection.clone(),
        };
        let read_ok =
            matches!(&result.main_data, Some(Ok(_))) || matches!(&result.pinned_data, Some(Ok(_)));

        {
            let params = self.read_mut();
            params.read_duration = Some(result.read_duration);
            params.loading = false;
            if let Some(s) = main_ascii {
                params.ascii_string = s;
            }
            if let Some(s) = pinned_ascii {
                params.pinned_ascii_string = s;
            }
            if let Some(Err(e)) = &result.main_data {
                params.main_rows = vec![e.clone()];
                params.main_changed = Vec::new();
                params.data_start = result.window_start;
            }
        }

        // Re-format from the (now partially updated) cache; the unread panel just
        // re-renders its previous data unchanged.
        if read_ok {
            self.rebuild_read_rows();
        }
        self.connection = connection;
    }

    pub fn rebuild_read_rows(&mut self) {
        let visible = self.visible_rows.get().max(1);
        let (window_start, register_type) = {
            let p = self.read();
            (p.window_start, p.register_type)
        };

        let mut main_rows = Vec::with_capacity(visible as usize);
        let mut main_changed = Vec::with_capacity(visible as usize);
        for i in 0..visible {
            let addr = window_start.saturating_add(i);
            let cell = (register_type, addr);
            let label = self.labels.get(&cell).map(String::as_str);
            match self.read_log.get(&cell) {
                Some(&(value, time)) => {
                    let neighbor = |offset: u16| {
                        self.read_log
                            .get(&(register_type, addr.saturating_add(offset)))
                            .map(|&(v, _)| v)
                            .unwrap_or(0)
                    };
                    main_rows.push(self.interpreter.format_row(
                        addr,
                        value,
                        [neighbor(1), neighbor(2), neighbor(3)],
                        time.with_timezone(&Local),
                        label,
                    ));
                    main_changed.push(self.changed.get(&cell).copied().unwrap_or(false));
                }
                None => {
                    main_rows.push(self.interpreter.placeholder(addr, label));
                    main_changed.push(false);
                }
            }
        }

        let (pinned_rows, pinned_changed) = match &self.last_read {
            Some(lr) => {
                let rows =
                    Self::format_pinned(&self.interpreter, &lr.pinned_data, lr.pinned_read_at, &self.labels);
                let changed = lr
                    .pinned_data
                    .iter()
                    .map(|&(cell, _)| self.changed.get(&cell).copied().unwrap_or(false))
                    .collect();
                (rows, changed)
            }
            None => (Vec::new(), Vec::new()),
        };

        let params = self.read_mut();
        params.main_rows = main_rows;
        params.main_changed = main_changed;
        params.pinned_rows = pinned_rows;
        params.pinned_changed = pinned_changed;
        params.data_start = window_start;
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

    pub fn commit_write(&mut self) {
        if self.background_task.is_some() {
            if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
                w.result = Some("Device is busy.".to_string());
            }
            return;
        }

        let (position, number, write_type) = {
            let Some(Popup::Write(w)) = &mut self.read_mut().popup else {
                return;
            };
            let Some(number) = w.value else {
                w.result = Some("Enter a value first.".to_string());
                return;
            };
            w.result = Some("Writing...".to_string());
            (w.position, number, w.write_type)
        };

        let device = self.device.clone();
        self.background_task = Some(BackgroundTask::Write(tokio::spawn(async move {
            let result = match write_type {
                WriteType::Word => device.write_register(position, number as u16).await,
                WriteType::DWord => device.write_register_word(position, number as i32).await,
            };
            match result {
                Ok(()) => "Write OK".to_string(),
                Err(e) => format!("Write failed: {e}"),
            }
        })));
    }

    /// Number of editable bits for the open write popup (16 for word, 32 dword).
    fn write_bit_count(&self) -> u16 {
        match &self.read().popup {
            Some(Popup::Write(w)) => match w.write_type {
                WriteType::Word => 16,
                WriteType::DWord => 32,
            },
            _ => 16,
        }
    }

    pub fn write_toggle_type(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            w.write_type = match w.write_type {
                WriteType::Word => WriteType::DWord,
                WriteType::DWord => WriteType::Word,
            };
            let bits = match w.write_type {
                WriteType::Word => 16,
                WriteType::DWord => 32,
            };
            w.bit_cursor = w.bit_cursor.min(bits - 1);
        }
        // The narrower width may no longer fit the current value.
        self.clamp_write_value();
    }

    pub fn clamp_write_value(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            if let Some(value) = w.value {
                let (lo, hi) = match w.write_type {
                    WriteType::Word => (i16::MIN as i64, u16::MAX as i64),
                    WriteType::DWord => (i32::MIN as i64, u32::MAX as i64),
                };
                w.value = Some(value.clamp(lo, hi));
            }
        }
    }

    pub fn write_move_bit(&mut self, left: bool) {
        let bits = self.write_bit_count();
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            w.bit_cursor = if left {
                (w.bit_cursor + 1).min(bits - 1)
            } else {
                w.bit_cursor.saturating_sub(1)
            };
        }
    }

    pub fn write_toggle_bit(&mut self) {
        if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
            let mask = 1u32 << w.bit_cursor;
            let current = w.value.unwrap_or(0) as u32;
            w.value = Some((current ^ mask) as i64);
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
                    {
                        let params = self.read_mut();
                        params.main_rows = vec![message.clone()];
                        params.main_changed = Vec::new();
                        params.loading = false;
                    }
                    self.connection = ConnectionStatus::Error(message);
                }
            },
            BackgroundTask::Write(handle) => {
                let result = handle.await.unwrap_or_else(|e| e.to_string());
                if let Some(Popup::Write(w)) = &mut self.read_mut().popup {
                    w.result = Some(result);
                }
            }
        }
    }

    pub fn toggle_type(&mut self) {
        let p = self.read_mut();
        p.main_rows = Vec::new();
        p.read_duration = None;
        p.ascii_string = String::new();
        p.main_changed = Vec::new();
        p.register_type.toggle();
    }
}
