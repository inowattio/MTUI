use crate::config::Keybinds;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, value: u16) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Slave ID: ", theme.dim_style()),
            Span::styled(value.min(u8::MAX as u16).to_string(), theme.accent_style()),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(Span::styled(
            format!(" {} \u{b7} set   {} \u{b7} cancel", kb.action, kb.exit),
            theme.dim_style(),
        )),
    ];

    super::render(frame, area, theme, "Slave", 36, lines);
}
