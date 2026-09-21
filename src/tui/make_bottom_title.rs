use crate::app::App;
use crate::input::KeyCode;
use crate::state::{SettingsFocus, State};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::text::{Line, Span};

pub fn make_bottom_title(theme: &Theme, app: &App) -> Line<'static> {
    let kb = &app.config.keybinds;
    match &app.state {
        State::Read(p) => {
            if let Some(name) = app.copy_column_name() {
                let mut line = hints::footer(
                    theme,
                    [
                        Hint::pair(KeyCode::Left, KeyCode::Right, "Column"),
                        Hint::key(KeyCode::Enter, "Copy"),
                        Hint::key(KeyCode::Esc, "Cancel"),
                    ],
                );
                line.spans
                    .insert(0, Span::styled(format!(" {name}"), theme.accent_style()));
                return line;
            }
            let panel = Hint::key(kb.panel, "Panel");
            let read = Hint::key(kb.refresh, "Read");
            let help = Hint::key(kb.help, "Help");

            if p.graph {
                let hold = Hint::key(kb.pin, "Hold");
                let clear = Hint::key(kb.clear_session, "Clear");
                if !app.cursor_cell().0.is_bit() && app.graph_cycle_len() > 1 {
                    let cycle = Hint::key(kb.dump, "Cycle");
                    hints::footer(theme, [cycle, hold, clear, read, help])
                } else {
                    hints::footer(theme, [hold, clear, panel, read, help])
                }
            } else {
                let kind = Hint::key(kb.register_type, "Type");
                hints::footer(
                    theme,
                    [
                        Hint::pair(KeyCode::Down, KeyCode::Right, "Move"),
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
                Hint::key(KeyCode::Enter, "Open")
            } else {
                Hint::key(KeyCode::Enter, "Apply")
            };
            hints::footer(
                theme,
                [
                    primary,
                    Hint::key(kb.panel, "Category"),
                    Hint::key(KeyCode::Esc, "Back"),
                ],
            )
        }
        State::Logs(l) => hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Down, KeyCode::Right, "Scroll"),
                Hint::key(kb.write, if l.wrap { "Unwrap" } else { "Wrap" }),
                Hint::key(kb.copy_column, "Copy"),
                Hint::key(kb.dump, "Dump"),
                Hint::key(KeyCode::Esc, "Back"),
            ],
        ),
    }
}
