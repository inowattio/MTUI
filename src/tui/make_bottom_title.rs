use crate::config::Keybinds;
use crate::state::State;

pub fn make_bottom_title(state: &State, kb: &Keybinds) -> String {
    match state {
        State::Read(_) => format!("{} - Read; {} - Help", kb.action, kb.help),
        State::Discovery(_) => format!("{} - Connect; {} - Back", kb.action, kb.exit),
        State::Settings(_) => format!("{} - Apply; {} - Back", kb.action, kb.exit),
        State::Logs(_) => format!(
            "{}/{} - Scroll; {} - Back",
            kb.move_up, kb.move_down, kb.exit
        ),
    }
}
