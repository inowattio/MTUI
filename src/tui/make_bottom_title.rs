use crate::app::App;
use crate::state::{SettingsFocus, State};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::Line;

pub fn make_bottom_title(theme: &Theme, app: &App) -> Line<'static> {
    let kb = &app.config.keybinds;
    match &app.state {
        State::Read(p) => {
            let base = [
                Hint::key(kb.switch_view, "Panel"),
                Hint::key(kb.action, "Read"),
                Hint::key(kb.help, "Help"),
            ];
            if p.graph {
                let [panel, read, help] = base;
                let clear = Hint::key(kb.clear, "Clear");
                if !app.cursor_cell().0.is_bit() && app.graph_cycle_len() > 1 {
                    let cycle = Hint::key(kb.dump, "Cycle");
                    hints::footer(theme, [cycle, clear, panel, read, help])
                } else {
                    hints::footer(theme, [clear, panel, read, help])
                }
            } else {
                hints::footer(theme, base)
            }
        }
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
