use crate::constants::keybind::*;
use crate::state::State;

pub fn make_bottom_title(state: &State) -> String {
    match state {
        State::Read(_) => format!("{HELP} - Help; {PIN} - Add/Remove Pin"),
        State::Jump(_) => format!("{ACTION} - Go; {EXIT} - Back"),
        State::Write(_) => format!("{ACTION} - Write; {EXIT} - Back"),
        State::Help => format!("{EXIT}/{ACTION} - Back"),
        State::Dump(_) => format!("{ACTION} - Start; 0-9 Set Batches; {EXIT} - Back"),
    }
}
