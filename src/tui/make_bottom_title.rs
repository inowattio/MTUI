use crate::constants::keybind::*;
use crate::state::State;

pub fn make_bottom_title(state: &State) -> String {
    match state {
        State::Read(_) => {
            format!("{ACTION} - Read; {SWITCH_VIEW} - Main/Pinned; {SEARCH} - Search; {REFRESH} - Refresh; {PIN} - Pin; {LABEL} - Label; {SAVE} - Save; {HELP} - Help")
        }
        State::Jump(_) => format!("{ACTION} - Go; {EXIT} - Back"),
        State::Write(_) => format!("{ACTION} - Write; {EXIT} - Back"),
        State::Help => format!("{EXIT}/{ACTION} - Back"),
        State::Dump(_) => format!("{ACTION} - Start; 0-9 Set Batches; {EXIT} - Back"),
        State::Label(_) => format!("{ACTION} - Set (empty to remove); Esc - Cancel"),
        State::Save(_) => format!("{ACTION} - Save to file; {EXIT} - Back"),
        State::Search(_) => format!("Type to filter; {MOVE_UP}/{MOVE_DOWN} - Select; {ACTION} - Jump; Esc - Back"),
    }
}
