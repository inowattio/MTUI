use crate::state::State;

pub fn make_top_title(state: &State) -> &str {
    match state {
        State::Read(_) => "Read",
        State::Write(_) => "Write",
        State::Help => "Help",
        State::Label(_) => "Label",
        State::Save(_) => "Save",
        State::Search(_) => "Search",
        State::Dump(_) => "Dump",
    }
}
