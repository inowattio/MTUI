use crate::app::{App, AppResult};
use crate::config::{KeybindAction, Keybinds};
use crate::input::{KeyCode, KeyEvent};
use crate::modbus::{DataBits, Parity, StopBits, WordOrder};
use crate::num_ops::{
    cycle, decrement_option_by, digit_add, digit_add_option, digit_remove, digit_remove_option,
    increment_option_by, negate_opt_option, set_option_to_zero, set_to_zero, wrap_index,
};
use crate::state::{
    DiscoveryField, DiscoveryParams, InterfaceKind, LogsParams, Popup, PopupKind, ReadPanel,
    SettingsField, SweepField,
};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    let rows = app.visible_rows.get();
    let kb = app.config.keybinds;

    if app.discovery().is_some() {
        handle_discovery_key(key_event, app).await;
        return Ok(());
    }

    if app.settings().is_some() {
        handle_settings_key(key_event, app).await;
        return Ok(());
    }

    if app.log_view().is_some() {
        handle_logs_view_key(key_event, app);
        return Ok(());
    }

    if let Some(kind) = app.popup_kind() {
        handle_popup_key(kind, key_event, app).await;
        return Ok(());
    }

    if app.read().graph {
        match key_event.code {
            c if c == kb.dump => {
                app.toggle_graph_width();
                return Ok(());
            }
            c if c == kb.columns || c == kb.slave || c == kb.switch_view || c == kb.toggle => {
                return Ok(())
            }
            _ => {}
        }
    }

    if let Some(action) = kb.action_for(key_event.code) {
        run_action(app, action).await;
        return Ok(());
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
                return Ok(());
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
    Ok(())
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
            p.scroll_pinned(rows, panel_len);
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
        DeviceId => app.open_device_id().await,
        Raw => app.open_raw(),
        Graph => app.toggle_graph(),
        Discovery => app.open_discovery(),
        Settings => app.open_settings(),
        CopyAddress => app.copy_address(),
        Logs => app.open_logs(),
        AppLogs => app.open_log_view(),
        Sweep => app.open_sweep(),
        Clear => app.clear_session_data(),
        SwitchView => {
            let rows = app.visible_rows.get();
            app.read_mut().toggle_panel();
            let len = app.panel_len();
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            p.scroll_pinned(rows, len);
            p.scroll_to_cursor(rows, cols);
        }
        MoveUp | MoveDown | PageUp | PageDown => {
            move_read_cursor(app, app.config.keybinds.get(action));
        }
    }
}

async fn handle_popup_key(kind: PopupKind, key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    match kind {
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

        PopupKind::Inspect => match key_event.code {
            c if c == kb.exit || c == kb.inspect => app.close_popup(),
            c if c == kb.refresh || c == kb.action => app.refresh().await,
            c if c == kb.word_order => app.toggle_word_order(),
            c if c == kb.move_up || c == kb.move_down || c == kb.page_up || c == kb.page_down => {
                move_read_cursor(app, key_event.code);
            }
            _ => {}
        },

        PopupKind::DeviceId => match key_event.code {
            c if c == kb.exit || c == kb.device_id => app.close_popup(),
            c if c == kb.refresh || c == kb.action => app.device_id_refresh().await,
            KeyCode::Left => app.device_id_cycle(false).await,
            KeyCode::Right => app.device_id_cycle(true).await,
            _ => {}
        },

        PopupKind::Raw => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => app.raw_send().await,
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
                    decrement_option_by(&mut w.value, 1);
                }
                app.clamp_write_value();
            }
            c if c == kb.move_down => {
                if let Some(w) = app.write_mut() {
                    increment_option_by(&mut w.value, 1);
                }
                app.clamp_write_value();
            }
            KeyCode::Left => app.write_move_bit(true),
            KeyCode::Right => app.write_move_bit(false),
            c if c == kb.pause => app.write_toggle_bit(),
            KeyCode::Char('-') => {
                if let Some(w) = app.write_mut() {
                    negate_opt_option(&mut w.value);
                }
                app.clamp_write_value();
            }
            KeyCode::Backspace => {
                if let Some(w) = app.write_mut() {
                    digit_remove_option(&mut w.value);
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let Some(w) = app.write_mut() {
                    digit_add_option(&mut w.value, digit);
                }
                app.clamp_write_value();
            }
            _ => {}
        },

        PopupKind::Search => match key_event.code {
            c if c == kb.exit => app.close_popup(),
            c if c == kb.action => {
                let _ = app.search_commit();
            }
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
            let field = match &app.read().popup {
                Some(Popup::Custom(c)) => c.current_field(),
                _ => return,
            };
            match key_event.code {
                c if c == kb.exit => app.close_popup(),
                c if c == kb.move_up => app.custom_move(false),
                c if c == kb.move_down => app.custom_move(true),
                KeyCode::Left => app.custom_cycle(field, false),
                KeyCode::Right => app.custom_cycle(field, true),
                c if c == kb.action => app.custom_enter(field),
                KeyCode::Backspace => app.custom_backspace(field),
                KeyCode::Char(c) => app.custom_char(field, c),
                _ => {}
            }
        }

        PopupKind::Slave => match key_event.code {
            c if c == kb.exit || c == kb.slave => app.close_popup(),
            c if c == kb.action => app.commit_slave().await,
            KeyCode::Backspace => {
                let p = app.read_mut();
                if let Some(Popup::Slave(value)) = &mut p.popup {
                    digit_remove(value);
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                let p = app.read_mut();
                if let Some(Popup::Slave(value)) = &mut p.popup {
                    digit_add(value, digit);
                }
            }
            _ => {}
        },

        PopupKind::Logs => match key_event.code {
            c if c == kb.exit || c == kb.logs => app.close_popup(),
            c if c == kb.move_up => app.logs_scroll(-1),
            c if c == kb.move_down => app.logs_scroll(1),
            c if c == kb.page_up => app.logs_scroll(-(LogsParams::VISIBLE as i32)),
            c if c == kb.page_down => app.logs_scroll(LogsParams::VISIBLE as i32),
            _ => {}
        },

        PopupKind::SweepConfig => {
            let field = match &app.read().popup {
                Some(Popup::SweepConfig(p)) => p.current_field(),
                _ => return,
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

        PopupKind::Import => match key_event.code {
            c if c == kb.action => app.apply_import(),
            c if c == kb.exit => app.cancel_import(),
            KeyCode::Backspace => app.cancel_import(),
            _ => {}
        },

        PopupKind::Quit => match key_event.code {
            c if c == kb.action || c == kb.exit => app.quit(),
            KeyCode::Backspace => app.close_popup(),
            _ => {}
        },
    }
}

pub fn handle_paste(data: String, app: &mut App) {
    if app.discovery().is_some() || app.settings().is_some() || app.log_view().is_some() {
        return;
    }

    let trimmed = data.trim();
    if trimmed.is_empty() {
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
                set_option_to_zero(&mut w.value);
                for digit in digits {
                    digit_add_option(&mut w.value, digit);
                }
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
            set_to_zero(&mut p.position);
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

    if app.discovery().is_some_and(|d| d.scan_open) {
        handle_scan_popup_key(key_event, app, kb);
        return;
    }

    let (field, count) = match app.discovery() {
        Some(d) => (d.current_field(), d.fields().len() as u16),
        None => return,
    };

    match key_event.code {
        c if c == kb.exit => {
            if app.device.is_some() {
                app.return_to_read();
            } else {
                app.quit();
            }
        }
        c if c == kb.action => match field {
            DiscoveryField::ScanNetwork => app.start_network_scan(),
            _ => app.discovery_connect().await,
        },
        c if c == kb.move_up => {
            if let Some(d) = app.discovery_mut() {
                d.selected = wrap_index(d.selected, count, false);
            }
        }
        c if c == kb.move_down => {
            if let Some(d) = app.discovery_mut() {
                d.selected = wrap_index(d.selected, count, true);
            }
        }
        KeyCode::Left => {
            if let Some(d) = app.discovery_mut() {
                cycle_field(d, field, false);
            }
        }
        KeyCode::Right => {
            if let Some(d) = app.discovery_mut() {
                cycle_field(d, field, true);
            }
        }
        KeyCode::Backspace => {
            if let Some(d) = app.discovery_mut() {
                match field {
                    DiscoveryField::Ip => {
                        d.ip.pop();
                    }
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

fn handle_scan_popup_key(key_event: KeyEvent, app: &mut App, kb: Keybinds) {
    let len = app.discovery().map_or(0, |d| d.found.len() as u16);
    match key_event.code {
        c if c == kb.exit => {
            if let Some(d) = app.discovery_mut() {
                d.scan_open = false;
            }
        }
        c if c == kb.action => {
            if len > 0 {
                let selected = app.discovery().map_or(0, |d| d.scan_selected);
                app.use_found_ip(selected);
            }
        }
        c if c == kb.move_up => {
            if let Some(d) = app.discovery_mut() {
                if len > 0 {
                    d.scan_selected = wrap_index(d.scan_selected, len, false);
                }
            }
        }
        c if c == kb.move_down => {
            if let Some(d) = app.discovery_mut() {
                if len > 0 {
                    d.scan_selected = wrap_index(d.scan_selected, len, true);
                }
            }
        }
        _ => {}
    }
}

fn cycle_field(d: &mut DiscoveryParams, field: DiscoveryField, forward: bool) {
    const KINDS: [InterfaceKind; 3] = [
        InterfaceKind::Mock,
        InterfaceKind::Wired,
        InterfaceKind::Network,
    ];
    const BAUDS: [u32; 6] = [9600, 19200, 38400, 57600, 115200, 230400];
    const DATA_BITS: [DataBits; 4] = [
        DataBits::Five,
        DataBits::Six,
        DataBits::Seven,
        DataBits::Eight,
    ];
    const PARITY: [Parity; 3] = [Parity::None, Parity::Odd, Parity::Even];
    const STOP_BITS: [StopBits; 2] = [StopBits::One, StopBits::Two];
    const ORDERS: [WordOrder; 4] = [
        WordOrder::ABCD,
        WordOrder::BADC,
        WordOrder::CDAB,
        WordOrder::DCBA,
    ];

    match field {
        DiscoveryField::Interface => {
            d.interface = cycle(&KINDS, d.interface, forward);
            d.selected = 0;
        }
        DiscoveryField::Port => {
            if !d.ports.is_empty() {
                let n = d.ports.len() as u16;
                d.port_index = wrap_index(d.port_index, n, forward);
            }
        }
        DiscoveryField::Baud => d.baud_rate = cycle(&BAUDS, d.baud_rate, forward),
        DiscoveryField::DataBits => d.data_bits = cycle(&DATA_BITS, d.data_bits, forward),
        DiscoveryField::Parity => d.parity = cycle(&PARITY, d.parity, forward),
        DiscoveryField::StopBits => d.stop_bits = cycle(&STOP_BITS, d.stop_bits, forward),
        DiscoveryField::WordOrder => d.word_order = cycle(&ORDERS, d.word_order, forward),
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
        _ => {}
    }
}

async fn handle_settings_key(key_event: KeyEvent, app: &mut App) {
    if app.settings().is_some_and(|s| s.editing_keybinds) {
        handle_keybinds_key(key_event, app);
        return;
    }

    let kb = app.config.keybinds;
    let count = SettingsField::ALL.len() as u16;
    let selected = app.settings().map_or(0, |s| s.selected);
    let field = SettingsField::ALL[selected as usize];

    match key_event.code {
        c if c == kb.exit => app.close_settings(),
        c if c == kb.settings && !field.is_text_input() => app.close_settings(),
        c if c == kb.move_up => {
            if let Some(s) = app.settings_mut() {
                s.selected = wrap_index(s.selected, count, false);
            }
        }
        c if c == kb.move_down => {
            if let Some(s) = app.settings_mut() {
                s.selected = wrap_index(s.selected, count, true);
            }
        }
        KeyCode::Left => app.settings_adjust(field, -1),
        KeyCode::Right => app.settings_adjust(field, 1),
        c if c == kb.pause && field.is_toggle() => app.settings_adjust(field, 1),
        c if c == kb.action => match field {
            SettingsField::ClearPins => app.clear_pins(),
            SettingsField::ClearLabels => app.clear_labels(),
            SettingsField::ClearCustom => app.clear_custom(),
            f if f.is_toggle() => app.settings_adjust(f, 1),
            SettingsField::EditKeybinds => {
                if let Some(s) = app.settings_mut() {
                    s.open_keybinds();
                }
            }
            SettingsField::Save => app.settings_save(),
            SettingsField::LoadConfig => app.settings_load().await,
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
        if key_event.code != KeyCode::Esc {
            if let Some(&action) = KeybindAction::ALL.get(selected) {
                app.config.keybinds.set(action, key_event.code);
                app.dirty = true;
            }
        }
        if let Some(s) = app.settings_mut() {
            s.kb_capturing = false;
        }
        return;
    }

    match key_event.code {
        KeyCode::Esc => {
            if let Some(s) = app.settings_mut() {
                s.editing_keybinds = false;
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
