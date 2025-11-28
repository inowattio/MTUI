use crate::app::{App, AppResult, State};
use crossterm::event::{KeyCode, KeyEvent};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Char('h') => {
            app.switch_focus_to(State::Help);
        }
        KeyCode::Char('r') => {
            app.refresh().await;
        }
        KeyCode::Char('t') => {
            app.toggle_type();
        }
        KeyCode::Char('w') => {
            app.switch_focus_to(State::Write);
        }
        KeyCode::Char('j') => {
            if let Some(n) = app.input_number.as_mut() {
                *n = n.abs();
            }
            app.switch_focus_to(State::Jump);
        }
        KeyCode::Char(c) => {
            if c.is_digit(10) {
                let n = c as u16 - '0' as u16;
                match app.input_number {
                    None => {
                        app.input_number = Some(n as i32);
                    },
                    Some(input_number) => {
                        if let Some(new_value) = input_number.checked_mul(10).map(|i| i.checked_add(n as i32)).flatten() {
                            app.input_number = Some(new_value);
                        }
                    }
                }
            } else if c == '-' && app.state != State::Jump {
                if let Some(input_number) = app.input_number {
                    app.input_number = Some(-input_number);
                }
            }
        }
        KeyCode::Backspace => {
            if let Some(input_number) = app.input_number {
                if input_number.to_string().len() == 1 {
                    app.input_number = None;
                } else {
                    app.input_number = Some(input_number / 10);
                }
            }
        },
        KeyCode::Enter => {
            app.do_action().await;
        }
        KeyCode::Up => {
            app.up();
        }
        KeyCode::Down => {
            app.down();
        }
        _ => {}
    }
    Ok(())
}
