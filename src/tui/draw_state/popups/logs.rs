use crate::config::Keybinds;
use crate::state::LogsParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, logs: &LogsParams) {
    let visible = LogsParams::VISIBLE as usize;
    let len = logs.lines.len();
    let top = (logs.top as usize).min(len.saturating_sub(1));
    let end = (top + visible).min(len);

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

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(
            " {}/{}   {}/{} scroll \u{b7} {}/{} page \u{b7} {} close",
            end.min(len),
            len,
            kb.move_up,
            kb.move_down,
            kb.page_up,
            kb.page_down,
            kb.exit
        ),
        theme.dim_style(),
    )));

    super::render(frame, area, theme, "Write log", 78, lines);
}
