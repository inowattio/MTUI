use crate::state::StatusMessage;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    prompt: &str,
    result: &Option<StatusMessage>,
    footer: &str,
) {
    let mut lines = vec![
        Line::from(Span::styled(prompt.to_string(), theme.base())),
        Line::from(Span::styled(footer.to_string(), theme.dim_style())),
    ];

    if let Some(result) = result {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            result.text.clone(),
            theme.message_style(result.kind),
        )));
    }

    super::render(frame, area, theme, title, 60, lines);
}
