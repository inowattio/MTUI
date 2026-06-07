use crate::constants::keybind::*;
use crate::state::State;

pub fn make_bottom_title(state: &State) -> String {
    match state {
        State::Read(_) => format!("{ACTION} - Read; {HELP} - Help"),
        State::Discovery(_) => format!("{MOVE_UP}/{MOVE_DOWN} - Move; {ACTION} - Select; Esc - Back"),
        State::Settings(_) => format!("{MOVE_UP}/{MOVE_DOWN} - Move; {MOVE_LEFT}/{MOVE_RIGHT} - Change; {ACTION} - Apply; Esc - Back"),
    }
}
