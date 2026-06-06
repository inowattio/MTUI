use crate::state::LabelParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(
    params: &LabelParams,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let (text, text_style) = if params.text.is_empty() {
        ("(empty - will remove label)".to_string(), theme.dim_style())
    } else {
        (params.text.clone(), theme.base())
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Device: ", theme.dim_style()),
            Span::styled(device.to_string(), theme.base()),
        ]),
        Line::default(),
        Line::from(vec![
            Span::styled("Label at ", theme.dim_style()),
            Span::styled(params.position.to_string(), theme.accent_style()),
            Span::styled(format!("   ({:?})", params.register_type), theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("Text: ", theme.dim_style()),
            Span::styled(text, text_style),
        ]),
    ];

    if let Some(result) = &params.result {
        lines.push(Line::from(Span::styled(result.clone(), theme.changed_style())));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left),
        area,
    );
}
