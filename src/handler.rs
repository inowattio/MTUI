use crate::app::{App, AppResult};
use crate::config::Column;
use crate::constants::keybind;
use crate::input::{KeyCode, KeyEvent};
use crate::modbus::{DataBits, Parity, StopBits, WordOrder};
use crate::num_ops::{
    decrement_option_by, digit_add, digit_add_option, digit_remove, digit_remove_option,
    increment_option_by, negate_opt_option, set_option_to_zero, set_to_zero,
};
use crate::state::{
    DiscoveryField, DiscoveryParams, InterfaceKind, LogsParams, Popup, PopupKind, ReadPanel,
    SettingsField,
};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    let rows = app.visible_rows.get();

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
            keybind::DUMP => {
                app.toggle_graph_width();
                return Ok(());
            }
            keybind::COLUMNS | keybind::SLAVE | keybind::SWITCH_VIEW | keybind::TOGGLE => {
                return Ok(())
            }
            _ => {}
        }
    }

    match key_event.code {
        keybind::EXIT => app.request_quit(),
        keybind::PIN => app.pin(),
        keybind::DUMP => app.open_dump(),
        keybind::HELP => app.open_help(),
        keybind::COLUMNS => app.open_columns(),
        keybind::JUMP => app.open_search(),
        keybind::WRITE => app.open_write(),
        keybind::LABEL => app.open_label(),
        keybind::CUSTOM => app.open_custom(),
        keybind::SLAVE => app.open_slave(),
        keybind::DISCOVERY => app.open_discovery(),
        keybind::SETTINGS => app.open_settings(),
        keybind::GRAPH => app.toggle_graph(),
        keybind::INSPECT => app.open_inspect(),
        keybind::CYCLE_POSITION => app.cycle_position(),
        keybind::COPY_ADDRESS => app.copy_address(),
        keybind::LOGS => app.open_logs(),
        keybind::APP_LOGS => app.open_log_view(),
        keybind::WORD_ORDER => app.toggle_word_order(),
        keybind::REFRESH => app.refresh().await,
        keybind::TOGGLE => app.toggle_type(),
        keybind::PAUSE => app.toggle_pause(),
        keybind::ACTION => app.refresh().await,
        keybind::SWITCH_VIEW => {
            app.read_mut().toggle_panel();
            let len = app.panel_len();
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            p.scroll_pinned(rows, len);
            p.scroll_to_cursor(rows, cols);
        }
        keybind::MOVE_UP | keybind::MOVE_DOWN | keybind::PAGE_UP | keybind::PAGE_DOWN => {
            move_read_cursor(app, key_event.code);
        }
        KeyCode::Left | KeyCode::Right if app.read().panel == ReadPanel::Matrix => {
            let cols = app.config.matrix_cols;
            let p = app.read_mut();
            p.position = if key_event.code == KeyCode::Left {
                p.position.saturating_sub(1)
            } else {
                p.position.saturating_add(1)
            };
            p.scroll_to_cursor(rows, cols);
        }
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

fn move_read_cursor(app: &mut App, code: KeyCode) {
    let rows = app.visible_rows.get();
    let panel_len = app.panel_len();
    let cols = app.config.matrix_cols;
    let step = if matches!(code, keybind::PAGE_UP | keybind::PAGE_DOWN) {
        rows
    } else {
        1
    };
    let up = matches!(code, keybind::MOVE_UP | keybind::PAGE_UP);
    let p = app.read_mut();
    match p.panel {
        ReadPanel::Main => {
            p.position = if up {
                p.position.saturating_sub(step)
            } else {
                p.position.saturating_add(step)
            };
            p.scroll_to_cursor(rows, cols);
        }
        ReadPanel::Matrix => {
            let step = step.saturating_mul(cols.max(1));
            p.position = if up {
                p.position.saturating_sub(step)
            } else {
                p.position.saturating_add(step)
            };
            p.scroll_to_cursor(rows, cols);
        }
        _ => {
            p.pinned_index = if up {
                p.pinned_index.saturating_sub(step)
            } else {
                p.pinned_index.saturating_add(step)
            };
            p.scroll_pinned(rows, panel_len);
        }
    }
}

async fn handle_popup_key(kind: PopupKind, key_event: KeyEvent, app: &mut App) {
    match kind {
        PopupKind::Help => app.close_popup(),

        PopupKind::Inspect => match key_event.code {
            keybind::EXIT | keybind::INSPECT => app.close_popup(),
            keybind::REFRESH | keybind::ACTION => app.refresh().await,
            keybind::MOVE_UP | keybind::MOVE_DOWN | keybind::PAGE_UP | keybind::PAGE_DOWN => {
                move_read_cursor(app, key_event.code);
            }
            _ => {}
        },

        PopupKind::Dump => match key_event.code {
            keybind::ACTION => app.commit_dump(),
            keybind::EXIT | keybind::DUMP => app.close_popup(),
            _ => {}
        },

        PopupKind::Columns => {
            let count = Column::ALL.len() as u16;
            match key_event.code {
                keybind::EXIT | keybind::COLUMNS => app.close_popup(),
                keybind::MOVE_UP => {
                    let p = app.read_mut();
                    if let Some(Popup::Columns(i)) = &mut p.popup {
                        if *i == 0 {
                            *i = count - 1;
                        } else {
                            *i -= 1;
                        }
                    }
                }
                keybind::MOVE_DOWN => {
                    let p = app.read_mut();
                    if let Some(Popup::Columns(i)) = &mut p.popup {
                        if *i == count - 1 {
                            *i = 0;
                        } else {
                            *i += 1;
                        }
                    }
                }
                keybind::ACTION | KeyCode::Char(' ') => {
                    let selected = match &app.read().popup {
                        Some(Popup::Columns(i)) => *i as usize,
                        _ => 0,
                    };
                    if let Some(&column) = Column::ALL.get(selected) {
                        app.toggle_column(column);
                    }
                }
                _ => {}
            }
        }

        PopupKind::Write => match key_event.code {
            keybind::EXIT => app.close_popup(),
            keybind::ACTION => app.commit_write(),
            keybind::WRITE => app.write_toggle_type(),
            keybind::MOVE_UP => {
                if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
                    decrement_option_by(&mut w.value, 1);
                }
                app.clamp_write_value();
            }
            keybind::MOVE_DOWN => {
                if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
                    increment_option_by(&mut w.value, 1);
                }
                app.clamp_write_value();
            }
            KeyCode::Left => app.write_move_bit(true),
            KeyCode::Right => app.write_move_bit(false),
            keybind::PAUSE => app.write_toggle_bit(),
            keybind::NEGATOR => {
                if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
                    negate_opt_option(&mut w.value);
                }
                app.clamp_write_value();
            }
            KeyCode::Backspace => {
                if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
                    digit_remove_option(&mut w.value);
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
                    digit_add_option(&mut w.value, digit);
                }
                app.clamp_write_value();
            }
            _ => {}
        },

        PopupKind::Search => match key_event.code {
            keybind::EXIT => app.close_popup(),
            keybind::ACTION => {
                let _ = app.search_commit();
            }
            keybind::MOVE_UP => app.search_move(false),
            keybind::MOVE_DOWN => app.search_move(true),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(c) => app.search_input(c),
            _ => {}
        },

        PopupKind::Label => match key_event.code {
            keybind::EXIT => app.close_popup(),
            keybind::ACTION => app.commit_label(),
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
                keybind::EXIT => app.close_popup(),
                keybind::MOVE_UP => app.custom_move(false),
                keybind::MOVE_DOWN => app.custom_move(true),
                KeyCode::Left => app.custom_cycle(field, false),
                KeyCode::Right => app.custom_cycle(field, true),
                keybind::ACTION => app.custom_enter(field),
                KeyCode::Backspace => app.custom_backspace(field),
                KeyCode::Char(c) => app.custom_char(field, c),
                _ => {}
            }
        }

        PopupKind::Slave => match key_event.code {
            keybind::EXIT | keybind::SLAVE => app.close_popup(),
            keybind::ACTION => app.commit_slave().await,
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
            keybind::EXIT | keybind::LOGS => app.close_popup(),
            keybind::MOVE_UP => app.logs_scroll(-1),
            keybind::MOVE_DOWN => app.logs_scroll(1),
            keybind::PAGE_UP => app.logs_scroll(-(LogsParams::VISIBLE as i32)),
            keybind::PAGE_DOWN => app.logs_scroll(LogsParams::VISIBLE as i32),
            _ => {}
        },

        PopupKind::Quit => match key_event.code {
            keybind::ACTION | keybind::EXIT => app.quit(),
            KeyCode::Backspace => app.close_popup(),
            _ => {}
        },
    }
}

pub fn handle_paste(data: String, app: &mut App) {
    if app.discovery().is_some() || app.settings().is_some() {
        return;
    }
    let original_size = data.len();
    let digits = data
        .chars()
        .filter(char::is_ascii_digit)
        .map(|c| c as u8 - b'0')
        .collect::<Vec<_>>();

    if digits.len() != original_size {
        return;
    }

    let rows = app.visible_rows.get();

    match app.popup_kind() {
        Some(PopupKind::Write) => {
            if let Some(Popup::Write(w)) = &mut app.read_mut().popup {
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
    let (field, count) = match app.discovery() {
        Some(d) => (d.current_field(), d.fields().len() as u16),
        None => return,
    };

    match key_event.code {
        keybind::EXIT => {
            if app.device.is_some() {
                app.return_to_read();
            } else {
                app.quit();
            }
        }
        keybind::ACTION => app.discovery_connect().await,
        keybind::MOVE_UP => {
            if let Some(d) = app.discovery_mut() {
                d.selected = if d.selected == 0 {
                    count - 1
                } else {
                    d.selected - 1
                };
            }
        }
        keybind::MOVE_DOWN => {
            if let Some(d) = app.discovery_mut() {
                d.selected = (d.selected + 1) % count;
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

fn cycle<T: Copy + PartialEq>(items: &[T], current: T, forward: bool) -> T {
    if items.is_empty() {
        return current;
    }
    let i = items.iter().position(|x| *x == current).unwrap_or(0);
    let n = items.len();
    let j = if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    };
    items[j]
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
                d.port_index = if forward {
                    (d.port_index + 1) % n
                } else {
                    (d.port_index + n - 1) % n
                };
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
    match key_event.code {
        keybind::EXIT | keybind::APP_LOGS => app.close_log_view(),
        keybind::MOVE_UP => app.log_view_scroll(-1),
        keybind::MOVE_DOWN => app.log_view_scroll(1),
        keybind::PAGE_UP => app.log_view_scroll(-(app.visible_rows.get() as i32)),
        keybind::PAGE_DOWN => app.log_view_scroll(app.visible_rows.get() as i32),
        _ => {}
    }
}

async fn handle_settings_key(key_event: KeyEvent, app: &mut App) {
    let count = SettingsField::ALL.len() as u16;
    let selected = app.settings().map(|s| s.selected).unwrap_or(0);
    let field = SettingsField::ALL[selected as usize];

    match key_event.code {
        keybind::EXIT => app.close_settings(),
        keybind::SETTINGS if field != SettingsField::LoadConfig => app.close_settings(),
        keybind::MOVE_UP => {
            if let Some(s) = app.settings_mut() {
                s.selected = if s.selected == 0 {
                    count - 1
                } else {
                    s.selected - 1
                };
            }
        }
        keybind::MOVE_DOWN => {
            if let Some(s) = app.settings_mut() {
                s.selected = (s.selected + 1) % count;
            }
        }
        KeyCode::Left => app.settings_adjust(field, -1),
        KeyCode::Right => app.settings_adjust(field, 1),
        keybind::PAUSE
            if matches!(
                field,
                SettingsField::ReadOnly
                    | SettingsField::LogWrites
                    | SettingsField::ShowContinuation
                    | SettingsField::StartupPanel
                    | SettingsField::IgnoreDirty
            ) =>
        {
            app.settings_adjust(field, 1)
        }
        keybind::ACTION => match field {
            SettingsField::ClearPins => app.clear_pins(),
            SettingsField::ClearLabels => app.clear_labels(),
            SettingsField::ClearCustom => app.clear_custom(),
            SettingsField::ReadOnly
            | SettingsField::LogWrites
            | SettingsField::ShowContinuation
            | SettingsField::StartupPanel
            | SettingsField::IgnoreDirty => app.settings_adjust(field, 1),
            SettingsField::Save => app.settings_save(),
            SettingsField::LoadConfig => app.settings_load().await,
            _ => {}
        },
        KeyCode::Backspace => app.settings_backspace(field),
        KeyCode::Char(c) if field == SettingsField::LoadConfig => {
            if let Some(s) = app.settings_mut() {
                s.load_path.push(c);
            }
        }
        KeyCode::Char(c) if c.is_ascii_digit() => app.settings_digit(field, c as u8 - b'0'),
        _ => {}
    }
}
