use crate::constants::keybind::*;
use crate::state::State;

pub fn make_bottom_title(state: &State) -> String {
    match state {
        State::Read(_) => format!("{ACTION} - Read; {HELP} - Help"),
        State::Discovery(_) => format!("{ACTION} - Connect; Esc - Back"),
        State::Settings(_) => format!("{ACTION} - Apply; Esc - Back"),
    }
}
