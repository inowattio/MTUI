use crate::app::{App, AppResult, DumpParams, State, WriteType};
use crossterm::event::{KeyCode, KeyEvent};
use crate::num_ops::{decrement_by, decrement_option_by, digit_add, digit_add_option, digit_remove, digit_remove_option, increment_by, increment_option_by, negate_opt_option};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Char('p') => {
            app.pin();
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
            if let State::Write(params) = &mut app.state {
                params.write_type = match params.write_type {
                    WriteType::Word => WriteType::DWord,
                    WriteType::DWord => WriteType::Word
                }
            } else {
                app.switch_focus_to(State::Write(Default::default()));
            }
        }
        KeyCode::Char('j') => {
            app.switch_focus_to(State::Jump(Default::default()));
        }
        KeyCode::Char(c) => {
            if c.is_ascii_digit() {
                let digit = c as u8 - '0' as u8;

                match &mut app.state {
                    State::Read(params) => digit_add(&mut params.position, digit),
                    State::Jump(params) => digit_add(&mut params.to, digit),
                    State::Write(params) => digit_add_option(&mut params.value, digit),
                    State::Dump(params) => {
                        if params.started {
                            return Ok(());
                        }

                        params.error = None;

                        digit_add_option(&mut params.total_batches, digit)
                    },
                    _ => {}
                };
            } else if c == '-' {
                match &mut app.state {
                    State::Write(params) => negate_opt_option(&mut params.value),
                    _ => {}
                };
            } else {
                return Ok(());
            };
        }
        KeyCode::Backspace => {
            match &mut app.state {
                State::Read(params) => digit_remove(&mut params.position),
                State::Jump(params) => digit_remove(&mut params.to),
                State::Write(params) => digit_remove_option(&mut params.value),
                State::Dump(params) => {
                    if params.started {
                        return Ok(());
                    }

                    params.error = None;

                    digit_remove_option(&mut params.total_batches)
                },
                _ => {},
            };
        },
        KeyCode::Enter => {
            app.do_action().await;
        }
        KeyCode::Up => {
            match &mut app.state {
                State::Read(p) => decrement_by(&mut p.position, 1),
                State::Jump(p) => decrement_by(&mut p.to, 1),
                State::Write(p) => decrement_option_by(&mut p.value, 1),
                State::Dump(p) => decrement_by(&mut p.start_position, 1),
                _ => {},
            }
        }
        KeyCode::Down => {
            match &mut app.state {
                State::Read(p) => increment_by(&mut p.position, 1),
                State::Jump(p) => increment_by(&mut p.to, 1),
                State::Write(p) => increment_option_by(&mut p.value, 1),
                State::Dump(p) => increment_by(&mut p.start_position, 1),
                _ => {},
            }
        }
        _ => {}
    }
    Ok(())
}
