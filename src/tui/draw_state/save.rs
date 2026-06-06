use crate::constants::CONFIG_PATH;
use crate::state::SaveParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(
    params: &SaveParams,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Device: ", theme.dim_style()),
            Span::styled(device.to_string(), theme.base()),
        ]),
        Line::default(),
        Line::from(Span::styled(
            format!("Save current configuration (labels & pins) to {CONFIG_PATH}?"),
            theme.base(),
        )),
        Line::from(Span::styled("Press Enter to confirm.", theme.dim_style())),
    ];

    if let Some(result) = &params.result {
        let style = if result.starts_with("Saved") {
            theme.ok_style()
        } else {
            Style::default().fg(theme.err)
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(result.clone(), style)));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.panel("Save"))
            .alignment(Alignment::Left),
        area,
    );
}
