use crate::config::Keybinds;
use crate::state::State;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::Line;

pub fn make_bottom_title(theme: &Theme, state: &State, kb: &Keybinds) -> Line<'static> {
    let items = match state {
        State::Read(_) => vec![Hint::key(kb.action, "Read"), Hint::key(kb.help, "Help")],
        State::Discovery(_) => vec![Hint::key(kb.action, "Connect"), Hint::key(kb.exit, "Back")],
        State::Settings(_) => vec![Hint::key(kb.action, "Apply"), Hint::key(kb.exit, "Back")],
        State::Logs(_) => vec![
            Hint::keys(hints::pair(kb.move_up, kb.move_down), "Scroll"),
            Hint::key(kb.exit, "Back"),
        ],
    };
    hints::footer(theme, &items)
}
