use super::{save_config, App, BackgroundTask, ImportPayload, LoadConfigTaskResult};
use crate::compat;
use crate::config::{Config, CustomRules, Startup};
use crate::custom::CustomRule;
use crate::modbus::ModbusDevice;
use crate::register::RegisterCell;
use crate::state::{DumpParams, ImportParams, Outcome, Popup, State, StatusMessage};
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

        self.dirty = true;
        self.close_popup();
        log::info!("Imported {pins} pin(s), {labels} label(s), {rules} rule(s) from clipboard");
        self.set_read_status(StatusMessage::ok(format!(
            "Imported {pins} pin(s), {labels} label(s), {rules} rule(s)"
        )));
    }

    pub(super) fn persist_config(&mut self) -> Outcome {
        self.config.labels = (&self.labels).into();

        let rebuilt: CustomRules = (&self.custom_rules).into();
        self.config.custom_rules.holdings = rebuilt.holdings;
        self.config.custom_rules.inputs = rebuilt.inputs;
        self.config.custom_rules.coils = rebuilt.coils;
        self.config.custom_rules.discretes = rebuilt.discretes;

        self.config.pinned_registers = self.pinned_registers.as_slice().into();

        self.config.interpretations = self.interpreter.config();
        if let State::Read(p) = &self.state {
            self.config.startup = Startup {
                address: p.position,
                register_type: p.register_type,
                panel: p.panel,
            };
        }

        save_config(&self.config_path, &self.config)
            .map(|()| format!("Saved to {}", self.config_path))
            .map_err(|e| format!("Save failed: {e}"))
    }

    pub fn config_path(&self) -> &str {
        &self.config_path
    }

    pub(super) fn start_config_load(&mut self, path: String) -> Result<(), String> {
        if path.is_empty() {
            return Err("Load failed: enter a file name".to_string());
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("Load failed: {e}"))?;
        let config: Config =
            serde_json::from_str(&content).map_err(|e| format!("Load failed: {e}"))?;

        let device_config = config.device.clone();
        self.background_task = Some(BackgroundTask::LoadConfig(compat::spawn(async move {
            let result = ModbusDevice::new(&device_config)
                .await
                .map_err(|e| e.to_string());
            LoadConfigTaskResult {
                path,
                config: Box::new(config),
                result,
            }
        })));
        Ok(())
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
                    self.dirty = true;

                    let read = self.startup_read_params();
                    if let Some(s) = self.settings_mut() {
                        s.previous = read;
                    }

                    Ok(format!("Loaded {path}"))
                }
                Err(e) => Err(format!("Load failed: device: {e}")),
            },
            None => Err("Load failed: task stopped unexpectedly".to_string()),
        };

        match &outcome {
            Ok(message) => log::info!("{message}"),
            Err(error) => log::error!("{error}"),
        }
        self.set_settings_status(outcome.into());
    }

    pub fn commit_dump(&mut self) {
        let result = self.dump_read_log();
        if let Some(d) = self.popup_as_mut::<DumpParams>() {
            d.result = Some(result);
        }
    }
}
