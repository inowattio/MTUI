use super::{App, BackgroundTask, LoadConfigTaskResult, save_config};
use crate::compat;
use crate::config::{Config, Registers, Startup};
use crate::constants::message;
use crate::import::mbmap;
use crate::import::{Device, home_assistant, solarman};
use crate::modbus::ModbusDevice;
use crate::num_ops::wrap_index;
use crate::state::{
    ConnectionStatus, FirmwareInput, ImportDevice, ImportParams, ImportSource, Outcome, Popup,
    State, StatusMessage,
};
use std::fs;
use std::path::{Path, PathBuf};

type ImportParser = fn(&str) -> Option<Vec<Device>>;

struct ParsedImport {
    devices: Vec<Device>,
    source: ImportSource,
}

#[derive(Debug)]
pub(super) struct PendingImport {
    devices: Vec<Registers>,
    map: Option<String>,
}

fn summary(device: &Device) -> ImportDevice {
    ImportDevice {
        name: device.name.clone(),
        pins: device.registers.pins(),
        labels: device.registers.labels(),
        rules: device.registers.rules(),
        replaced: device.replaced,
    }
}

impl App {
    fn parse_import(data: &str) -> Option<ParsedImport> {
        let parsers: [(ImportParser, ImportSource); 4] = [
            (
                |data| Some(vec![Self::parse_json_import(data)?.into()]),
                ImportSource::Mtui,
            ),
            (
                |data| Some(vec![mbmap::parse(data)?.into()]),
                ImportSource::Mbmap,
            ),
            (home_assistant::parse, ImportSource::HomeAssistant),
            (solarman::parse, ImportSource::Solarman),
        ];
        parsers.into_iter().find_map(|(parse, source)| {
            Some(ParsedImport {
                devices: parse(data)?,
                source,
            })
        })
    }

    fn parse_json_import(data: &str) -> Option<Registers> {
        let value: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
        let value = value.get("registers").cloned().unwrap_or(value);
        let registers: Registers = serde_json::from_value(value).ok()?;
        (registers.total() > 0).then_some(registers)
    }

    pub fn import_text(&mut self, data: &str) -> bool {
        let Some(ParsedImport { devices, source }) = Self::parse_import(data) else {
            let Some(version) = mbmap::unsupported_version(data) else {
                return false;
            };
            log::warn!("Import ignored | .mbmap version {version} is not supported");
            self.set_read_status(StatusMessage::warn(format!(
                "Unsupported .mbmap version {version} - only version {} can be imported",
                mbmap::VERSION
            )));
            return true;
        };
        let map = (source == ImportSource::Mbmap).then_some(data);
        self.open_import(devices, source, map);
        true
    }

    pub(super) fn open_import(
        &mut self,
        devices: Vec<Device>,
        source: ImportSource,
        map: Option<&str>,
    ) {
        let firmware = map.and_then(mbmap::firmware);
        let params = ImportParams {
            source,
            devices: devices.iter().map(summary).collect(),
            selected: 0,
            firmware: firmware.map(|known| FirmwareInput {
                text: String::new(),
                known,
            }),
            revision: map.and_then(mbmap::revision),
            status: map.and_then(mbmap::status),
        };
        self.pending_import = Some(PendingImport {
            devices: devices.into_iter().map(|device| device.registers).collect(),
            map: map
                .filter(|_| params.firmware.is_some())
                .map(str::to_string),
        });
        self.read_mut().popup = Some(Popup::Import(params));
    }

    pub fn editing_import_firmware(&self) -> bool {
        self.popup_as::<ImportParams>()
            .is_some_and(|p| p.firmware.is_some())
    }

    pub fn import_firmware_input(&mut self, c: char) {
        if let Some(firmware) = self
            .popup_as_mut::<ImportParams>()
            .and_then(|p| p.firmware.as_mut())
        {
            firmware.text.push(c);
            self.refilter_import();
        }
    }

    pub fn import_firmware_backspace(&mut self) {
        if let Some(firmware) = self
            .popup_as_mut::<ImportParams>()
            .and_then(|p| p.firmware.as_mut())
        {
            firmware.text.pop();
            self.refilter_import();
        }
    }

    fn refilter_import(&mut self) {
        let Some(firmware) = self
            .popup_as::<ImportParams>()
            .and_then(|p| p.firmware.as_ref())
            .map(|firmware| firmware.text.clone())
        else {
            return;
        };
        let Some(map) = self.pending_import.as_ref().and_then(|p| p.map.as_deref()) else {
            return;
        };
        let device = Device::from(mbmap::parse_for_firmware(map, &firmware).unwrap_or_default());
        if let Some(p) = self.popup_as_mut::<ImportParams>() {
            p.devices = vec![summary(&device)];
            p.selected = 0;
        }
        if let Some(pending) = &mut self.pending_import {
            pending.devices = vec![device.registers];
        }
    }

    pub fn import_move(&mut self, down: bool) {
        if let Some(p) = self.popup_as_mut::<ImportParams>() {
            p.selected = wrap_index(p.selected, p.devices.len() as u16, down);
        }
    }

    pub fn cancel_import(&mut self) {
        self.pending_import = None;
        self.close_popup();
    }

    pub fn apply_import(&mut self) {
        let selected = self
            .popup_as::<ImportParams>()
            .map_or(0, |p| usize::from(p.selected));
        let Some(payload) = self
            .pending_import
            .take()
            .and_then(|pending| pending.devices.into_iter().nth(selected))
        else {
            self.close_popup();
            return;
        };

        let (pins, labels, rules) = (payload.pins(), payload.labels(), payload.rules());
        let (pinned, incoming_labels, incoming_rules) = payload.into_views();
        for cell in pinned {
            if !self.pinned_registers.contains(&cell) {
                self.pinned_registers.push(cell);
            }
        }
        self.pinned_registers.sort();
        self.labels.extend(incoming_labels);
        self.custom_rules.extend(incoming_rules);

        self.sync_auto_widths();
        self.sync_api_registers();
        self.refresh_dirty();
        self.close_popup();
        log::info!("Imported {pins} pin(s), {labels} label(s), {rules} rule(s)");
        self.set_read_status(StatusMessage::ok(format!(
            "Imported {pins} pin(s), {labels} label(s), {rules} rule(s)"
        )));
    }

    fn export_payload_json(&self) -> String {
        serde_json::to_string_pretty(&self.registers()).unwrap_or_default()
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
            StatusMessage::err(message::CLIPBOARD_UNAVAILABLE)
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
            StatusMessage::ok(message::CONFIG_COPIED)
        } else {
            StatusMessage::err(message::CLIPBOARD_UNAVAILABLE)
        };
        self.set_settings_status(message);
    }

    fn registers(&self) -> Registers {
        Registers::from_views(&self.pinned_registers, &self.labels, &self.custom_rules)
    }

    fn effective_config(&self) -> Config {
        let mut config = self.config.clone();
        config.registers = self.registers();
        config.columns = self.interpreter.config();
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

    pub fn save_and_load_config(&mut self) {
        self.save_then(Self::confirm_load_config);
    }

    pub fn config_changes(&self) -> Vec<super::ConfigChange> {
        let saved = serde_json::from_str(&self.saved_config).unwrap_or_default();
        let current = serde_json::to_value(self.effective_config()).unwrap_or_default();
        super::changes::config_changes(&saved, &current)
    }

    pub fn refresh_dirty(&mut self) {
        self.dirty = !self.config_changes().is_empty();
    }

    pub(super) fn persist_config(&mut self) -> Outcome {
        self.config = self.effective_config();

        let result = save_config(&self.config_path, &self.config)
            .map(|()| format!("Saved to {}", self.config_path.display()))
            .map_err(|e| format!("Save failed: {e}"));
        if result.is_ok() {
            self.mark_config_saved();
        }
        result
    }

    const fn current_position(&self) -> Option<Startup> {
        let p = match &self.state {
            State::Read(p) => p,
            State::Settings(s) => &s.previous,
            State::Diff(d) => &d.previous,
            State::Library(l) => &l.previous,
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

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    fn read_config_file(path: &Path) -> Result<Config, String> {
        if path.is_empty() {
            return Err(message::LOAD_NEEDS_FILE_NAME.to_string());
        }
        let content = fs::read_to_string(path).map_err(|e| format!("Load failed: {e}"))?;
        serde_json::from_str(&content).map_err(|e| format!("Load failed: {e}"))
    }

    pub(super) fn load_config_from(&mut self, path: PathBuf) -> StatusMessage {
        let config = match Self::read_config_file(&path) {
            Ok(config) => config,
            Err(error) => {
                log::error!("{error}");
                return StatusMessage::err(error);
            }
        };
        if !self.free_background_slot() {
            return StatusMessage::info(message::DEVICE_BUSY);
        }
        self.spawn_config_load(path, config);
        StatusMessage::info(message::LOADING)
    }

    fn spawn_config_load(&mut self, path: PathBuf, config: Config) {
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
        let status = match result {
            Some(LoadConfigTaskResult {
                path,
                config,
                result,
            }) => {
                let (device, error) = match result {
                    Ok(device) => (Some(device), None),
                    Err(e) => (None, Some(e)),
                };
                self.apply_config(*config, device);

                self.config_path = path.clone();
                self.mark_config_saved();

                *self.read_or_previous_mut() = self.startup_read_params();
                self.rediff();

                match error {
                    None => {
                        let message = format!("Loaded {}", path.display());
                        log::info!("{message}");
                        StatusMessage::ok(message)
                    }
                    Some(e) => {
                        self.connection = ConnectionStatus::Error(e.clone());
                        self.logged_connection = self.connection.clone();
                        self.reconnect.link_lost = true;
                        let message = format!("Loaded {}, device: {e}", path.display());
                        log::warn!("{message}");
                        StatusMessage::warn(message)
                    }
                }
            }
            None => {
                log::error!("{}", message::LOAD_TASK_STOPPED);
                StatusMessage::err(message::LOAD_TASK_STOPPED)
            }
        };

        match &mut self.state {
            State::Read(p) => p.set_status(status),
            State::Settings(_) => self.set_settings_status(status),
            State::Logs(l) => l.previous.set_status(status),
            State::Diff(d) => d.previous.set_status(status),
            State::Library(l) => l.previous.set_status(status),
        }
    }

    fn cycle_target(&self) -> Option<PathBuf> {
        let next = self.config.next_config.trim();
        if !next.is_empty() {
            Some(PathBuf::from(next))
        } else if self.config_path != self.origin_config_path {
            Some(self.origin_config_path.clone())
        } else {
            None
        }
    }

    pub fn cycle_config(&mut self) {
        let Some(target) = self.cycle_target() else {
            self.set_read_status(StatusMessage::info(message::NO_NEXT_CONFIG));
            return;
        };

        if let Some(target) = self.guard_unsaved_load(target) {
            self.load_config_target(target);
        }
    }

    pub(super) fn guard_unsaved_load(&mut self, target: PathBuf) -> Option<PathBuf> {
        if !self.dirty || self.config.skip_unsaved_warning || target.is_empty() {
            return Some(target);
        }
        self.close_settings();
        self.read_mut().popup = Some(Popup::LoadConfig(target));
        None
    }

    pub fn confirm_load_config(&mut self) {
        let target = self.popup_as::<PathBuf>().cloned();
        self.close_popup();
        if let Some(target) = target {
            self.load_config_target(target);
        }
    }

    fn load_config_target(&mut self, target: PathBuf) {
        let status = self.load_config_from(target);
        self.set_read_status(status);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{App, BackgroundTask, LoadConfigTaskResult};
    use crate::config::Config;
    use crate::state::{ConnectionStatus, ImportSource, MessageKind};
    use std::assert_matches;

    async fn app_with_refresh_in_flight() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.refresh();
        assert_matches!(app.background_task, Some(BackgroundTask::Refresh(_)));
        app.open_settings();
        app
    }

    fn settings_status(app: &App) -> MessageKind {
        app.settings()
            .and_then(|s| s.status.as_ref())
            .map(|s| s.kind)
            .expect("a status is shown")
    }

    async fn dirty_app_loading_from_settings(skip_warning: bool) -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.skip_unsaved_warning = skip_warning;
        app.config.name = "edited".to_string();
        app.refresh_dirty();
        app.open_settings();
        if let Some(s) = app.settings_mut() {
            s.load_path = "missing-config.json".to_string();
        }
        app.settings_load();
        app
    }

    #[tokio::test]
    async fn settings_load_with_unsaved_changes_asks_first() {
        let app = dirty_app_loading_from_settings(false).await;

        assert!(app.settings().is_none());
        assert_eq!(
            app.popup_as::<std::path::PathBuf>(),
            Some(&std::path::PathBuf::from("missing-config.json"))
        );
    }

    #[tokio::test]
    async fn settings_load_with_an_empty_path_errors_without_asking() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.name = "edited".to_string();
        app.refresh_dirty();
        app.open_settings();
        app.settings_load();

        assert_eq!(settings_status(&app), MessageKind::Err);
    }

    #[tokio::test]
    async fn settings_load_skips_the_prompt_when_the_warning_is_off() {
        let app = dirty_app_loading_from_settings(true).await;

        assert_eq!(settings_status(&app), MessageKind::Err);
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
            parsed.registers.inputs.len(),
            Config::demo().registers.inputs.len()
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

        let parsed = App::parse_import(&json).expect("export is importable");
        assert_eq!(parsed.source, ImportSource::Mtui);
        let payload = parsed
            .devices
            .into_iter()
            .next()
            .expect("one device")
            .registers;
        assert_eq!(payload.pins(), 2);
        assert_eq!(payload.labels(), app.labels.len());
        assert_eq!(payload.rules(), app.custom_rules.len());

        let mut other = App::boot(Config::default(), String::new()).await;
        assert!(other.import_text(&json));
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

        assert_matches!(app.background_task, Some(BackgroundTask::Refresh(_)));
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
        assert_matches!(app.background_task, Some(BackgroundTask::LoadConfig(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_failed_connect_still_switches_the_config() {
        let mut app = App::boot(Config::default(), String::new()).await;
        let next = Config {
            name: "next".to_string(),
            ..Config::default()
        };
        let path = std::path::PathBuf::from("next.json");

        app.apply_load_config_result(Some(LoadConfigTaskResult {
            path: path.clone(),
            config: Box::new(next),
            result: Err("Connection refused".to_string()),
        }));

        assert_eq!(app.config.name, "next");
        assert_eq!(app.config_path(), path.as_path());
        assert!(app.device.is_none());
        assert!(app.reconnect.link_lost);
        assert_matches!(app.connection, ConnectionStatus::Error(_));
        assert!(!app.dirty);
    }
}
