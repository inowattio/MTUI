use std::ops::Neg;
use crate::app::{App, AppResult, DumpParams, State};
use crossterm::event::{KeyCode, KeyEvent};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Char('d') => {
            app.switch_focus_to(State::Dump(DumpParams::default()));
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
            app.switch_focus_to(State::Write(Default::default()));
        }
        KeyCode::Char('j') => {
            app.switch_focus_to(State::Jump(Default::default()));
        }
        KeyCode::Char(c) => {
            let target = match &mut app.state {
                State::Jump(params) => &mut params.position,
                State::Write(params) => {
                    if c == '-' {
                        if let Some(input_number) = &mut params.value {
                            *input_number = input_number.neg();
                        }
                    }

                    &mut params.value
                },
                _ => &mut None,
            };

            if c.is_digit(10) {
                let n = c as u16 - '0' as u16;
                match target {
                    None => {
                        *target = Some(n as i32);
                    },
                    Some(input_number) => {
                        if let Some(new_value) = input_number.checked_mul(10).map(|i| i.checked_add(n as i32)).flatten() {
                            *target = Some(new_value);
                        }
                    }
                }
            }
        }
        KeyCode::Backspace => {
            let target = match &mut app.state {
                State::Jump(params) => &mut params.position,
                State::Write(params) => &mut params.value,
                _ => &mut None,
            };

            let new_value = if let Some(input_number) = target {
                if input_number.to_string().len() == 1 {
                    None
                } else {
                    Some(*input_number / 10)
                }
            } else {
                target.clone()
            };
            *target = new_value;
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
