use crate::constants::keybind::*;
use crate::state::State;

pub fn make_bottom_title(state: &State) -> String {
    match state {
        State::Read(_) => {
            format!("{ACTION} - Read; {SWITCH_VIEW} - Main/Pinned; {REFRESH} - Refresh; {HELP} - Help; {PIN} - Pin; {LABEL} - Label")
        }
        State::Jump(_) => format!("{ACTION} - Go; {EXIT} - Back"),
        State::Write(_) => format!("{ACTION} - Write; {EXIT} - Back"),
        State::Help => format!("{EXIT}/{ACTION} - Back"),
        State::Dump(_) => format!("{ACTION} - Start; 0-9 Set Batches; {EXIT} - Back"),
        State::Label(_) => format!("{ACTION} - Save (empty to remove); Esc - Cancel"),
    }
}
