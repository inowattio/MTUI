use crate::config::Keybinds;
use crate::state::{SettingsFocus, State};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::Line;

pub fn make_bottom_title(theme: &Theme, state: &State, kb: &Keybinds) -> Line<'static> {
    let items = match state {
        State::Read(_) => [Hint::key(kb.action, "Read"), Hint::key(kb.help, "Help")],
        State::Discovery(_) => [Hint::key(kb.action, "Connect"), Hint::key(kb.exit, "Back")],
        State::Settings(s) => {
            let primary = if s.focus == SettingsFocus::Categories {
                Hint::key(kb.action, "Open")
            } else {
                Hint::key(kb.action, "Apply")
            };
            [primary, Hint::key(kb.exit, "Back")]
        }
        State::Logs(_) => [
            Hint::pair(kb.move_up, kb.move_down, "Scroll"),
            Hint::key(kb.exit, "Back"),
        ],
    };
    hints::footer(theme, items)
}
