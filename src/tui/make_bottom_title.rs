use crate::app::App;
use crate::input::KeyCode;
use crate::state::{SettingsFocus, State};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::Line;

pub fn make_bottom_title(theme: &Theme, app: &App) -> Line<'static> {
    let kb = &app.config.keybinds;
    match &app.state {
        State::Read(p) => {
            let panel = Hint::key(kb.switch_view, "Panel");
            let read = Hint::key(kb.action, "Read");
            let help = Hint::key(kb.help, "Help");

            if p.graph {
                let hold = Hint::key(kb.pin, "Hold");
                let clear = Hint::key(kb.clear, "Clear");
                if !app.cursor_cell().0.is_bit() && app.graph_cycle_len() > 1 {
                    let cycle = Hint::key(kb.dump, "Cycle");
                    hints::footer(theme, [cycle, hold, clear, read, help])
                } else {
                    hints::footer(theme, [hold, clear, panel, read, help])
                }
            } else {
                let kind = Hint::key(kb.toggle, "Type");
                hints::footer(
                    theme,
                    [
                        Hint::pair(kb.move_down, KeyCode::Right, "Move"),
                        kind,
                        panel,
                        read,
                        help,
                    ],
                )
            }
        }
        State::Settings(s) => {
            let primary = if s.focus == SettingsFocus::Categories {
                Hint::key(kb.action, "Open")
            } else {
                Hint::key(kb.action, "Apply")
            };
            hints::footer(theme, [primary, Hint::key(KeyCode::Esc, "Back")])
        }
        State::Logs(l) => hints::footer(
            theme,
            [
                Hint::pair(kb.move_down, KeyCode::Right, "Scroll"),
                Hint::key(kb.write, if l.wrap { "Unwrap" } else { "Wrap" }),
                Hint::key(KeyCode::Esc, "Back"),
            ],
        ),
    }
}
