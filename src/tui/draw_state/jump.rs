use crate::state::JumpParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(
    params: &JumpParams,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Device: ", theme.dim_style()),
            Span::styled(device.to_string(), theme.base()),
        ]),
        Line::default(),
        Line::from(vec![
            Span::styled("Jump from ", theme.dim_style()),
            Span::styled(params.from.to_string(), theme.base()),
            Span::styled(" to ", theme.dim_style()),
            Span::styled(params.to.to_string(), theme.accent_style()),
            Span::styled(format!("   ({:?})", params.register_type), theme.dim_style()),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left),
        area,
    );
}
