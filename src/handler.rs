use crate::app::{App, AppResult, WriteType};
use crate::constants::keybind;
use crate::num_ops::{decrement_by, decrement_option_by, digit_add, digit_add_option, digit_remove, digit_remove_option, increment_by, increment_option_by, negate_opt_option, set_option_to_zero, set_to_zero};
use crate::state::{ReadPanel, State, StateTransition};
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
        keybind::REFRESH => app.refresh().await,
        keybind::TOGGLE => app.toggle_type(),
        keybind::JUMP => app.switch_focus_to(StateTransition::Jump),
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
            if let State::Write(params) = &mut app.state {
                params.write_type = match params.write_type {
                    WriteType::Word => WriteType::DWord,
                    WriteType::DWord => WriteType::Word,
                }
            } else {
                app.switch_focus_to(StateTransition::Write);
            }
        }
        keybind::MOVE_UP => match &mut app.state {
            State::Read(p) => match p.panel {
                ReadPanel::Main => {
                    decrement_by(&mut p.position, 1);
                    p.scroll_to_cursor(rows);
                }
                ReadPanel::Pinned => {
                    p.pinned_index = p.pinned_index.saturating_sub(1);
                    p.scroll_pinned(rows, pinned_len);
                }
            },
            State::Jump(p) => decrement_by(&mut p.to, 1),
            State::Write(p) => decrement_option_by(&mut p.value, 1),
            _ => {}
        },
        keybind::MOVE_DOWN => match &mut app.state {
            State::Read(p) => match p.panel {
                ReadPanel::Main => {
                    increment_by(&mut p.position, 1);
                    p.scroll_to_cursor(rows);
                }
                ReadPanel::Pinned => {
                    p.pinned_index = p.pinned_index.saturating_add(1);
                    p.scroll_pinned(rows, pinned_len);
                }
            },
            State::Jump(p) => increment_by(&mut p.to, 1),
            State::Write(p) => increment_option_by(&mut p.value, 1),
            _ => {}
        },
        keybind::NEGATOR => match &mut app.state {
            State::Write(params) => negate_opt_option(&mut params.value),
            _ => {}
        },
        KeyCode::Char(c) => {
            if !c.is_ascii_digit() {
                return Ok(());
            }

            let digit = c as u8 - '0' as u8;

            match &mut app.state {
                State::Read(params) => {
                    digit_add(&mut params.position, digit);
                    params.scroll_to_cursor(rows);
                }
                State::Jump(params) => digit_add(&mut params.to, digit),
                State::Write(params) => digit_add_option(&mut params.value, digit),
                _ => {}
            };
        }
        KeyCode::Backspace => match &mut app.state {
            State::Read(params) => {
                digit_remove(&mut params.position);
                params.scroll_to_cursor(rows);
            }
            State::Jump(params) => digit_remove(&mut params.to),
            State::Write(params) => digit_remove_option(&mut params.value),
            _ => {}
        },
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

    match &mut app.state {
        State::Read(params) => set_to_zero(&mut params.position),
        State::Jump(params) => set_to_zero(&mut params.to),
        State::Write(params) => set_option_to_zero(&mut params.value),
        _ => {}
    };

    for digit in digits {
        match &mut app.state {
            State::Read(params) => digit_add(&mut params.position, digit),
            State::Jump(params) => digit_add(&mut params.to, digit),
            State::Write(params) => digit_add_option(&mut params.value, digit),
            _ => {}
        };
    }

    let rows = app.visible_rows.get();
    if let State::Read(params) = &mut app.state {
        params.scroll_to_cursor(rows);
    }
}
