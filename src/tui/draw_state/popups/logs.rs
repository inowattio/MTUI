use crate::config::Keybinds;
use crate::state::LogsParams;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, logs: &LogsParams) {
    let visible = LogsParams::VISIBLE as usize;
    let len = logs.lines.len();
    let (top, end) = super::window(logs.top as usize, visible, len);

    let mut lines = vec![
        Line::from(Span::styled(format!(" {}", logs.path), theme.dim_style())),
        Line::default(),
    ];

    for line in &logs.lines[top..end] {
        lines.push(Line::from(Span::styled(format!(" {line}"), theme.base())));
    }
    for _ in end..top + visible {
        lines.push(Line::default());
    }

    lines.push(hints::more(theme, top, len.saturating_sub(end)));
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(kb.move_up, kb.move_down, "Scroll"),
            Hint::pair(kb.page_up, kb.page_down, "Page"),
            Hint::key(kb.exit, "Close"),
        ],
    ));

    super::render(frame, area, theme, "Write log", 78, lines);
}
