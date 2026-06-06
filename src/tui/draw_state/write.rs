use crate::state::WriteParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(
    params: &WriteParams,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let value = params
        .value
        .map_or("none".to_string(), |n| n.to_string());
    let result = params.result.as_deref().unwrap_or("-");

    let lines = vec![
        Line::from(vec![
            Span::styled("Device: ", theme.dim_style()),
            Span::styled(device.to_string(), theme.base()),
        ]),
        Line::default(),
        Line::from(vec![
            Span::styled("Write at ", theme.dim_style()),
            Span::styled(params.position.to_string(), theme.accent_style()),
            Span::styled("   value ", theme.dim_style()),
            Span::styled(value, theme.base()),
            Span::styled(format!("   ({:?})", params.write_type), theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("Result: ", theme.dim_style()),
            Span::styled(result.to_string(), theme.base()),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left),
        area,
    );
}
