use crate::state::State;

pub fn make_top_title(state: &State) -> &str {
    match state {
        State::Read(_) => "Read",
        State::Jump(_) => "Jump",
        State::Write(_) => "Write",
        State::Help => "Help",
        State::Dump(_) => "Dump",
        State::Label(_) => "Label",
    }
}
