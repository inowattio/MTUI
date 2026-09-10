use super::{App, BackgroundTask, ExportPayload, ImportPayload, LoadConfigTaskResult, save_config};
use crate::compat;
use crate::config::{Config, CustomRules, Startup};
use crate::custom::CustomRule;
use crate::modbus::ModbusDevice;
use crate::register::RegisterCell;
use crate::state::{
    ConnectionStatus, DumpParams, ImportParams, Outcome, Popup, State, StatusMessage,
};
use std::collections::BTreeMap;
use std::fs;

impl App {
    pub fn open_dump(&mut self) {
        self.read_mut().popup = Some(Popup::Dump(DumpParams::default()));
    }

    fn parse_import(data: &str) -> Option<ImportPayload> {
        let payload: ImportPayload = serde_json::from_str(data.trim()).ok()?;
        (payload.total() > 0).then_some(payload)
    }

    pub fn paste_import(&mut self, data: &str) {
        match Self::parse_import(data) {
            Some(payload) => {
                let params = ImportParams {
                    pins: payload.pins(),
                    labels: payload.labels(),
                    rules: payload.rules(),
                };
                self.pending_import = Some(payload);
                self.read_mut().popup = Some(Popup::Import(params));
            }
            None => self.set_read_status(StatusMessage::warn(
                "Pasted text isn't pinned/labels/custom data",
            )),
        }
    }

    pub fn cancel_import(&mut self) {
        self.pending_import = None;
        self.close_popup();
    }

    pub fn apply_import(&mut self) {
        let Some(payload) = self.pending_import.take() else {
            self.close_popup();
            return;
        };

        let mut pins = 0;
        if let Some(p) = payload.pinned_registers {
            let incoming: Vec<RegisterCell> = p.into();
            pins = incoming.len();
            for cell in incoming {
                if !self.pinned_registers.contains(&cell) {
                    self.pinned_registers.push(cell);
                }
            }
            self.pinned_registers.sort();
            self.pinned_registers.dedup();
        }

        let mut labels = 0;
        if let Some(l) = payload.labels {
            let incoming: BTreeMap<RegisterCell, String> = l.into();
            labels = incoming.len();
            self.labels.extend(incoming);
        }

        let mut rules = 0;
        if let Some(r) = payload.custom_rules {
            let incoming: BTreeMap<RegisterCell, CustomRule> = r.into();
            rules = incoming.len();
            self.custom_rules.extend(incoming);
        }

        self.refresh_dirty();
        self.close_popup();
        log::info!("Imported {pins} pin(s), {labels} label(s), {rules} rule(s) from clipboard");
        self.set_read_status(StatusMessage::ok(format!(
            "Imported {pins} pin(s), {labels} label(s), {rules} rule(s)"
        )));
    }

    fn export_payload_json(&self) -> String {
        let payload = ExportPayload {
            pinned_registers: self.pinned_registers.as_slice().into(),
            labels: (&self.labels).into(),
            custom_rules: (&self.custom_rules).into(),
        };
        serde_json::to_string_pretty(&payload).unwrap_or_default()
    }

    pub fn copy_data_to_clipboard(&mut self) {
        let pins = self.pinned_registers.len();
        let labels = self.labels.len();
        let rules = self.custom_rules.len();
        let json = self.export_payload_json();
        let message = if self.set_clipboard(json) {
            log::info!("Copied {pins} pin(s), {labels} label(s), {rules} rule(s) to clipboard");
            StatusMessage::ok(format!(
                "Copied {pins} pin(s), {labels} label(s), {rules} rule(s) to clipboard"
            ))
        } else {
            StatusMessage::err("Clipboard unavailable")
        };
        self.set_settings_status(message);
    }

    fn config_json(&self) -> String {
        let mut config = self.effective_config();
        if let Some(startup) = self.current_position() {
            config.startup = startup;
        }
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }

    pub fn copy_config_to_clipboard(&mut self) {
        let json = self.config_json();
        let message = if self.set_clipboard(json) {
            log::info!("Copied the configuration to clipboard");
            StatusMessage::ok("Copied the configuration to clipboard")
        } else {
            StatusMessage::err("Clipboard unavailable")
        };
        self.set_settings_status(message);
    }

    fn effective_config(&self) -> Config {
        let mut config = self.config.clone();
        config.labels = (&self.labels).into();

        let rebuilt: CustomRules = (&self.custom_rules).into();
        config.custom_rules.holdings = rebuilt.holdings;
        config.custom_rules.inputs = rebuilt.inputs;
        config.custom_rules.coils = rebuilt.coils;
        config.custom_rules.discretes = rebuilt.discretes;

        config.pinned_registers = self.pinned_registers.as_slice().into();
        config.interpretations = self.interpreter.config();
        config
    }

    fn serialize_config(config: &Config) -> String {
        serde_json::to_string(config).unwrap_or_default()
    }

    pub(super) fn mark_config_saved(&mut self) {
        self.saved_config = Self::serialize_config(&self.effective_config());
        self.dirty = false;
    }

    fn save_then(&mut self, next: fn(&mut Self)) {
        match self.persist_config() {
            Ok(_) => {
                log::info!("Configuration saved");
                next(self);
            }
            Err(error) => {
                log::error!("Save failed | {error}");
                self.close_popup();
                self.set_read_status(StatusMessage::err(error));
            }
        }
    }

    pub fn save_and_quit(&mut self) {
        self.save_then(Self::quit);
    }

    pub fn save_and_cycle_config(&mut self) {
        self.save_then(Self::confirm_cycle_config);
    }

    pub(super) fn refresh_dirty(&mut self) {
        self.dirty = Self::serialize_config(&self.effective_config()) != self.saved_config;
    }

    pub(super) fn persist_config(&mut self) -> Outcome {
        self.config = self.effective_config();
        if let State::Read(p) = &self.state {
            self.config.startup = Startup {
                address: p.position,
                register_type: p.register_type,
                panel: p.panel,
            };
        }

        let result = save_config(&self.config_path, &self.config)
            .map(|()| format!("Saved to {}", self.config_path))
            .map_err(|e| format!("Save failed: {e}"));
        if result.is_ok() {
            self.mark_config_saved();
        }
        result
    }

    fn current_position(&self) -> Option<Startup> {
        let p = match &self.state {
            State::Read(p) => p,
            State::Settings(s) => &s.previous,
            State::Logs(_) => return None,
        };
        Some(Startup {
            address: p.position,
            register_type: p.register_type,
            panel: p.panel,
        })
    }

    pub(super) fn persist_startup_position(&self) {
        let Some(startup) = self.current_position() else {
            return;
        };
        let mut config: Config = match serde_json::from_str(&self.saved_config) {
            Ok(config) => config,
            Err(error) => {
                log::error!("Startup position not saved | {error}");
                return;
            }
        };
        config.startup = startup;
        match save_config(&self.config_path, &config) {
            Ok(()) => log::info!(
                "Startup position saved | {} {} on {}",
                startup.register_type.name(),
                startup.address,
                startup.panel.name()
            ),
            Err(error) => log::error!("Startup position not saved | {error}"),
        }
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    fn read_config_file(path: &str) -> Result<Config, String> {
        if path.is_empty() {
            return Err("Load failed: enter a file name".to_string());
        }
        let content = fs::read_to_string(path).map_err(|e| format!("Load failed: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Load failed: {e}"))
    }

    pub(super) fn load_config_from(&mut self, path: String) -> StatusMessage {
        let config = match Self::read_config_file(&path) {
            Ok(config) => config,
            Err(error) => {
                log::error!("{error}");
                return StatusMessage::err(error);
            }
        };
        if !self.free_background_slot() {
            return StatusMessage::info("Device is busy.");
        }
        self.spawn_config_load(path, config);
        StatusMessage::info("Loading\u{2026}")
    }

    fn spawn_config_load(&mut self, path: String, config: Config) {
        let previous = self.take_device();
        let device_config = config.device.clone();
        self.background_task = Some(BackgroundTask::LoadConfig(compat::spawn(async move {
            let result = ModbusDevice::replace(previous, &device_config)
                .await
                .map_err(|e| e.to_string());
            LoadConfigTaskResult {
                path,
                config: Box::new(config),
                result,
            }
        })));
    }

    pub(super) fn apply_load_config_result(&mut self, result: Option<LoadConfigTaskResult>) {
        let outcome: Outcome = match result {
            Some(LoadConfigTaskResult {
                path,
                config,
                result,
            }) => match result {
                Ok(device) => {
                    self.apply_config(*config, Some(device));

                    self.config_path = path.clone();
                    self.mark_config_saved();

                    let read = self.startup_read_params();
                    match &mut self.state {
                        State::Settings(s) => s.previous = read,
                        State::Read(p) => *p = read,
                        _ => {}
                    }

                    Ok(format!("Loaded {path}"))
                }
                Err(e) => {
                    if self.device.is_none() {
                        self.connection = ConnectionStatus::Error(e.clone());
                        self.logged_connection = self.connection.clone();
                    }
                    Err(format!("Load failed: device: {e}"))
                }
            },
            None => Err("Load failed: task stopped unexpectedly".to_string()),
        };

        match &outcome {
            Ok(message) => log::info!("{message}"),
            Err(error) => log::error!("{error}"),
        }
        match &self.state {
            State::Read(_) => self.set_read_status(outcome.into()),
            State::Settings(_) => self.set_settings_status(outcome.into()),
            _ => {}
        }
    }

    fn cycle_target(&self) -> Option<String> {
        let next = self.config.next_config.trim();
        if !next.is_empty() {
            Some(next.to_string())
        } else if self.config_path != self.origin_config_path {
            Some(self.origin_config_path.clone())
        } else {
            None
        }
    }

    pub fn cycle_config(&mut self) {
        let Some(target) = self.cycle_target() else {
            self.set_read_status(StatusMessage::info("No next configuration set"));
            return;
        };

        if self.dirty && !self.config.ignore_dirty {
            self.read_mut().popup = Some(Popup::CycleConfig);
            return;
        }

        self.load_config_target(target);
    }

    pub fn confirm_cycle_config(&mut self) {
        self.close_popup();
        if let Some(target) = self.cycle_target() {
            self.load_config_target(target);
        }
    }

    fn load_config_target(&mut self, target: String) {
        let status = self.load_config_from(target);
        self.set_read_status(status);
    }

    pub fn commit_dump(&mut self) {
        let result = self.dump_read_log();
        if let Some(d) = self.popup_as_mut::<DumpParams>() {
            d.result = Some(result);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{App, BackgroundTask};
    use crate::config::Config;
    use crate::state::MessageKind;

    async fn app_with_refresh_in_flight() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.refresh().await;
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::Refresh(_))
        ));
        app.open_settings();
        app
    }

    fn settings_status(app: &App) -> MessageKind {
        app.settings()
            .and_then(|s| s.status.as_ref())
            .map(|s| s.kind)
            .expect("a status is shown")
    }

    #[tokio::test]
    async fn the_copied_configuration_is_a_loadable_config_with_the_current_position() {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.config.name = "copied".to_string();
        app.read_mut().position = 77;
        app.open_settings();

        let parsed: Config = serde_json::from_str(&app.config_json()).expect("valid config json");
        assert_eq!(parsed.name, "copied");
        assert_eq!(parsed.startup.address, 77);
        assert_eq!(
            parsed.labels.inputs.len(),
            Config::demo().labels.inputs.len()
        );
    }

    #[tokio::test]
    async fn the_exported_data_imports_back_unchanged() {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.pinned_registers = vec![
            (crate::register::RegisterType::Holding, 9),
            (crate::register::RegisterType::Coil, 2),
        ];
        let json = app.export_payload_json();

        let payload = App::parse_import(&json).expect("export is importable");
        assert_eq!(payload.pins(), 2);
        assert_eq!(payload.labels(), app.labels.len());
        assert_eq!(payload.rules(), app.custom_rules.len());

        let mut other = App::boot(Config::default(), String::new()).await;
        other.paste_import(&json);
        other.apply_import();
        assert_eq!(other.pinned_registers, app.pinned_registers);
        assert_eq!(other.labels, app.labels);
        assert_eq!(other.custom_rules, app.custom_rules);
    }

    #[tokio::test]
    async fn an_unreadable_path_leaves_the_pending_read_and_device_alone() {
        let mut app = app_with_refresh_in_flight().await;
        let device = app.device.clone().expect("mock device");

        for path in ["", "/nonexistent/mtui-config.json"] {
            app.settings_mut().unwrap().load_path = path.to_string();
            app.settings_load();
            assert_eq!(settings_status(&app), MessageKind::Err);
        }

        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::Refresh(_))
        ));
        assert!(!device.is_poisoned());
    }

    #[tokio::test]
    async fn a_valid_path_frees_the_slot_and_starts_loading() {
        let dir = std::env::temp_dir().join(format!("mtui-{}-load", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, serde_json::to_string(&Config::default()).unwrap()).unwrap();

        let mut app = app_with_refresh_in_flight().await;
        app.settings_mut().unwrap().load_path = path.to_string_lossy().into_owned();
        app.settings_load();

        assert_eq!(settings_status(&app), MessageKind::Info);
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::LoadConfig(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
