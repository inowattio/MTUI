use crate::input::KeyCode;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, settings: KeyCode) {
    let footer = [
        Hint::key(KeyCode::Enter, "OK"),
        Hint::key(settings, "Settings"),
    ];
    let lines = vec![
        Line::default(),
        Line::from(Span::styled(
            " There are no library sources enabled.",
            theme.base(),
        )),
        Line::from(Span::styled(
            " Turn them on in Settings / Libraries.",
            theme.dim_style(),
        )),
        Line::default(),
        hints::footer(theme, footer),
    ];

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Library", width, lines);
}
