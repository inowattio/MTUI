use crate::input::KeyCode;
use crate::state::ImportParams;
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, params: &ImportParams) {
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(
            " Found importable data on the clipboard:",
            theme.base(),
        )),
        Line::default(),
    ];

    for (count, noun) in [
        (params.pins, "pinned register"),
        (params.labels, "label"),
        (params.rules, "custom rule"),
    ] {
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            lines.push(Line::from(vec![
                Span::styled(format!("   {count} "), theme.accent_style()),
                Span::styled(format!("{noun}{plural}"), theme.base()),
            ]));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Entries at the same address are overwritten.",
        theme.dim_style(),
    )));
    super::push_footer(
        &mut lines,
        theme,
        [
            Hint::key(KeyCode::Enter, "Import"),
            Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel"),
        ],
    );

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Paste import", width, lines);
}
