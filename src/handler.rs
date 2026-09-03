use crate::app::App;
use crate::config::{KeybindAction, Keybinds};
use crate::input::{KeyCode, KeyEvent};
use crate::modbus::{DataBits, Parity, StopBits, WordOrder};
use crate::num_ops::{cycle, digit_add, digit_remove, wrap_index};
use crate::state::{
    CustomParams, DiscoveryField, DiscoveryParams, InterfaceKind, LogsParams, PopupKind, ReadPanel,
    SettingsCategory, SettingsField, SettingsFocus, SlaveField, SlaveParams, SweepConfigParams,
    SweepField,
};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) {
    if key_event.is_ctrl_c() {
        app.interrupt();
        return;
    }

    if key_event.ctrl && matches!(key_event.code, KeyCode::Char(_)) {
        return;
    }

    let rows = app.visible_rows.get();
    let kb = app.config.keybinds;

    if app.settings().is_some() {
        handle_settings_key(key_event, app).await;
        return;
    }

    if app.log_view().is_some() {
        handle_logs_view_key(key_event, app);
        return;
    }

    if let Some(kind) = app.popup_kind() {
        handle_popup_key(kind, key_event, app).await;
        return;
    }

    if app.read().graph && key_event.code == kb.dump {
        app.cycle_graph_interpretation();
        return;
    }

    if app.read().graph && key_event.code == kb.pin {
        app.graph_hold_series();
        return;
    }

    if let Some(action) = kb.action_for(key_event.code) {
        run_action(app, action).await;
        return;
    }

    match key_event.code {
        KeyCode::Left | KeyCode::Right if app.read().panel == ReadPanel::Matrix => {
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            p.position = step_pos(p.position, key_event.code == KeyCode::Left, 1);
            p.scroll_to_cursor(rows, cols);
        }
        KeyCode::Left => app.scroll_columns(false),
        KeyCode::Right => app.scroll_columns(true),
        KeyCode::Char(c) => {
            if !c.is_ascii_digit() {
                return;
            }
            let digit = c as u8 - b'0';
            {
                let cols = app.config.matrix_cols;
                let p = app.read_mut();
                digit_add(&mut p.position, digit);
                p.scroll_to_cursor(rows, cols);
            }
        }
        KeyCode::Backspace => {
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            digit_remove(&mut p.position);
            p.scroll_to_cursor(rows, cols);
        }
        _ => {}
    }
}

fn step_pos(value: u16, up: bool, step: u16) -> u16 {
    if up {
        value.saturating_sub(step)
    } else {
        value.saturating_add(step)
    }
}

fn move_read_cursor(app: &mut App, code: KeyCode) {
    let rows = app.visible_rows.get();
    let panel_len = app.panel_len();
    let cols = app.config.matrix_cols;
    let kb = app.config.keybinds;
    let step = if code == kb.page_up || code == kb.page_down {
        rows
    } else {
        1
    };
    let up = code == kb.move_up || code == kb.page_up;
    let scroll_rows = app.panel_scroll_rows();
    let p = app.read_mut();
    match p.panel {
        ReadPanel::Main => {
            p.position = step_pos(p.position, up, step);
            p.scroll_to_cursor(rows, cols);
        }
        ReadPanel::Matrix => {
            let step = step.saturating_mul(cols.max(1));
            p.position = step_pos(p.position, up, step);
            p.scroll_to_cursor(rows, cols);
        }
        _ => {
            p.pinned_index = step_pos(p.pinned_index, up, step);
            p.scroll_pinned(scroll_rows, panel_len);
        }
    }
}

async fn run_action(app: &mut App, action: KeybindAction) {
    use KeybindAction::*;
    match action {
        Exit => app.request_quit(),
        Pin => app.pin(),
        Dump => app.open_dump(),
        Help => app.open_help(),
        About => app.open_about(),
        Refresh | Action => app.refresh().await,
        Toggle => app.toggle_type(),
        Write => app.open_write(),
        Jump => app.open_search(),
        Label => app.open_label(),
        Custom => app.open_custom(),
        Columns => app.open_columns(),
        Pause => app.toggle_pause(),
        WordOrder => app.toggle_word_order(),
        Slave => app.open_slave(),
        CyclePosition => app.cycle_position(),
        Inspect => app.open_inspect(),
        DeviceId => app.open_device_id(),
        Raw => app.open_raw(),
        Graph => app.toggle_graph(),
        Discovery => app.open_discovery(),
        Settings => app.open_settings(),
        CopyAddress => app.copy_address(),
        Logs => app.open_logs(),
        AppLogs => app.open_log_view(),
        Stats => app.open_stats(),
        Sweep => app.open_sweep(),
        Clear => {
            if app.read().graph {
                app.clear_graph_history();
            } else {
                app.clear_session_data();
            }
        }
        NextConfig => app.cycle_config(),
        SwitchView | SwitchViewBack => {
            let rows = app.visible_rows.get();
            app.read_mut().toggle_panel(action == SwitchView);
            let len = app.panel_len();
            let cols = app.config.matrix_cols;
            let scroll_rows = app.panel_scroll_rows();
            let p = app.read_mut();
            p.scroll_pinned(scroll_rows, len);
            p.scroll_to_cursor(rows, cols);
        }
        BatchDecrease | BatchIncrease => app.adjust_batch(action == BatchIncrease),
        MoveUp | MoveDown | PageUp | PageDown => {
            move_read_cursor(app, app.config.keybinds.get(action));
        }
    }
}

async fn handle_popup_key(kind: PopupKind, key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    match kind {
        PopupKind::Discovery => handle_discovery_key(key_event, app).await,

        PopupKind::Help => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => {
                if let Some(action) = app.help_commit() {
                    run_action(app, action).await;
                }
            }
            c if c == kb.move_up => app.help_move(false),
            c if c == kb.move_down => app.help_move(true),
            KeyCode::Backspace => app.help_backspace(),
            KeyCode::Char(c) => app.help_input(c),
            _ => {}
        },

        PopupKind::About => match key_event.code {
            c if c == kb.exit || c == kb.about => app.close_popup(),
            _ => {}
        },

        PopupKind::Stats => match key_event.code {
            c if c == kb.exit || c == kb.stats => app.close_popup(),
            _ => {}
        },

        PopupKind::Inspect => match key_event.code {
            c if c == kb.exit || c == kb.inspect => app.close_popup(),
            c if c == kb.refresh || c == kb.action => app.refresh().await,
            c if c == kb.word_order => app.toggle_word_order(),
            KeyCode::Left => app.inspect_cycle(false),
            KeyCode::Right => app.inspect_cycle(true),
            c if c == kb.move_up || c == kb.move_down || c == kb.page_up || c == kb.page_down => {
                move_read_cursor(app, key_event.code);
            }
            _ => {}
        },

        PopupKind::DeviceId => match key_event.code {
            c if c == kb.exit || c == kb.device_id => app.close_popup(),
            c if c == kb.refresh || c == kb.action => app.device_id_refresh(),
            c if c == kb.switch_view => app.device_id_cycle(true),
            c if c == kb.switch_view_back => app.device_id_cycle(false),
            KeyCode::Left => app.device_id_hscroll(false),
            KeyCode::Right => app.device_id_hscroll(true),
            _ => {}
        },

        PopupKind::Raw => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.raw_send(),
            c if c == kb.move_up => app.raw_move(false),
            c if c == kb.move_down => app.raw_move(true),
            KeyCode::Backspace => app.raw_backspace(),
            KeyCode::Char(c) => app.raw_input(c),
            _ => {}
        },

        PopupKind::Dump => match key_event.code {
            c if c == kb.action => app.commit_dump(),
            c if c == kb.exit || c == kb.dump => app.close_popup(),
            _ => {}
        },

        PopupKind::Columns => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.columns_toggle_selected(),
            c if c == kb.move_up => app.columns_move(false),
            c if c == kb.move_down => app.columns_move(true),
            KeyCode::Left => app.columns_switch(false),
            KeyCode::Right => app.columns_switch(true),
            KeyCode::Backspace => app.columns_backspace(),
            KeyCode::Char(c) => app.columns_input(c),
            _ => {}
        },

        PopupKind::Write => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.commit_write(),
            c if c == kb.write => app.write_toggle_type(),
            c if c == kb.move_up => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(|v| v.checked_sub(1));
                }
                app.clamp_write_value();
            }
            c if c == kb.move_down => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(|v| v.checked_add(1));
                }
                app.clamp_write_value();
            }
            KeyCode::Left => app.write_move_bit(true),
            KeyCode::Right => app.write_move_bit(false),
            c if c == kb.pause => app.write_toggle_bit(),
            KeyCode::Char('-') => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(|v| v.checked_neg());
                }
                app.clamp_write_value();
            }
            KeyCode::Backspace => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.map(|mut n| {
                        digit_remove(&mut n);
                        n
                    });
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let Some(w) = app.write_mut() {
                    let mut n = w.value.unwrap_or_default();
                    digit_add(&mut n, digit);
                    w.value = Some(n);
                }
                app.clamp_write_value();
            }
            _ => {}
        },

        PopupKind::Search => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.search_commit(),
            c if c == kb.move_up => app.search_move(false),
            c if c == kb.move_down => app.search_move(true),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(c) => app.search_input(c),
            _ => {}
        },

        PopupKind::Label => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.commit_label(),
            KeyCode::Backspace => app.label_backspace(),
            KeyCode::Char(c) => app.label_input(c),
            _ => {}
        },

        PopupKind::Custom => {
            let Some(field) = app
                .popup_as::<CustomParams>()
                .map(CustomParams::current_field)
            else {
                return;
            };
            match key_event.code {
                c if c == kb.exit => app.close_popup(),
                c if c == kb.move_up => app.custom_move(false),
                c if c == kb.move_down => app.custom_move(true),
                KeyCode::Left => app.custom_cycle(field, false),
                KeyCode::Right => app.custom_cycle(field, true),
                KeyCode::Delete => app.remove_custom(),
                c if c == kb.action => app.custom_enter(field),
                KeyCode::Backspace => app.custom_backspace(field),
                KeyCode::Char(c) => app.custom_char(field, c),
                _ => {}
            }
        }

        PopupKind::Slave => {
            let Some(field) = app
                .popup_as::<SlaveParams>()
                .map(SlaveParams::current_field)
            else {
                return;
            };
            match key_event.code {
                c if c == kb.exit || c == kb.slave => app.close_popup(),
                c if c == kb.action => match field {
                    SlaveField::Id => app.commit_slave().await,
                    SlaveField::Hit(index) => app.commit_slave_hit(index).await,
                    SlaveField::From | SlaveField::To | SlaveField::Mode | SlaveField::Scan => {
                        app.slave_scan_action()
                    }
                },
                c if c == kb.move_up => app.slave_move(false),
                c if c == kb.move_down => app.slave_move(true),
                c if c == kb.pause && field == SlaveField::Mode => app.slave_scan_toggle(),
                KeyCode::Left | KeyCode::Right if field == SlaveField::Mode => {
                    app.slave_scan_toggle()
                }
                KeyCode::Backspace => app.slave_backspace(field),
                KeyCode::Char(c) if c.is_ascii_digit() => app.slave_digit(field, c),
                _ => {}
            }
        }

        PopupKind::Logs => match key_event.code {
            c if c == kb.exit || c == kb.logs => app.close_popup(),
            c if c == kb.move_up => app.logs_scroll(-1),
            c if c == kb.move_down => app.logs_scroll(1),
            c if c == kb.page_up => app.logs_scroll(-(LogsParams::VISIBLE as i32)),
            c if c == kb.page_down => app.logs_scroll(LogsParams::VISIBLE as i32),
            _ => {}
        },

        PopupKind::SweepConfig => {
            let Some(field) = app
                .popup_as::<SweepConfigParams>()
                .map(SweepConfigParams::current_field)
            else {
                return;
            };
            match key_event.code {
                c if c == kb.exit || c == kb.sweep => app.close_popup(),
                c if c == kb.action => app.sweep_action(),
                c if c == kb.move_up => app.sweep_config_move(false),
                c if c == kb.move_down => app.sweep_config_move(true),
                c if c == kb.pause && field == SweepField::Mode => app.sweep_config_toggle(),
                KeyCode::Left | KeyCode::Right if field == SweepField::Mode => {
                    app.sweep_config_toggle()
                }
                KeyCode::Backspace => app.sweep_config_backspace(field),
                KeyCode::Char(c) if c.is_ascii_digit() => app.sweep_config_digit(field, c),
                _ => {}
            }
        }

        PopupKind::Import | PopupKind::CycleConfig | PopupKind::Quit => {
            type Action = fn(&mut App);
            let (confirm, cancel): (Action, Action) = match kind {
                PopupKind::Import => (App::apply_import, App::cancel_import),
                PopupKind::CycleConfig => (App::confirm_cycle_config, App::close_popup),
                _ => (App::quit, App::close_popup),
            };
            match key_event.code {
                c if c == kb.action => confirm(app),
                c if c == kb.exit || c == KeyCode::Backspace => cancel(app),
                _ => {}
            }
        }
    }
}

pub fn handle_paste(data: String, app: &mut App) {
    if app.settings().is_some() || app.log_view().is_some() {
        return;
    }

    let trimmed = data.trim();
    if trimmed.is_empty() {
        return;
    }

    if let Some(d) = app.discovery_mut() {
        if d.current_field() == DiscoveryField::CustomPath {
            let first_line = trimmed.lines().next().unwrap_or_default();
            d.custom_path.push_str(first_line);
        }
        return;
    }

    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        paste_digits(trimmed, app);
        return;
    }

    if app.popup_kind().is_none() {
        app.paste_import(trimmed);
    }
}

fn paste_digits(digits: &str, app: &mut App) {
    let digits = digits.bytes().map(|b| b - b'0');
    let rows = app.visible_rows.get();

    match app.popup_kind() {
        Some(PopupKind::Write) => {
            if let Some(w) = app.write_mut() {
                let mut n = 0;
                for digit in digits {
                    digit_add(&mut n, digit);
                }

                w.value = Some(n);
            }
            app.clamp_write_value();
        }
        Some(PopupKind::Search) => {
            for digit in digits {
                app.search_input((b'0' + digit) as char);
            }
        }
        None => {
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            p.position = 0;
            for digit in digits {
                digit_add(&mut p.position, digit);
            }
            p.scroll_to_cursor(rows, cols);
        }
        _ => {}
    }
}

async fn handle_discovery_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let Some(field) = app.discovery().map(DiscoveryParams::current_field) else {
        return;
    };

    match key_event.code {
        c if c == kb.exit => app.close_popup(),
        c if c == kb.action => match field {
            DiscoveryField::ScanNetwork => app.start_network_scan(),
            DiscoveryField::Port(index) => app.choose_port(index),
            DiscoveryField::Found(index) => app.use_found_ip(index),
            _ => app.discovery_connect(),
        },
        c if c == kb.move_up || c == kb.move_down => {
            if let Some(d) = app.discovery_mut() {
                d.move_cursor(c == kb.move_down);
            }
        }
        c if c == kb.switch_view || c == kb.switch_view_back => {
            if let Some(d) = app.discovery_mut() {
                d.toggle_column();
            }
        }
        KeyCode::Left | KeyCode::Right => {
            let show_mock = app.config.show_mock;
            if let Some(d) = app.discovery_mut() {
                cycle_field(d, field, key_event.code == KeyCode::Right, show_mock);
            }
        }
        KeyCode::Backspace => {
            if let Some(d) = app.discovery_mut() {
                match field {
                    DiscoveryField::Ip => {
                        d.ip.pop();
                    }
                    DiscoveryField::CustomPath => {
                        d.custom_path.pop();
                    }
                    DiscoveryField::Baud => digit_remove(&mut d.baud_rate),
                    DiscoveryField::NetPort => digit_remove(&mut d.net_port),
                    DiscoveryField::SlaveId => digit_remove(&mut d.slave_id),
                    DiscoveryField::ConnectTimeout => digit_remove(&mut d.connect_timeout_ms),
                    DiscoveryField::CommandTimeout => digit_remove(&mut d.command_timeout_ms),
                    DiscoveryField::BetweenCommands => digit_remove(&mut d.between_commands_ms),
                    _ => {}
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(d) = app.discovery_mut() {
                let digit = (c as u8).saturating_sub(b'0');
                match field {
                    DiscoveryField::Ip if c.is_ascii_digit() || c == '.' => d.ip.push(c),
                    DiscoveryField::CustomPath if !c.is_control() => d.custom_path.push(c),
                    DiscoveryField::Baud if c.is_ascii_digit() => {
                        digit_add(&mut d.baud_rate, digit)
                    }
                    DiscoveryField::NetPort if c.is_ascii_digit() => {
                        digit_add(&mut d.net_port, digit)
                    }
                    DiscoveryField::SlaveId if c.is_ascii_digit() => {
                        digit_add(&mut d.slave_id, digit)
                    }
                    DiscoveryField::ConnectTimeout if c.is_ascii_digit() => {
                        digit_add(&mut d.connect_timeout_ms, digit)
                    }
                    DiscoveryField::CommandTimeout if c.is_ascii_digit() => {
                        digit_add(&mut d.command_timeout_ms, digit)
                    }
                    DiscoveryField::BetweenCommands if c.is_ascii_digit() => {
                        digit_add(&mut d.between_commands_ms, digit)
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn cycle_field(d: &mut DiscoveryParams, field: DiscoveryField, forward: bool, show_mock: bool) {
    let kinds: &[InterfaceKind] = if show_mock {
        &InterfaceKind::ALL
    } else {
        &[InterfaceKind::Wired, InterfaceKind::Network]
    };

    match field {
        DiscoveryField::Interface => d.set_interface(cycle(kinds, d.interface, forward)),
        DiscoveryField::Baud => d.cycle_baud(forward),
        DiscoveryField::DataBits => d.data_bits = cycle(&DataBits::ALL, d.data_bits, forward),
        DiscoveryField::Parity => d.parity = cycle(&Parity::ALL, d.parity, forward),
        DiscoveryField::StopBits => d.stop_bits = cycle(&StopBits::ALL, d.stop_bits, forward),
        DiscoveryField::WordOrder => d.word_order = cycle(&WordOrder::ALL, d.word_order, forward),
        _ => {}
    }
}

fn handle_logs_view_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    match key_event.code {
        c if c == kb.exit || c == kb.app_logs => app.close_log_view(),
        c if c == kb.move_up => app.log_view_scroll(-1),
        c if c == kb.move_down => app.log_view_scroll(1),
        c if c == kb.page_up => app.log_view_scroll(-(app.visible_rows.get() as i32)),
        c if c == kb.page_down => app.log_view_scroll(app.visible_rows.get() as i32),
        c if c == kb.write => app.log_view_toggle_wrap(),
        KeyCode::Left => app.log_view_hscroll(false),
        KeyCode::Right => app.log_view_hscroll(true),
        _ => {}
    }
}

async fn handle_settings_key(key_event: KeyEvent, app: &mut App) {
    match app
        .settings()
        .map_or(SettingsFocus::Categories, |s| s.focus)
    {
        SettingsFocus::Categories => handle_settings_category_key(key_event, app),
        SettingsFocus::Fields
            if app
                .settings()
                .is_some_and(|s| s.current_category().is_keybinds()) =>
        {
            handle_keybinds_key(key_event, app)
        }
        SettingsFocus::Fields => handle_settings_field_key(key_event, app).await,
    }
}

fn handle_settings_category_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = SettingsCategory::ALL.len() as u16;

    match key_event.code {
        c if c == kb.exit || c == kb.settings => app.close_settings(),
        c if c == kb.move_up => {
            if let Some(s) = app.settings_mut() {
                s.category = wrap_index(s.category, count, false);
            }
        }
        c if c == kb.move_down => {
            if let Some(s) = app.settings_mut() {
                s.category = wrap_index(s.category, count, true);
            }
        }
        c if c == kb.action || c == KeyCode::Right => {
            if let Some(s) = app.settings_mut() {
                s.enter_category();
            }
        }
        _ => {}
    }
}

async fn handle_settings_field_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = app
        .settings()
        .map_or(0, |s| s.current_fields().len() as u16);
    let Some(field) = app.settings().and_then(|s| s.current_field()) else {
        return;
    };

    match key_event.code {
        c if c == kb.exit => {
            if let Some(s) = app.settings_mut() {
                s.focus = SettingsFocus::Categories;
            }
        }
        c if c == kb.settings && !field.is_text_input() => app.close_settings(),
        c if c == kb.move_up => {
            if let Some(s) = app.settings_mut() {
                s.field = wrap_index(s.field, count, false);
            }
        }
        c if c == kb.move_down => {
            if let Some(s) = app.settings_mut() {
                s.field = wrap_index(s.field, count, true);
            }
        }
        KeyCode::Left => app.settings_adjust(field, -1),
        KeyCode::Right => app.settings_adjust(field, 1),
        c if c == kb.pause && field.is_toggle() => app.settings_adjust(field, 1),
        c if c == kb.action => match field {
            SettingsField::ClearPins => app.clear_pins(),
            SettingsField::ClearLabels => app.clear_labels(),
            SettingsField::ClearCustom => app.clear_custom(),
            f if f.is_toggle() || f.is_theme_color() => app.settings_adjust(f, 1),
            SettingsField::Save => app.settings_save(),
            SettingsField::LoadConfig => app.settings_load(),
            _ => {}
        },
        KeyCode::Backspace => app.settings_backspace(field),
        KeyCode::Char(c) if field.is_text_input() => app.settings_text_input(field, c),
        KeyCode::Char(c) if c.is_ascii_digit() => app.settings_digit(field, c as u8 - b'0'),
        _ => {}
    }
}

fn handle_keybinds_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = KeybindAction::ALL.len() as u16;
    let selected = app.settings().map_or(0, |s| s.kb_selected) as usize;

    // Capture mode: the next key (other than Esc) becomes the new binding.
    if app.settings().is_some_and(|s| s.kb_capturing) {
        if key_event.code != KeyCode::Esc
            && let Some(&action) = KeybindAction::ALL.get(selected)
        {
            app.config.keybinds.set(action, key_event.code);
            app.dirty = true;
        }
        if let Some(s) = app.settings_mut() {
            s.kb_capturing = false;
        }
        return;
    }

    match key_event.code {
        KeyCode::Esc => {
            if let Some(s) = app.settings_mut() {
                s.focus = SettingsFocus::Categories;
            }
        }
        c if c == kb.move_up => {
            if let Some(s) = app.settings_mut() {
                s.kb_move(true, count);
            }
        }
        c if c == kb.move_down => {
            if let Some(s) = app.settings_mut() {
                s.kb_move(false, count);
            }
        }
        c if c == kb.page_up => {
            if let Some(s) = app.settings_mut() {
                s.kb_page(true, count);
            }
        }
        c if c == kb.page_down => {
            if let Some(s) = app.settings_mut() {
                s.kb_page(false, count);
            }
        }
        c if c == kb.action => {
            if let Some(s) = app.settings_mut() {
                s.kb_capturing = true;
            }
        }
        KeyCode::Backspace => {
            if let Some(&action) = KeybindAction::ALL.get(selected) {
                let default = Keybinds::default().get(action);
                app.config.keybinds.set(action, default);
                app.dirty = true;
            }
        }
        _ => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::handle_key_events;
    use crate::app::App;
    use crate::config::Config;
    use crate::input::{KeyCode, KeyEvent};
    use crate::state::{DiscoveryField, DiscoveryParams, InterfaceKind, Popup, PopupKind, State};

    async fn app() -> App {
        App::boot(Config::default(), String::new()).await
    }

    #[tokio::test]
    async fn the_custom_serial_path_is_typed_and_pasted() {
        let mut app = app().await;
        app.open_discovery();
        {
            let d = app.discovery_mut().unwrap();
            *d = DiscoveryParams {
                ports: vec!["/dev/ttyUSB0".to_string()],
                ..DiscoveryParams::default()
            };
            d.set_interface(InterfaceKind::Wired);
            d.toggle_column();
            d.move_cursor(true); // past the single port row onto the custom path
        }
        assert_eq!(
            app.discovery().unwrap().current_field(),
            DiscoveryField::CustomPath
        );

        for c in "/dev/ttyAMA0".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app).await;
        }
        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app).await;
        super::handle_paste("1\n".to_string(), &mut app);

        let d = app.discovery().unwrap();
        assert_eq!(d.custom_path, "/dev/ttyAMA1");
        assert_eq!(d.serial_path().as_deref(), Some("/dev/ttyAMA1"));
        assert_eq!(app.popup_kind(), Some(PopupKind::Discovery), "still open");
    }

    #[tokio::test]
    async fn the_baud_rate_is_typed_or_stepped() {
        let mut app = app().await;
        app.open_discovery();
        {
            let d = app.discovery_mut().unwrap();

            *d = DiscoveryParams::default();
            d.set_interface(InterfaceKind::Wired);
            d.toggle_column();
            d.move_cursor(true);
        }
        assert_eq!(
            app.discovery().unwrap().current_field(),
            DiscoveryField::Baud
        );

        for _ in 0..4 {
            handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app).await;
        }
        assert_eq!(app.discovery().unwrap().baud_rate, 0, "9600 erased");
        for c in "250000".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app).await;
        }
        assert_eq!(app.discovery().unwrap().baud_rate, 250_000);

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app).await;
        assert_eq!(
            app.discovery().unwrap().baud_rate,
            460_800,
            "next preset above"
        );
        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app).await;
        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app).await;
        assert_eq!(app.discovery().unwrap().baud_rate, 115_200);
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::with_ctrl(KeyCode::Char(c), true)
    }

    #[tokio::test]
    async fn ctrl_c_quits_a_clean_session_at_once() {
        let mut app = app().await;
        handle_key_events(ctrl('c'), &mut app).await;
        assert!(!app.running);
    }

    #[tokio::test]
    async fn ctrl_c_asks_before_discarding_unsaved_changes() {
        let mut app = app().await;
        app.dirty = true;
        handle_key_events(ctrl('c'), &mut app).await;
        assert!(app.running, "unsaved changes must be confirmed first");
        assert_eq!(app.popup_kind(), Some(PopupKind::Quit));

        handle_key_events(ctrl('c'), &mut app).await;
        assert!(!app.running, "a second Ctrl+C confirms");
    }

    #[tokio::test]
    async fn ctrl_c_in_settings_returns_to_the_read_view_to_ask() {
        let mut app = app().await;
        app.open_settings();
        app.dirty = true;
        handle_key_events(ctrl('c'), &mut app).await;
        assert!(app.running);
        assert!(matches!(&app.state, State::Read(p) if p.popup == Some(Popup::Quit)));
    }

    #[tokio::test]
    async fn ignore_dirty_skips_the_prompt() {
        let mut app = app().await;
        app.dirty = true;
        app.config.ignore_dirty = true;
        handle_key_events(ctrl('c'), &mut app).await;
        assert!(!app.running);
    }

    #[tokio::test]
    async fn ctrl_letters_do_not_fire_the_bare_letter_binding() {
        let mut app = app().await;
        handle_key_events(ctrl('w'), &mut app).await;
        assert_eq!(
            app.popup_kind(),
            None,
            "Ctrl+W must not open the write popup"
        );

        handle_key_events(KeyEvent::new(KeyCode::Char('c')), &mut app).await;
        assert_eq!(
            app.popup_kind(),
            Some(PopupKind::Columns),
            "plain c still opens Columns"
        );
    }
}
