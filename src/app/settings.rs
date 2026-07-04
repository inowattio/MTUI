use super::App;

use crate::num_ops::cycle;
use crate::state::{ReadPanel, SettingsField, SettingsParams, State, StatusMessage};
use crate::tui::theme::{self, Theme};
use ratatui::style::Color;

fn theme_field(theme: &mut Theme, field: SettingsField) -> Option<&mut Color> {
    Some(match field {
        SettingsField::ThemeBorder => &mut theme.border,
        SettingsField::ThemeAccent => &mut theme.accent,
        SettingsField::ThemeText => &mut theme.text,
        SettingsField::ThemeDim => &mut theme.dim,
        SettingsField::ThemeChanged => &mut theme.changed,
        SettingsField::ThemeZebra => &mut theme.zebra,
        SettingsField::ThemeOk => &mut theme.ok,
        SettingsField::ThemeWarn => &mut theme.warn,
        SettingsField::ThemeErr => &mut theme.err,
        SettingsField::ThemeSelectedFg => &mut theme.selected_fg,
        _ => return None,
    })
}

impl App {
    pub fn settings(&self) -> Option<&SettingsParams> {
        match &self.state {
            State::Settings(s) => Some(s),
            _ => None,
        }
    }

    pub fn settings_mut(&mut self) -> Option<&mut SettingsParams> {
        match &mut self.state {
            State::Settings(s) => Some(s),
            _ => None,
        }
    }

    pub fn open_settings(&mut self) {
        let previous = std::mem::take(self.read_mut());
        self.state = State::Settings(SettingsParams {
            previous,
            ..Default::default()
        });
    }

    pub fn close_settings(&mut self) {
        let mut previous = match &mut self.state {
            State::Settings(s) => std::mem::take(&mut s.previous),
            _ => return,
        };
        previous.loading = false;
        self.state = State::Read(previous);
    }

    pub(super) fn set_settings_status(&mut self, message: StatusMessage) {
        if let Some(s) = self.settings_mut() {
            s.status = Some(message);
        }
    }

    fn numeric_spec(field: SettingsField) -> Option<(i64, i64, i64)> {
        match field {
            SettingsField::RegistersBatch
            | SettingsField::HistoryCap
            | SettingsField::MatrixCols => Some((1, u16::MAX as i64, 1)),
            SettingsField::AutoUpdate => Some((0, u32::MAX as i64, 100)),
            SettingsField::ApiPort => Some((-1, u16::MAX as i64, 1)),
            _ => None,
        }
    }

    fn numeric_get(&self, field: SettingsField) -> i64 {
        match field {
            SettingsField::RegistersBatch => self.config.registers_batch as i64,
            SettingsField::AutoUpdate => self.config.update_interval_ms.map_or(0, |n| n as i64),
            SettingsField::HistoryCap => self.config.graph_history_cap as i64,
            SettingsField::MatrixCols => self.config.matrix_cols as i64,
            SettingsField::ApiPort => self.config.port.map_or(-1, |p| p as i64),
            _ => 0,
        }
    }

    fn numeric_set(&mut self, field: SettingsField, value: i64) {
        match field {
            SettingsField::RegistersBatch => self.config.registers_batch = value as u16,
            SettingsField::AutoUpdate => {
                self.config.update_interval_ms = (value > 0).then_some(value as u64)
            }
            SettingsField::HistoryCap => self.config.graph_history_cap = value as u16,
            SettingsField::MatrixCols => self.config.matrix_cols = value as u16,
            SettingsField::ApiPort => self.config.port = (value >= 0).then_some(value as u16),
            _ => {}
        }
    }

    pub fn settings_adjust(&mut self, field: SettingsField, delta: i64) {
        match field {
            SettingsField::IgnoreDirty => self.config.ignore_dirty = !self.config.ignore_dirty,
            SettingsField::ReadOnly => self.config.read_only = !self.config.read_only,
            SettingsField::ApiSlaveOverride => {
                self.config.allow_api_slave_id = !self.config.allow_api_slave_id
            }
            SettingsField::LogWrites => self.config.log_writes = !self.config.log_writes,
            SettingsField::ShowContinuation => {
                self.config.custom_rules.show_continuation =
                    !self.config.custom_rules.show_continuation
            }
            SettingsField::ShowClock => self.config.show_clock = !self.config.show_clock,
            SettingsField::ShowFrameTime => {
                self.config.show_frame_time = !self.config.show_frame_time
            }
            SettingsField::StartupPanel => {
                self.config.startup.panel =
                    cycle(&ReadPanel::ALL, self.config.startup.panel, delta > 0);
            }
            SettingsField::CycleHoldings
            | SettingsField::CycleInputs
            | SettingsField::CycleCoils
            | SettingsField::CycleDiscretes => {
                let rt = field.cycle_register_type().expect("cycle field");
                self.config.cycle_types.toggle(rt);
            }
            f if f.is_theme_color() => {
                if let Some(slot) = theme_field(&mut self.config.theme, f) {
                    *slot = cycle(theme::PALETTE, *slot, delta > 0);
                }
            }
            _ => {
                let Some((min, max, step)) = Self::numeric_spec(field) else {
                    return;
                };
                let value = (self.numeric_get(field) + delta * step).clamp(min, max);
                self.numeric_set(field, value);
            }
        }
        self.refresh_writes_log_state();
        self.sync_api_read_only();
        self.sync_api_allow_slave_id();
        self.dirty = true;
    }

    pub fn settings_digit(&mut self, field: SettingsField, digit: u8) {
        if field.is_theme_color() {
            if let Some(slot) = theme_field(&mut self.config.theme, field) {
                let current = match *slot {
                    Color::Indexed(n) => n as i64,
                    _ => 0,
                };
                let value = (current * 10 + digit as i64).clamp(0, 255) as u8;
                *slot = Color::Indexed(value);
                self.dirty = true;
            }
            return;
        }
        let Some((min, max, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = (self.numeric_get(field).max(0) * 10 + digit as i64).clamp(min, max);
        self.numeric_set(field, value);
        self.dirty = true;
    }

    pub fn settings_text_input(&mut self, field: SettingsField, c: char) {
        match field {
            SettingsField::LoadConfig => {
                if let Some(s) = self.settings_mut() {
                    s.load_path.push(c);
                }
            }
            SettingsField::Name => {
                self.config.name.push(c);
                self.dirty = true;
            }
            _ => {}
        }
    }

    pub fn settings_backspace(&mut self, field: SettingsField) {
        if field == SettingsField::LoadConfig {
            if let Some(s) = self.settings_mut() {
                s.load_path.pop();
            }
            return;
        }
        if field == SettingsField::Name {
            self.config.name.pop();
            self.dirty = true;
            return;
        }
        if field.is_theme_color() {
            let default = theme_field(&mut Theme::default(), field).copied();
            if let Some(slot) = theme_field(&mut self.config.theme, field) {
                match *slot {
                    Color::Indexed(n) if n > 0 => *slot = Color::Indexed(n / 10),
                    _ => {
                        if let Some(def) = default {
                            *slot = def;
                        }
                    }
                }
                self.dirty = true;
            }
            return;
        }
        let Some((min, _, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = self.numeric_get(field);
        self.numeric_set(field, if value >= 10 { value / 10 } else { min });
        self.dirty = true;
    }

    pub fn settings_save(&mut self) {
        let result = self.persist_config();
        match &result {
            Ok(_) => {
                self.dirty = false;
                log::info!("Configuration saved");
            }
            Err(error) => log::error!("Save failed \u{b7} {error}"),
        }
        self.set_settings_status(result.into());
    }

    pub fn settings_load(&mut self) {
        let Some(path) = self.settings().map(|s| s.load_path.trim().to_string()) else {
            return;
        };
        if !self.free_background_slot() {
            self.set_settings_status(StatusMessage::info("Device is busy."));
            return;
        }
        match self.start_config_load(path) {
            Ok(()) => self.set_settings_status(StatusMessage::info("Loading\u{2026}")),
            Err(error) => {
                log::error!("{error}");
                self.set_settings_status(StatusMessage::err(error));
            }
        }
    }
}
