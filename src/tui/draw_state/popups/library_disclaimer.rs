use crate::input::KeyCode;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines = Vec::with_capacity(12);
    for text in [
        "",
        " The library lists device maps from external, third-party sources.",
        "",
        " They are not made, reviewed or verified by MTUI. Addresses, scaling,",
        " types and writable registers may be wrong for your device or firmware.",
        "",
        " Check imported maps against the vendor's documentation, especially",
        " before writing to a device.",
    ] {
        lines.push(Line::from(Span::styled(text, theme.base())));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Browsing downloads files from these sources. This is shown only once.",
        theme.dim_style(),
    )));
    lines.push(Line::default());
    lines.push(hints::footer(
        theme,
        [
            Hint::key(KeyCode::Enter, "I understand"),
            Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel"),
        ],
    ));

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Library", width, lines);
}
