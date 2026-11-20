use super::App;

use crate::config::{AddressMode, BatchAnchor, TimeMode};
use crate::interpretator::WIDTH_MAX;
use crate::num_ops::cycle;
use crate::register::RegisterType;
use crate::state::{ReadPanel, SettingsField, SettingsParams, State, StatusMessage};
use crate::tui::theme::{self, Theme};
use ratatui::style::Color;

const fn theme_field(theme: &mut Theme, field: SettingsField) -> Option<&mut Color> {
    Some(match field {
        SettingsField::ThemeBackground => &mut theme.background,
        SettingsField::ThemeBorder => &mut theme.border,
        SettingsField::ThemeAccent => &mut theme.accent,
        SettingsField::ThemeText => &mut theme.text,
        SettingsField::ThemeDim => &mut theme.dim,
        SettingsField::ThemeChanged => &mut theme.changed,
        SettingsField::ThemeTrendUp => &mut theme.trend_up,
        SettingsField::ThemeTrendDown => &mut theme.trend_down,
        SettingsField::ThemeZebra => &mut theme.zebra,
        SettingsField::ThemeOk => &mut theme.ok,
        SettingsField::ThemeWarning => &mut theme.warning,
        SettingsField::ThemeError => &mut theme.error,
        SettingsField::ThemeSelectedText => &mut theme.selected_text,
        SettingsField::ThemeSelectedBackground => &mut theme.selected_background,
        SettingsField::ThemeSeries1 => &mut theme.series_1,
        SettingsField::ThemeSeries2 => &mut theme.series_2,
        SettingsField::ThemeSeries3 => &mut theme.series_3,
        _ => return None,
    })
}

impl App {
    pub const fn settings(&self) -> Option<&SettingsParams> {
        match &self.state {
            State::Settings(s) => Some(s),
            _ => None,
        }
    }

    pub const fn settings_mut(&mut self) -> Option<&mut SettingsParams> {
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
        self.clamp_panel_cursor();
    }

    pub fn set_settings_status(&mut self, message: StatusMessage) {
        if let Some(s) = self.settings_mut() {
            s.status = Some(message);
        }
    }

    pub(super) fn note_cleared(&mut self, n: usize, noun: &str) {
        self.refresh_dirty();
        log::info!("Cleared {n} {noun}(s)");
        self.set_settings_status(StatusMessage::ok(format!("Cleared {n} {noun}(s)")));
    }

    const fn numeric_spec(field: SettingsField) -> Option<(i64, i64, i64)> {
        match field {
            SettingsField::BatchSize | SettingsField::GraphHistory => Some((1, u16::MAX as i64, 1)),
            SettingsField::MatrixColumns
            | SettingsField::StartupAddress
            | SettingsField::ApiPort => Some((0, u16::MAX as i64, 1)),
            SettingsField::PaddingHorizontal | SettingsField::PaddingVertical => Some((0, 50, 1)),
            SettingsField::LabelWidth | SettingsField::CustomWidth => {
                Some((0, WIDTH_MAX as i64, 1))
            }
            SettingsField::RefreshInterval | SettingsField::ChangedExpiry => {
                Some((0, u32::MAX as i64, 100))
            }
            _ => None,
        }
    }

    const fn numeric_get(&self, field: SettingsField) -> i64 {
        match field {
            SettingsField::BatchSize => self.config.batch.size as i64,
            SettingsField::RefreshInterval => self.config.refresh_interval_ms as i64,
            SettingsField::ChangedExpiry => self.config.changed_expiry_ms as i64,
            SettingsField::GraphHistory => self.config.graph.history as i64,
            SettingsField::MatrixColumns => self.config.matrix.columns as i64,
            SettingsField::LabelWidth => self.interpreter.label_width() as i64,
            SettingsField::CustomWidth => self.interpreter.custom_width() as i64,
            SettingsField::StartupAddress => self.config.startup.address as i64,
            SettingsField::PaddingHorizontal => self.config.display.padding.horizontal as i64,
            SettingsField::PaddingVertical => self.config.display.padding.vertical as i64,
            SettingsField::ApiPort => match self.settings() {
                Some(&SettingsParams {
                    api_port_draft: Some(port),
                    ..
                }) => port as i64,
                _ => self.config.api.port as i64,
            },
            _ => 0,
        }
    }

    fn numeric_set(&mut self, field: SettingsField, value: i64) {
        match field {
            SettingsField::BatchSize => self.config.batch.size = value as u16,
            SettingsField::RefreshInterval => self.config.refresh_interval_ms = value.max(0) as u64,
            SettingsField::ChangedExpiry => self.config.changed_expiry_ms = value.max(0) as u64,
            SettingsField::GraphHistory => self.config.graph.history = value as u16,
            SettingsField::MatrixColumns => self.config.matrix.columns = value as u16,
            SettingsField::LabelWidth => {
                self.interpreter.set_label_width(value as u16);
                self.sync_auto_widths();
            }
            SettingsField::CustomWidth => {
                self.interpreter.set_custom_width(value as u16);
                self.sync_auto_widths();
            }
            SettingsField::StartupAddress => self.config.startup.address = value as u16,
            SettingsField::PaddingHorizontal => {
                self.config.display.padding.horizontal = value as u16;
            }
            SettingsField::PaddingVertical => self.config.display.padding.vertical = value as u16,
            SettingsField::ApiPort => {
                if let Some(s) = self.settings_mut() {
                    s.api_port_draft = Some(value as u16);
                }
            }
            _ => {}
        }
    }

    pub const fn settings_field_disabled(&self, field: SettingsField) -> bool {
        match field {
            SettingsField::ClockMillis => !self.config.display.clock,
            _ => field.is_startup() && self.config.save_position_on_exit,
        }
    }

    pub fn settings_adjust(&mut self, field: SettingsField, delta: i64) {
        if self.settings_field_disabled(field) {
            return;
        }
        match field {
            SettingsField::SkipUnsavedWarning => self.config.skip_unsaved_warning.toggle(),
            SettingsField::SavePositionOnExit => self.config.save_position_on_exit.toggle(),
            SettingsField::ShowMockDevice => self.config.display.mock_device.toggle(),
            SettingsField::ReadOnly => self.config.read_only.toggle(),
            SettingsField::WriteWaitsForRead => self.config.write_waits_for_read.toggle(),
            SettingsField::TimeMode => {
                let next = cycle(&TimeMode::ALL, self.interpreter.time_mode(), delta > 0);
                self.interpreter.set_time_mode(next);
            }
            SettingsField::AddressMode => {
                let next = cycle(
                    &AddressMode::ALL,
                    self.interpreter.address_mode(),
                    delta > 0,
                );
                self.interpreter.set_address_mode(next);
            }
            SettingsField::BatchAnchor => {
                self.config.batch.anchor =
                    cycle(&BatchAnchor::ALL, self.config.batch.anchor, delta > 0);
            }
            SettingsField::ReadFullCustoms => self.config.batch.read_full_customs.toggle(),
            SettingsField::CustomBatchByRegisters => self.config.batch.custom_by_registers.toggle(),
            SettingsField::FilterPanelsByType => self.config.filter_panels_by_type.toggle(),
            SettingsField::ApiEnabled => self.config.api.enabled.toggle(),
            SettingsField::ApiUnitIdOverride => self.config.api.unit_id_override.toggle(),
            SettingsField::ApiRawFunction => self.config.api.raw_function.toggle(),
            SettingsField::ApiMcp => self.config.api.mcp.toggle(),
            SettingsField::WriteLogEnabled => self.config.write_log.enabled.toggle(),
            SettingsField::ReconnectOnTimeout => self.config.reconnect_on_timeout.toggle(),
            SettingsField::RawExceptions => {
                self.config.raw_exceptions.toggle();
                self.sync_raw_exceptions();
            }
            SettingsField::ShowRuleContinuation => self.config.display.rule_continuation.toggle(),
            SettingsField::ChangedFade => self.config.display.changed_fade.toggle(),
            SettingsField::SelectedAddressOnly => {
                self.config.display.selected_address_only.toggle();
            }
            SettingsField::ChangedBits => self.config.display.changed_bits.toggle(),
            SettingsField::ChangedTrend => self.config.display.changed_trend.toggle(),
            SettingsField::ChangedTrendColor => self.config.display.changed_trend_color.toggle(),
            SettingsField::ChangedTrendCustom => self.config.display.changed_trend_custom.toggle(),
            SettingsField::ShowClock => self.config.display.clock.toggle(),
            SettingsField::ClockMillis => self.config.display.clock_millis.toggle(),
            SettingsField::ShowFrameTime => self.config.display.frame_time.toggle(),
            SettingsField::ShowRam => self.config.display.ram.toggle(),
            SettingsField::ShowConnectionLabel => self.config.display.connection_label.toggle(),
            SettingsField::ShowAsciiStrip => self.config.display.ascii_strip.toggle(),
            SettingsField::ShowInactiveTabs => self.config.display.inactive_tabs.toggle(),
            SettingsField::ShowMatrixContext => self.config.matrix.show_context.toggle(),
            SettingsField::ShowReadWindow => self.config.display.read_window.toggle(),
            SettingsField::GraphTimeAxis => self.config.graph.time_axis.toggle(),
            SettingsField::StartupPanel => {
                self.config.startup.panel =
                    cycle(&ReadPanel::ALL, self.config.startup.panel, delta > 0);
            }
            SettingsField::StartupType => {
                self.config.startup.register_type = cycle(
                    &RegisterType::ALL,
                    self.config.startup.register_type,
                    delta > 0,
                );
            }
            SettingsField::CycleHoldings
            | SettingsField::CycleInputs
            | SettingsField::CycleCoils
            | SettingsField::CycleDiscretes => {
                if let Some(rt) = field.cycle_register_type() {
                    self.config.cycle_register_types.toggle(rt);
                }
            }
            SettingsField::LibraryMbmap
            | SettingsField::LibrarySunSpec
            | SettingsField::LibrarySolarman => {
                if let Some(source) = field.library_source() {
                    self.config.libraries.toggle(source);
                }
            }
            SettingsField::CyclePinned
            | SettingsField::CycleLabeled
            | SettingsField::CycleCustom
            | SettingsField::CycleMatrix => {
                if let Some(panel) = field.cycle_panel() {
                    self.config.cycle_panels.toggle(panel);
                }
            }
            SettingsField::ThemePreset => {
                let presets = Theme::PRESETS;
                let index = match presets.iter().position(|&(_, t)| t == self.config.theme) {
                    Some(i) if delta > 0 => (i + 1) % presets.len(),
                    Some(i) => (i + presets.len() - 1) % presets.len(),
                    None if delta > 0 => 0,
                    None => presets.len() - 1,
                };
                self.config.theme = presets[index].1;
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
        self.sync_api_allow_unit_id();
        self.sync_api_allow_raw();
        self.sync_api_allow_mcp();
        self.refresh_dirty();
    }

    pub fn settings_digit(&mut self, field: SettingsField, digit: u8) {
        if self.settings_field_disabled(field) {
            return;
        }
        if field.is_theme_color() {
            if let Some(slot) = theme_field(&mut self.config.theme, field) {
                let current = match *slot {
                    Color::Indexed(n) => n as i64,
                    _ => 0,
                };
                let value = (current * 10 + digit as i64).clamp(0, 255) as u8;
                *slot = Color::Indexed(value);
                self.refresh_dirty();
            }
            return;
        }
        let Some((min, max, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = (self.numeric_get(field) * 10 + digit as i64).clamp(min, max);
        self.numeric_set(field, value);
        self.refresh_dirty();
    }

    pub fn settings_address_digit(&mut self, c: char) {
        if self.settings_field_disabled(SettingsField::StartupAddress) {
            return;
        }
        let mode = self.interpreter.address_mode();
        if let Some(next) = mode.push_digit(self.config.startup.address, c) {
            self.config.startup.address = next;
            self.refresh_dirty();
        }
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
                self.refresh_dirty();
            }
            SettingsField::NextConfig => {
                self.config.next_config.push(c);
                self.refresh_dirty();
            }
            SettingsField::WriteLogDirectory => {
                self.config.write_log.directory.push(c);
                self.refresh_writes_log_state();
                self.refresh_dirty();
            }
            SettingsField::ApiHost if c.is_ascii_hexdigit() || c == '.' || c == ':' => {
                self.edit_api_host_draft(|draft| draft.push(c));
            }
            _ => {}
        }
    }

    pub fn settings_backspace(&mut self, field: SettingsField) {
        if self.settings_field_disabled(field) {
            return;
        }
        if field == SettingsField::LoadConfig {
            if let Some(s) = self.settings_mut() {
                s.load_path.pop();
            }
            return;
        }
        if field == SettingsField::Name {
            self.config.name.pop();
            self.refresh_dirty();
            return;
        }
        if field == SettingsField::NextConfig {
            self.config.next_config.pop();
            self.refresh_dirty();
            return;
        }
        if field == SettingsField::WriteLogDirectory {
            self.config.write_log.directory.pop();
            self.refresh_writes_log_state();
            self.refresh_dirty();
            return;
        }
        if field == SettingsField::ApiHost {
            self.edit_api_host_draft(|draft| {
                draft.pop();
            });
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
                self.refresh_dirty();
            }
            return;
        }
        if field == SettingsField::StartupAddress {
            let mode = self.interpreter.address_mode();
            self.config.startup.address = mode.pop_digit(self.config.startup.address);
            self.refresh_dirty();
            return;
        }
        let Some((min, _, _)) = Self::numeric_spec(field) else {
            return;
        };
        let value = self.numeric_get(field);
        self.numeric_set(field, if value >= 10 { value / 10 } else { min });
        self.refresh_dirty();
    }

    pub fn settings_save(&mut self) {
        let result = self.persist_config();
        match &result {
            Ok(_) => log::info!("Configuration saved"),
            Err(error) => log::error!("Save failed | {error}"),
        }
        self.set_settings_status(result.into());
    }

    pub fn settings_load(&mut self) {
        let Some(path) = self.settings().map(|s| s.load_path.trim().to_string()) else {
            return;
        };
        if let Some(path) = self.guard_unsaved_load(std::path::PathBuf::from(path)) {
            let status = self.load_config_from(path);
            self.set_settings_status(status);
        }
    }
}
