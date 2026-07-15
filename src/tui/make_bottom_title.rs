use crate::config::Keybinds;
use crate::state::{SettingsFocus, State};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::Line;

pub fn make_bottom_title(theme: &Theme, state: &State, kb: &Keybinds) -> Line<'static> {
    match state {
        State::Read(_) => hints::footer(
            theme,
            [
                Hint::key(kb.switch_view, "Panel"),
                Hint::key(kb.action, "Read"),
                Hint::key(kb.help, "Help"),
            ],
        ),
        State::Settings(s) => {
            let primary = if s.focus == SettingsFocus::Categories {
                Hint::key(kb.action, "Open")
            } else {
                Hint::key(kb.action, "Apply")
            };
            hints::footer(theme, [primary, Hint::key(kb.exit, "Back")])
        }
        State::Logs(_) => hints::footer(
            theme,
            [
                Hint::pair(kb.move_up, kb.move_down, "Scroll"),
                Hint::key(kb.exit, "Back"),
            ],
        ),
    }
}
