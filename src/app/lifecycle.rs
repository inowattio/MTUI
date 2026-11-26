use super::{
    App, CommStats, ConfigError, ReconnectState, RegisterMap, SweepState, default_config_path,
    load_config,
};
use crate::compat;
use crate::config::Config;
use crate::custom::CustomRules;
use crate::interpretator::Interpretor;
use crate::modbus::{self, ModbusDevice};
use crate::register::RegisterCell;
use crate::state::{
    ConnectionStatus, Popup, PopupKind, PopupPayload, ReadParams, ScreenLayout, State,
    StatusMessage,
};
use crate::writes_log::WritesLogState;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type AppViews = (
    Vec<RegisterCell>,
    BTreeMap<RegisterCell, String>,
    CustomRules,
);

impl App {
    pub async fn new(
        config_path: Option<PathBuf>,
        make_config_if_none: bool,
    ) -> Result<Self, ConfigError> {
        let create_if_missing = make_config_if_none || config_path.is_none();
        let config_path = config_path.unwrap_or_else(default_config_path);
        let config = load_config(&config_path, create_if_missing)?;
        Ok(Self::boot(config, config_path).await)
    }

    pub async fn boot(config: Config, config_path: impl Into<PathBuf>) -> Self {
        let config_path = config_path.into();
        let device = ModbusDevice::new(&config.device)
            .await
            .inspect_err(|e| println!("Could not initialize device: {e}"))
            .ok();

        let (interpreter, (pinned_registers, labels, custom_rules)) =
            Self::derive_config_views(&config);
        let mut app = Self {
            origin_config_path: config_path.clone(),
            config_path,
            interpreter,
            pinned_registers,
            labels,
            custom_rules,
            state: State::Read(ReadParams::default()),
            config,
            device,
            running: true,
            connection: ConnectionStatus::Unknown,
            frame: 0,
            last_frame: Duration::ZERO,
            ram_bytes: None,
            paused: false,
            headless: false,
            dirty: false,
            saved_config: String::new(),
            sweep: SweepState::default(),
            stats: Arc::new(Mutex::new(CommStats::default())),
            reconnect: ReconnectState::default(),
            layout: ScreenLayout::default(),
            viewport_width: 0,
            background_task: None,
            network_scan: None,
            unit_scan: None,
            #[cfg(not(target_arch = "wasm32"))]
            network_scan_task: None,
            #[cfg(not(target_arch = "wasm32"))]
            ports_task: None,
            changed: BTreeMap::new(),
            custom_changed: BTreeMap::new(),
            read_log: BTreeMap::new(),
            value_history: BTreeMap::new(),
            pending_write: None,
            queued_write: None,
            pending_import: None,
            library: super::library::LibraryState::default(),
            logged_connection: ConnectionStatus::Unknown,
            api_device: Arc::new(Mutex::new(None)),
            api_bound_port: Arc::new(AtomicU16::new(0)),
            api_read_only: Arc::new(AtomicBool::new(false)),
            api_allow_unit_id: Arc::new(AtomicBool::new(false)),
            api_allow_raw: Arc::new(AtomicBool::new(false)),
            api_allow_mcp: Arc::new(AtomicBool::new(false)),
            api_registers: Arc::new(Mutex::new(RegisterMap::default())),
            api_status: Arc::new(AtomicU8::new(0)),
            api_bind: Arc::new(AtomicU8::new(0)),
            writes_log: Arc::new(Mutex::new(WritesLogState::default())),
            #[cfg(not(target_arch = "wasm32"))]
            api_server: None,
            #[cfg(not(target_arch = "wasm32"))]
            api_server_addr: None,
            #[cfg(not(target_arch = "wasm32"))]
            clipboard: None,
        };

        app.sync_raw_exceptions();
        app.sync_api_device();
        app.sync_api_read_only();
        app.sync_api_allow_unit_id();
        app.sync_api_allow_raw();
        app.sync_api_allow_mcp();
        app.sync_api_registers();
        app.refresh_writes_log_state();

        app.sync_auto_widths();
        app.mark_config_saved();
        app.layout.visible_rows = app.config.batch.size.max(1);

        if app.device.is_some() {
            app.state = State::Read(app.startup_read_params());
            log::info!("Started | {}", app.config.display_device());
            if app.config.cycle_register_types.enabled_count() == 0 {
                app.notify_no_cycle_types();
            }
        } else {
            app.state = State::Read(app.startup_read_params());
            app.open_discovery();
            log::warn!("Started | no device, opened the Device popup");
        }

        if app.config.legacy {
            let msg = "Loaded a pre-1.0 config | delete it and restart to regenerate";
            log::warn!("{msg}");
            app.set_read_status(StatusMessage::warn(msg));
        }

        #[cfg(not(target_arch = "wasm32"))]
        app.reconcile_api_server();

        app
    }

    fn derive_config_views(config: &Config) -> (Interpretor, AppViews) {
        let (pinned, labels, rules) = config.registers.clone().into_views();
        (
            Interpretor::new(config.columns.clone(), config.device.word_order),
            (pinned, labels, rules.into()),
        )
    }

    pub(super) fn sync_raw_exceptions(&self) {
        modbus::RAW_EXCEPTIONS.store(self.config.raw_exceptions, Ordering::Relaxed);
    }

    pub(super) fn apply_config(&mut self, config: Config, device: Option<ModbusDevice>) {
        (
            self.interpreter,
            (self.pinned_registers, self.labels, self.custom_rules),
        ) = Self::derive_config_views(&config);
        self.device = device;
        self.config = config;
        self.sweep.active = false;

        self.sync_raw_exceptions();
        self.sync_api_device();
        self.sync_api_read_only();
        self.sync_api_allow_unit_id();
        self.sync_api_allow_raw();
        self.sync_api_allow_mcp();
        self.sync_api_registers();
        self.refresh_writes_log_state();

        self.clear_read_accumulation();
        self.sync_auto_widths();
        self.connection = ConnectionStatus::Unknown;
        self.logged_connection = ConnectionStatus::Unknown;
        self.reconnect = ReconnectState::default();
    }

    pub(super) fn clear_read_accumulation(&mut self) {
        self.changed.clear();
        self.custom_changed.clear();
        self.read_log.clear();
        self.value_history.clear();
        *self.stats() = CommStats::default();
    }

    pub(super) fn startup_read_params(&self) -> ReadParams {
        ReadParams {
            position: self.config.startup.address,
            window_start: self.config.startup.address,
            register_type: self.config.startup.register_type,
            panel: self.config.startup.panel,
            ..Default::default()
        }
    }

    pub fn read(&self) -> &ReadParams {
        match &self.state {
            State::Read(p) => p,
            _ => unreachable!("read() called outside the Read state"),
        }
    }

    pub fn read_mut(&mut self) -> &mut ReadParams {
        match &mut self.state {
            State::Read(p) => p,
            _ => unreachable!("read_mut() called outside the Read state"),
        }
    }

    pub(super) const fn is_reading(&self) -> bool {
        matches!(self.state, State::Read(_))
    }

    pub fn popup_kind(&self) -> Option<PopupKind> {
        self.read().popup.as_ref().map(Popup::kind)
    }

    pub fn popup_as<T: PopupPayload>(&self) -> Option<&T> {
        match &self.state {
            State::Read(p) => p.popup.as_ref().and_then(T::from_popup),
            _ => None,
        }
    }

    pub fn popup_as_mut<T: PopupPayload>(&mut self) -> Option<&mut T> {
        match &mut self.state {
            State::Read(p) => p.popup.as_mut().and_then(T::from_popup_mut),
            _ => None,
        }
    }

    pub fn close_popup(&mut self) {
        self.queued_write = None;
        self.cancel_network_scan();
        if let Some(Popup::Unit(params)) = self.read_mut().popup.take() {
            let endpoint = self.config.device.interface.endpoint();
            self.unit_scan = Some((endpoint, params.suspended()));
        }
    }

    pub fn set_read_status(&mut self, message: StatusMessage) {
        self.read_mut().set_status(message);
    }

    pub fn request_quit(&mut self) {
        if self.read().graph {
            self.read_mut().graph = false;
            return;
        }
        if self.dirty && !self.config.skip_unsaved_warning {
            self.read_mut().popup = Some(Popup::Quit);
        } else {
            self.quit();
        }
    }

    pub fn interrupt(&mut self) {
        let prompting = matches!(
            &self.state,
            State::Read(p) if p.popup == Some(Popup::Quit)
        );
        if prompting || !self.dirty || self.config.skip_unsaved_warning {
            self.quit();
            return;
        }

        self.close_settings();
        self.close_log_view();
        self.close_diff_view();
        self.close_library_view();
        self.read_mut().popup = Some(Popup::Quit);
    }

    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        if self.config.display.ram && (self.ram_bytes.is_none() || self.frame.is_multiple_of(10)) {
            self.ram_bytes = compat::ram_bytes();
        }
        self.sync_api_status();
        #[cfg(not(target_arch = "wasm32"))]
        self.reconcile_api_server();
        self.complete_background_task();
        if self.background_task.is_some() {
            return;
        }

        if self.is_reading() {
            self.scroll_to_cursor();
        }

        if self.maybe_reconnect() {
            return;
        }

        if self.sweep.active {
            if self.is_reading() {
                let current = self.sweep.current;
                self.read_mut().position = current;
                self.scroll_to_cursor();
                self.refresh();
            }
            return;
        }

        let should_refresh = !self.paused
            && !self.headless
            && matches!(
                &self.state,
                State::Read(p)
                    if self.config.refresh_interval_ms > 0
                        && p.refresh_timer.elapsed().as_millis()
                            >= self.config.refresh_interval_ms as u128
            );

        if should_refresh {
            self.refresh();
        }
    }

    pub fn adjust_batch(&mut self, increase: bool) {
        let batch = self.config.batch.size;
        self.config.batch.size = if increase {
            batch.saturating_add(1)
        } else {
            batch.saturating_sub(1).max(1)
        };
        self.refresh_dirty();
    }

    pub fn toggle_pause(&mut self) {
        self.paused.toggle();
        if self.paused {
            log::info!("Auto-refresh paused");
        } else {
            log::info!("Auto-refresh resumed");
        }
    }

    pub fn quit(&mut self) {
        if self.config.save_position_on_exit {
            self.persist_startup_position();
        }
        self.running = false;
    }

    pub(super) const fn read_or_previous_mut(&mut self) -> &mut ReadParams {
        match &mut self.state {
            State::Read(p) => p,
            State::Settings(s) => &mut s.previous,
            State::Logs(l) => &mut l.previous,
            State::Diff(d) => &mut d.previous,
            State::Library(l) => &mut l.previous,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use super::App;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::config::{Config, CyclePanels};
    use crate::register::RegisterType;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::scratch::ScratchDir;
    #[cfg(not(target_arch = "wasm32"))]
    use crate::state::ReadPanel;

    #[cfg(not(target_arch = "wasm32"))]
    fn saved(scratch: &ScratchDir) -> Config {
        let content =
            std::fs::read_to_string(scratch.path("config.json")).expect("config written on exit");
        serde_json::from_str(&content).expect("valid config")
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn quitting_saves_the_position_as_startup_when_enabled() {
        let scratch = ScratchDir::new("save-position");
        let mut app = App::boot(Config::default(), scratch.path("config.json")).await;
        app.config.save_position_on_exit = true;
        app.config.name = "unsaved".to_string();
        {
            let read = app.read_mut();
            read.position = 42;
            read.register_type = RegisterType::Holding;
            read.panel = ReadPanel::Pinned;
        }

        app.quit();
        assert!(!app.running);

        let saved = saved(&scratch);
        assert_eq!(saved.startup.address, 42);
        assert_eq!(saved.startup.register_type, RegisterType::Holding);
        assert_eq!(saved.startup.panel, ReadPanel::Pinned);
        assert_eq!(
            saved.name, "demo",
            "only the position is written, unsaved changes stay unsaved"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn quitting_leaves_the_config_alone_when_disabled() {
        let scratch = ScratchDir::new("keep-position");
        let mut app = App::boot(Config::default(), scratch.path("config.json")).await;
        assert!(!app.config.save_position_on_exit, "off by default");
        app.read_mut().position = 42;

        app.quit();
        assert!(!app.running);
        assert!(!scratch.path("config.json").exists());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn a_clean_quit_request_also_saves_the_position() {
        let scratch = ScratchDir::new("request-quit-position");
        let mut app = App::boot(Config::default(), scratch.path("config.json")).await;
        app.config.save_position_on_exit = true;
        app.mark_config_saved();
        app.read_mut().position = 7;

        app.request_quit();
        assert!(!app.running);
        assert_eq!(saved(&scratch).startup.address, 7);
        assert!(
            saved(&scratch).save_position_on_exit,
            "a saved toggle is kept in the file"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn panel_cycle_skips_disabled_panels() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.cycle_panels.pinned = false;
        app.config.cycle_panels.custom = false;
        app.read_mut().panel = ReadPanel::Main;

        app.toggle_panel();
        assert_eq!(app.read_mut().panel, ReadPanel::Labeled);
        app.toggle_panel();
        assert_eq!(app.read_mut().panel, ReadPanel::Matrix);
        app.toggle_panel();
        assert_eq!(app.read_mut().panel, ReadPanel::Main);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn panel_cycle_stays_on_main_when_every_panel_is_disabled() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.cycle_panels = CyclePanels {
            pinned: false,
            labeled: false,
            custom: false,
            matrix: false,
        };
        app.read_mut().panel = ReadPanel::Main;

        app.toggle_panel();
        assert_eq!(app.read_mut().panel, ReadPanel::Main);
    }
}
