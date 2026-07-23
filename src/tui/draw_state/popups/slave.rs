use crate::config::Keybinds;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, value: u16) {
    let lines = vec![
        Line::from(vec![
            Span::styled("ID: ", theme.dim_style()),
            Span::styled(value.min(u8::MAX as u16).to_string(), theme.accent_style()),
            super::cursor_span(theme),
        ]),
        hints::footer(
            theme,
            [Hint::key(kb.action, "Set"), Hint::key(kb.exit, "Cancel")],
        ),
    ];

    super::render(frame, area, theme, "Slave", 36, lines);
}
