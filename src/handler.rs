use crate::app::{App, AppResult, WriteType};
use crate::config::Column;
use crate::constants::keybind;
use crate::num_ops::{decrement_by, decrement_option_by, digit_add, digit_add_option, digit_remove, digit_remove_option, increment_by, increment_option_by, negate_opt_option, set_option_to_zero, set_to_zero};
use crate::state::{ReadPanel, State, StateTransition, WriteParams};
use crossterm::event::{KeyCode, KeyEvent};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    let rows = app.visible_rows.get();
    let pinned_len = app.pinned_registers.len() as u16;

    if matches!(app.state, State::Label(_)) {
        match key_event.code {
            KeyCode::Esc => app.cancel_label(),
            keybind::ACTION => app.commit_label(),
            KeyCode::Backspace => app.label_backspace(),
            KeyCode::Char(c) => app.label_input(c),
            _ => {}
        }
        return Ok(());
    }

    if matches!(app.state, State::Search(_)) {
        match key_event.code {
            KeyCode::Esc => app.switch_focus_to(StateTransition::Read),
            keybind::ACTION => app.search_commit(),
            keybind::MOVE_UP => app.search_move(false),
            keybind::MOVE_DOWN => app.search_move(true),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(c) => app.search_input(c),
            _ => {}
        }
        return Ok(());
    }

    if matches!(&app.state, State::Read(p) if p.picker.is_some()) {
        let count = Column::ALL.len() as u16;
        match key_event.code {
            KeyCode::Esc | keybind::EXIT | keybind::COLUMNS => {
                if let State::Read(p) = &mut app.state {
                    p.picker = None;
                }
            }
            keybind::MOVE_UP => {
                if let State::Read(p) = &mut app.state {
                    if let Some(selected) = &mut p.picker {
                        *selected = selected.saturating_sub(1);
                    }
                }
            }
            keybind::MOVE_DOWN => {
                if let State::Read(p) = &mut app.state {
                    if let Some(selected) = &mut p.picker {
                        *selected = (*selected + 1).min(count - 1);
                    }
                }
            }
            keybind::ACTION | KeyCode::Char(' ') => {
                let selected = match &app.state {
                    State::Read(p) => p.picker.unwrap_or(0) as usize,
                    _ => 0,
                };
                if let Some(&column) = Column::ALL.get(selected) {
                    app.toggle_column(column);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    if matches!(&app.state, State::Read(p) if p.jump.is_some()) {
        match key_event.code {
            KeyCode::Esc | keybind::JUMP => {
                if let State::Read(p) = &mut app.state {
                    p.jump = None;
                }
            }
            keybind::ACTION => app.commit_jump(),
            KeyCode::Backspace => {
                if let State::Read(p) = &mut app.state {
                    if let Some(value) = &mut p.jump {
                        digit_remove(value);
                    }
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let State::Read(p) = &mut app.state {
                    if let Some(value) = &mut p.jump {
                        digit_add(value, digit);
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    if matches!(&app.state, State::Read(p) if p.write.is_some()) {
        match key_event.code {
            KeyCode::Esc => {
                if let State::Read(p) = &mut app.state {
                    p.write = None;
                }
            }
            keybind::WRITE => {
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        w.write_type = match w.write_type {
                            WriteType::Word => WriteType::DWord,
                            WriteType::DWord => WriteType::Word,
                        };
                    }
                }
            }
            keybind::ACTION => app.commit_write(),
            keybind::MOVE_UP => {
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        decrement_option_by(&mut w.value, 1);
                    }
                }
            }
            keybind::MOVE_DOWN => {
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        increment_option_by(&mut w.value, 1);
                    }
                }
            }
            keybind::NEGATOR => {
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        negate_opt_option(&mut w.value);
                    }
                }
            }
            KeyCode::Backspace => {
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        digit_remove_option(&mut w.value);
                    }
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let State::Read(p) = &mut app.state {
                    if let Some(w) = &mut p.write {
                        digit_add_option(&mut w.value, digit);
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    match key_event.code {
        keybind::EXIT => app.quit(),
        keybind::PIN => app.pin(),
        keybind::DUMP => {
            if matches!(app.state, State::Read(_)) {
                app.switch_focus_to(StateTransition::Dump);
            }
        }
        keybind::HELP => app.switch_focus_to(StateTransition::Help),
        keybind::SAVE => app.switch_focus_to(StateTransition::Save),
        keybind::SEARCH => app.switch_focus_to(StateTransition::Search),
        keybind::COLUMNS => {
            if let State::Read(p) = &mut app.state {
                p.picker = Some(0);
            }
        }
        keybind::REFRESH => app.refresh().await,
        keybind::TOGGLE => app.toggle_type(),
        keybind::JUMP => {
            if let State::Read(p) = &mut app.state {
                p.jump = Some(0);
            }
        }
        keybind::LABEL => {
            if matches!(app.state, State::Read(_)) {
                app.switch_focus_to(StateTransition::Label);
            }
        }
        keybind::SWITCH_VIEW => {
            if let State::Read(p) = &mut app.state {
                p.toggle_panel();
                p.scroll_pinned(rows, pinned_len);
            }
        }
        keybind::ACTION => app.do_action().await,
        keybind::WRITE => {
            if let State::Read(p) = &mut app.state {
                p.write = Some(WriteParams {
                    position: p.position,
                    ..Default::default()
                });
            }
        }
        keybind::MOVE_UP => {
            if let State::Read(p) = &mut app.state {
                match p.panel {
                    ReadPanel::Main => {
                        decrement_by(&mut p.position, 1);
                        p.scroll_to_cursor(rows);
                    }
                    ReadPanel::Pinned => {
                        p.pinned_index = p.pinned_index.saturating_sub(1);
                        p.scroll_pinned(rows, pinned_len);
                    }
                }
            }
        }
        keybind::MOVE_DOWN => {
            if let State::Read(p) = &mut app.state {
                match p.panel {
                    ReadPanel::Main => {
                        increment_by(&mut p.position, 1);
                        p.scroll_to_cursor(rows);
                    }
                    ReadPanel::Pinned => {
                        p.pinned_index = p.pinned_index.saturating_add(1);
                        p.scroll_pinned(rows, pinned_len);
                    }
                }
            }
        }
        KeyCode::Char(c) => {
            if !c.is_ascii_digit() {
                return Ok(());
            }

            let digit = c as u8 - b'0';

            if let State::Read(params) = &mut app.state {
                digit_add(&mut params.position, digit);
                params.scroll_to_cursor(rows);
            }
        }
        KeyCode::Backspace => {
            if let State::Read(params) = &mut app.state {
                digit_remove(&mut params.position);
                params.scroll_to_cursor(rows);
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_paste(data: String, app: &mut App) {
    let original_size = data.len();
    let digits = data.chars().into_iter().filter(char::is_ascii_digit).map(|c| c as u8 - '0' as u8).collect::<Vec<_>>();

    if digits.len() != original_size {
        return;
    }

    let rows = app.visible_rows.get();
    let State::Read(params) = &mut app.state else {
        return;
    };

    if let Some(w) = &mut params.write {
        set_option_to_zero(&mut w.value);
        for digit in digits {
            digit_add_option(&mut w.value, digit);
        }
        return;
    }

    set_to_zero(&mut params.position);
    for digit in digits {
        digit_add(&mut params.position, digit);
    }
    params.scroll_to_cursor(rows);
}
