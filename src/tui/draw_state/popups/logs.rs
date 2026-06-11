use crate::state::LogsParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, logs: &LogsParams) {
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
            " {}/{}   \u{2191}/\u{2193} scroll \u{b7} PgUp/Dn page \u{b7} esc close",
            end.min(len),
            len
        ),
        theme.dim_style(),
    )));

    super::render(frame, area, theme, "Write log", 78, lines);
}
