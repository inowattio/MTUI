use crate::state::State;

pub const fn make_top_title(state: &State) -> &str {
    match state {
        State::Read(_) => "Read",
        State::Settings(_) => "Settings",
        State::Logs(l) if l.frames => "Frames",
        State::Logs(_) => "Logs",
        State::Diff(_) => "Diff",
    }
}
