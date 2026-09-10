use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, quitting: bool) {
    let (prompt, confirm, save) = if quitting {
        ("Quit anyway?", "Quit", "Save & quit")
    } else {
        ("Load next configuration anyway?", "Load", "Save & load")
    };
    let footer = [
        Hint::key(kb.action, confirm),
        Hint::key(KeyCode::Char('s'), save),
        Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel"),
    ];
    let lines = vec![
        Line::from(Span::styled(format!(" {prompt}"), theme.base())),
        hints::footer(theme, footer),
    ];

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Unsaved changes", width, lines);
}
