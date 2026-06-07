use crate::state::State;

pub fn make_top_title(state: &State) -> &str {
    match state {
        State::Read(_) => "Read",
        State::Discovery(_) => "Discovery",
    }
}
