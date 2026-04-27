use crate::app::{App, AppResult, WriteType};
use crate::constants::keybind;
use crate::num_ops::{
    decrement_by, decrement_option_by, digit_add, digit_add_option, digit_remove,
    digit_remove_option, increment_by, increment_option_by, negate_opt_option,
};
use crate::state::{State, StateTransition};
use crossterm::event::{KeyCode, KeyEvent};

pub async fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        keybind::EXIT => app.quit(),
        keybind::PIN => app.pin(),
        keybind::DUMP => app.switch_focus_to(StateTransition::Dump),
        keybind::HELP => app.switch_focus_to(StateTransition::Help),
        keybind::REFRESH => app.refresh().await,
        keybind::TOGGLE => app.toggle_type(),
        keybind::JUMP => app.switch_focus_to(StateTransition::Jump),
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
            State::Read(p) => decrement_by(&mut p.position, 1),
            State::Jump(p) => decrement_by(&mut p.to, 1),
            State::Write(p) => decrement_option_by(&mut p.value, 1),
            State::Dump(p) => decrement_by(&mut p.start_position, 1),
            _ => {}
        },
        keybind::MOVE_DOWN => match &mut app.state {
            State::Read(p) => increment_by(&mut p.position, 1),
            State::Jump(p) => increment_by(&mut p.to, 1),
            State::Write(p) => increment_option_by(&mut p.value, 1),
            State::Dump(p) => increment_by(&mut p.start_position, 1),
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
                State::Read(params) => digit_add(&mut params.position, digit),
                State::Jump(params) => digit_add(&mut params.to, digit),
                State::Write(params) => digit_add_option(&mut params.value, digit),
                State::Dump(params) => {
                    if params.started {
                        return Ok(());
                    }

                    params.error = None;

                    digit_add_option(&mut params.total_batches, digit)
                }
                _ => {}
            };
        }
        KeyCode::Backspace => match &mut app.state {
            State::Read(params) => digit_remove(&mut params.position),
            State::Jump(params) => digit_remove(&mut params.to),
            State::Write(params) => digit_remove_option(&mut params.value),
            State::Dump(params) => {
                if params.started {
                    return Ok(());
                }

                params.error = None;

                digit_remove_option(&mut params.total_batches)
            }
            _ => {}
        },
        _ => {}
    }
    Ok(())
}
