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
    result: &Option<String>,
    footer: &str,
) {
    let mut lines = vec![
        Line::from(Span::styled(prompt.to_string(), theme.base())),
        Line::from(Span::styled(footer.to_string(), theme.dim_style())),
    ];

    if let Some(result) = result {
        let style = if result.starts_with("Saved") || result.starts_with("Dumped") {
            theme.ok_style()
        } else if result.contains("failed") {
            theme.err_style()
        } else {
            theme.dim_style()
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(result.clone(), style)));
    }

    super::render(frame, area, theme, title, 60, lines);
}
