use crate::app::{App, AppResult, WriteType};
use crate::config::Column;
use crate::constants::keybind;
use crate::num_ops::{
    decrement_by, decrement_option_by, digit_add, digit_add_option, digit_remove,
    digit_remove_option, increment_by, increment_option_by, negate_opt_option, set_option_to_zero,
    set_to_zero,
};
use crate::state::{Popup, PopupKind, ReadPanel};
use crossterm::event::{KeyCode, KeyEvent};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    let rows = app.visible_rows.get();
    let pinned_len = app.pinned_registers.len() as u16;

    // A popup is modal: while one is open it consumes every key.
    if let Some(kind) = app.popup_kind() {
        handle_popup_key(kind, key_event, app);
        return Ok(());
    }

    match key_event.code {
        keybind::EXIT => app.quit(),
        keybind::PIN => app.pin(),
        keybind::DUMP => app.open_dump(),
        keybind::HELP => app.open_help(),
        keybind::SAVE => app.open_save(),
        keybind::SEARCH => app.open_search(),
        keybind::COLUMNS => app.open_columns(),
        keybind::JUMP => app.open_jump(),
        keybind::WRITE => app.open_write(),
        keybind::LABEL => app.open_label(),
        keybind::REFRESH => app.refresh().await,
        keybind::TOGGLE => app.toggle_type(),
        keybind::ACTION => app.refresh().await,
        keybind::SWITCH_VIEW => {
            {
                let p = app.read_mut();
                p.toggle_panel();
                p.scroll_pinned(rows, pinned_len);
            }
        }
        keybind::MOVE_UP => {
            {
                let p = app.read_mut();
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
            {
                let p = app.read_mut();
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
            {
                let p = app.read_mut();
                digit_add(&mut p.position, digit);
                p.scroll_to_cursor(rows);
            }
        }
        KeyCode::Backspace => {
            {
                let p = app.read_mut();
                digit_remove(&mut p.position);
                p.scroll_to_cursor(rows);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Route a key to the currently-open popup. Data-only edits are done inline;
/// actions that touch the wider app (reads, writes, saves, toggles) call methods.
fn handle_popup_key(kind: PopupKind, key_event: KeyEvent, app: &mut App) {
    match kind {
        // Any key dismisses Help.
        PopupKind::Help => app.close_popup(),

        PopupKind::Save => match key_event.code {
            keybind::ACTION => app.commit_save(),
            KeyCode::Esc | keybind::EXIT | keybind::SAVE => app.close_popup(),
            _ => {}
        },

        PopupKind::Dump => match key_event.code {
            keybind::ACTION => app.commit_dump(),
            KeyCode::Esc | keybind::EXIT | keybind::DUMP => app.close_popup(),
            _ => {}
        },

        PopupKind::Columns => {
            let count = Column::ALL.len() as u16;
            match key_event.code {
                KeyCode::Esc | keybind::EXIT | keybind::COLUMNS => app.close_popup(),
                keybind::MOVE_UP => {
                    {
                let p = app.read_mut();
                        if let Some(Popup::Columns(i)) = &mut p.popup {
                            *i = i.saturating_sub(1);
                        }
                    }
                }
                keybind::MOVE_DOWN => {
                    {
                let p = app.read_mut();
                        if let Some(Popup::Columns(i)) = &mut p.popup {
                            *i = (*i + 1).min(count - 1);
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

        PopupKind::Jump => match key_event.code {
            KeyCode::Esc | keybind::JUMP => app.close_popup(),
            keybind::ACTION => app.commit_jump(),
            KeyCode::Backspace => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Jump(value)) = &mut p.popup {
                        digit_remove(value);
                    }
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                {
                let p = app.read_mut();
                    if let Some(Popup::Jump(value)) = &mut p.popup {
                        digit_add(value, digit);
                    }
                }
            }
            _ => {}
        },

        PopupKind::Write => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            keybind::ACTION => app.commit_write(),
            keybind::WRITE => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        w.write_type = match w.write_type {
                            WriteType::Word => WriteType::DWord,
                            WriteType::DWord => WriteType::Word,
                        };
                    }
                }
            }
            keybind::MOVE_UP => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        decrement_option_by(&mut w.value, 1);
                    }
                }
            }
            keybind::MOVE_DOWN => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        increment_option_by(&mut w.value, 1);
                    }
                }
            }
            keybind::NEGATOR => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        negate_opt_option(&mut w.value);
                    }
                }
            }
            KeyCode::Backspace => {
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        digit_remove_option(&mut w.value);
                    }
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                {
                let p = app.read_mut();
                    if let Some(Popup::Write(w)) = &mut p.popup {
                        digit_add_option(&mut w.value, digit);
                    }
                }
            }
            _ => {}
        },

        PopupKind::Search => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            keybind::ACTION => app.search_commit(),
            keybind::MOVE_UP => app.search_move(false),
            keybind::MOVE_DOWN => app.search_move(true),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(c) => app.search_input(c),
            _ => {}
        },

        PopupKind::Label => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            keybind::ACTION => app.commit_label(),
            KeyCode::Backspace => app.label_backspace(),
            KeyCode::Char(c) => app.label_input(c),
            _ => {}
        },
    }
}

pub fn handle_paste(data: String, app: &mut App) {
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
    let p = app.read_mut();

    // Paste into the numeric popup that's open, otherwise the cursor address.
    match &mut p.popup {
        Some(Popup::Write(w)) => {
            set_option_to_zero(&mut w.value);
            for digit in digits {
                digit_add_option(&mut w.value, digit);
            }
        }
        Some(Popup::Jump(value)) => {
            set_to_zero(value);
            for digit in digits {
                digit_add(value, digit);
            }
        }
        None => {
            set_to_zero(&mut p.position);
            for digit in digits {
                digit_add(&mut p.position, digit);
            }
            p.scroll_to_cursor(rows);
        }
        _ => {}
    }
}
