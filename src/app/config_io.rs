use super::{App, BackgroundTask, LoadConfigTaskResult, file_name, save_config};
use crate::compat;
use crate::config::{Config, Registers, Startup};
use crate::constants::message;
use crate::export;
use crate::import::mbmap;
use crate::import::{Device, home_assistant, solarman};
use crate::modbus::ModbusDevice;
use crate::num_ops::wrap_index;
use crate::state::{
    ConfigImport, ConfigImportAction, ConnectionStatus, ExportFormat, ExportTarget, FirmwareInput,
    ImportDevice, ImportParams, ImportSource, Outcome, Popup, State, StatusMessage,
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
    config: Option<Box<Config>>,
}

fn parse_whole_config(data: &str) -> Option<Config> {
    let value: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
    let fields = value.as_object()?;
    if !fields.contains_key("registers") || fields.len() < 2 {
        return None;
    }
    serde_json::from_value(value).ok()
}

fn config_file_name(input: &str) -> Option<String> {
    let input = input.trim();
    let stem = input
        .len()
        .checked_sub(".json".len())
        .filter(|&end| input.is_char_boundary(end) && input[end..].eq_ignore_ascii_case(".json"))
        .map_or(input, |end| &input[..end]);
    let name = file_name(&[stem], "json");
    (name != ".json").then_some(name)
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
        if let Some(config) = parse_whole_config(data) {
            self.open_config_import(config);
            return true;
        }
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
            config: None,
        };
        self.pending_import = Some(PendingImport {
            devices: devices.into_iter().map(|device| device.registers).collect(),
            map: map
                .filter(|_| params.firmware.is_some())
                .map(str::to_string),
            config: None,
        });
        self.read_mut().popup = Some(Popup::Import(params));
    }

    fn open_config_import(&mut self, config: Config) {
        let device = Device::from(config.registers.clone());
        self.open_import(vec![device], ImportSource::Mtui, None);
        let folder = self.config_folder();
        let folder = std::path::absolute(folder)
            .unwrap_or_else(|_| folder.to_path_buf())
            .display()
            .to_string();
        let import = ConfigImport {
            name: config.name.clone(),
            action: ConfigImportAction::Registers,
            file_name: file_name(&[&config.name], "json")
                .trim_end_matches(".json")
                .to_string(),
            folder,
            unsaved: self.dirty,
            error: None,
        };
        if let Some(p) = self.popup_as_mut::<ImportParams>() {
            p.config = Some(import);
        }
        if let Some(pending) = &mut self.pending_import {
            pending.config = Some(Box::new(config));
        }
    }

    fn config_folder(&self) -> &Path {
        self.config_path
            .parent()
            .filter(|parent| !parent.is_empty())
            .unwrap_or_else(|| Path::new("."))
    }

    pub fn editing_import_text(&self) -> bool {
        self.popup_as::<ImportParams>()
            .is_some_and(|p| p.firmware.is_some() || p.action() == ConfigImportAction::Save)
    }

    pub fn import_input(&mut self, c: char) {
        let Some(p) = self.popup_as_mut::<ImportParams>() else {
            return;
        };
        if let Some(config) = p
            .config
            .as_mut()
            .filter(|c| c.action == ConfigImportAction::Save)
        {
            config.file_name.push(c);
            config.error = None;
        } else if c.is_ascii_digit() || c == '.' {
            self.import_firmware_input(c);
        }
    }

    pub fn import_backspace(&mut self) {
        let Some(p) = self.popup_as_mut::<ImportParams>() else {
            return;
        };
        if let Some(config) = p
            .config
            .as_mut()
            .filter(|c| c.action == ConfigImportAction::Save)
        {
            config.file_name.pop();
            config.error = None;
        } else {
            self.import_firmware_backspace();
        }
    }

    fn import_firmware_input(&mut self, c: char) {
        if let Some(firmware) = self
            .popup_as_mut::<ImportParams>()
            .and_then(|p| p.firmware.as_mut())
        {
            firmware.text.push(c);
            self.refilter_import();
        }
    }

    fn import_firmware_backspace(&mut self) {
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
        let Some(p) = self.popup_as_mut::<ImportParams>() else {
            return;
        };
        if let Some(config) = p.config.as_mut() {
            let actions = &ConfigImportAction::ALL;
            let current = actions
                .iter()
                .position(|&a| a == config.action)
                .unwrap_or(0);
            let next = wrap_index(current as u16, actions.len() as u16, down);
            config.action = actions[usize::from(next)];
            config.error = None;
        } else {
            p.selected = wrap_index(p.selected, p.devices.len() as u16, down);
        }
    }

    pub fn cancel_import(&mut self) {
        self.pending_import = None;
        self.close_popup();
    }

    pub fn apply_import(&mut self) {
        match self.popup_as::<ImportParams>().map(ImportParams::action) {
            Some(ConfigImportAction::Load) => self.load_pasted_config(),
            Some(ConfigImportAction::Save) => self.save_pasted_config(),
            _ => self.import_registers(),
        }
    }

    fn import_registers(&mut self) {
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

    fn take_pasted_config(&mut self) -> Option<Config> {
        let config = self.pending_import.take()?.config?;
        Some(*config)
    }

    fn load_pasted_config(&mut self) {
        let config = self.take_pasted_config();
        self.close_popup();
        let Some(config) = config else {
            return;
        };
        if !self.free_background_slot() {
            self.set_read_status(StatusMessage::info(message::DEVICE_BUSY));
            return;
        }
        self.spawn_config_load(None, config);
        self.set_read_status(StatusMessage::info(message::LOADING));
    }

    fn save_pasted_config(&mut self) {
        let Some(input) = self
            .popup_as::<ImportParams>()
            .and_then(|p| p.config.as_ref())
            .map(|c| c.file_name.clone())
        else {
            return;
        };
        let result = match config_file_name(&input) {
            None => Err(message::SAVE_NEEDS_FILE_NAME.to_string()),
            Some(name) => self.write_pasted_config(&self.config_folder().join(name)),
        };
        match result {
            Ok(path) => {
                self.pending_import = None;
                self.close_popup();
                let message = format!("Saved pasted config to {}", path.display());
                log::info!("{message}");
                self.set_read_status(StatusMessage::ok(message));
            }
            Err(error) => {
                log::warn!("{error}");
                if let Some(config) = self
                    .popup_as_mut::<ImportParams>()
                    .and_then(|p| p.config.as_mut())
                {
                    config.error = Some(error);
                }
            }
        }
    }

    fn write_pasted_config(&self, path: &Path) -> Result<PathBuf, String> {
        if path.exists() {
            return Err(format!("Save failed: {} already exists", path.display()));
        }
        let config = self
            .pending_import
            .as_ref()
            .and_then(|pending| pending.config.as_deref())
            .ok_or_else(|| "Save failed: nothing to save".to_string())?;
        save_config(path, config).map_err(|e| format!("Save failed: {e}"))?;
        Ok(path.to_path_buf())
    }

    fn export_payload_json(&self) -> String {
        serde_json::to_string_pretty(&self.registers()).unwrap_or_default()
    }

    fn config_json(&self) -> String {
        let mut config = self.effective_config();
        if let Some(startup) = self.current_position() {
            config.startup = startup;
        }
        serde_json::to_string_pretty(&config).unwrap_or_default()
    }

    fn export_text(&self, format: ExportFormat) -> String {
        match format {
            ExportFormat::Registers => self.export_payload_json(),
            ExportFormat::Config => self.config_json(),
            ExportFormat::HomeAssistant => export::home_assistant::export(&self.effective_config()),
            ExportFormat::Mbmap => export::mbmap::export(&self.effective_config()),
        }
    }

    fn export_subject(&self, format: ExportFormat) -> String {
        match format {
            ExportFormat::Registers => format!(
                "{} pin(s), {} label(s), {} rule(s)",
                self.pinned_registers.len(),
                self.labels.len(),
                self.custom_rules.len()
            ),
            ExportFormat::Config => "the configuration".to_string(),
            ExportFormat::HomeAssistant | ExportFormat::Mbmap => {
                format!("the {} map", format.label())
            }
        }
    }

    pub fn export(&mut self) {
        let Some((format, target)) = self.settings().map(|s| (s.export_format, s.export_target))
        else {
            return;
        };
        let text = self.export_text(format);
        let subject = self.export_subject(format);
        let message = match target {
            ExportTarget::Clipboard => {
                if self.set_clipboard(text) {
                    StatusMessage::ok(format!("Exported {subject} to clipboard"))
                } else {
                    StatusMessage::err(message::CLIPBOARD_UNAVAILABLE)
                }
            }
            ExportTarget::File => {
                let date = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
                let path = file_name(
                    &["export", &self.config.name, format.file_tag(), &date],
                    format.extension(),
                );
                match fs::write(&path, text) {
                    Ok(()) => StatusMessage::ok(format!("Exported {subject} to {path}")),
                    Err(e) => StatusMessage::err(format!("Export failed: {e}")),
                }
            }
        };
        log::info!("{}", message.text);
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

    pub(super) fn persist_library_disclaimer(&mut self) {
        let mut config: Config = match serde_json::from_str(&self.saved_config) {
            Ok(config) => config,
            Err(error) => {
                log::error!("Library disclaimer acceptance not saved | {error}");
                return;
            }
        };
        config.libraries.disclaimer_accepted = true;
        match save_config(&self.config_path, &config) {
            Ok(()) => {
                self.saved_config = Self::serialize_config(&config);
                log::info!("Library disclaimer accepted");
            }
            Err(error) => log::error!("Library disclaimer acceptance not saved | {error}"),
        }
        self.refresh_dirty();
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
        self.spawn_config_load(Some(path), config);
        StatusMessage::info(message::LOADING)
    }

    fn spawn_config_load(&mut self, path: Option<PathBuf>, config: Config) {
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

                let source = match path {
                    Some(path) => {
                        self.config_path = path;
                        self.mark_config_saved();
                        self.config_path.display().to_string()
                    }
                    None => {
                        self.refresh_dirty();
                        "pasted config".to_string()
                    }
                };

                *self.read_or_previous_mut() = self.startup_read_params();
                self.rediff();

                match error {
                    None => {
                        let message = format!("Loaded {source}");
                        log::info!("{message}");
                        StatusMessage::ok(message)
                    }
                    Some(e) => {
                        self.connection = ConnectionStatus::Error(e.clone());
                        self.logged_connection = self.connection.clone();
                        self.reconnect.link_lost = true;
                        let message = format!("Loaded {source}, device: {e}");
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

#[cfg(test)]
mod pasted_config_tests {
    use super::{config_file_name, parse_whole_config};

    #[test]
    fn only_a_config_with_more_than_registers_is_whole() {
        let registers = r#"{"registers":{"holdings":[{"address":1,"pinned":true}]}}"#;
        assert!(parse_whole_config(registers).is_none());
        assert!(parse_whole_config(r#"{"holdings":[{"address":1,"label":"a"}]}"#).is_none());
        assert!(parse_whole_config("[]").is_none());

        let whole = r#"{"version":1,"name":"site","registers":{}}"#;
        assert_eq!(
            parse_whole_config(whole).map(|c| c.name),
            Some("site".to_string())
        );
    }

    #[test]
    fn config_file_names_are_sanitized_with_one_json_extension() {
        assert_eq!(config_file_name("site"), Some("site.json".to_string()));
        assert_eq!(
            config_file_name(" site.JSON "),
            Some("site.json".to_string())
        );
        assert_eq!(
            config_file_name("my site/2"),
            Some("my_site_2.json".to_string())
        );
        assert_eq!(config_file_name(""), None);
        assert_eq!(config_file_name(".json"), None);
        assert_eq!(config_file_name("//"), None);
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
            path: Some(path.clone()),
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
