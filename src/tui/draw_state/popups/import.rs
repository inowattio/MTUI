use crate::config::Keybinds;
use crate::state::ImportParams;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    params: &ImportParams,
) {
    let mut lines = vec![Line::from(Span::styled(
        "Found importable data on the clipboard:".to_string(),
        theme.base(),
    ))];

    for (count, noun) in [
        (params.pins, "pinned register"),
        (params.labels, "label"),
        (params.rules, "custom rule"),
    ] {
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            lines.push(Line::from(Span::styled(
                format!("  \u{2022} {count} {noun}{plural}"),
                theme.base(),
            )));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "Entries at the same address are overwritten.".to_string(),
        theme.dim_style(),
    )));
    lines.push(hints::footer(
        theme,
        &[
            Hint::key(kb.action, "Import"),
            Hint::keys(format!("Backspace/{}", hints::glyph(kb.exit)), "Cancel"),
        ],
    ));

    super::render(frame, area, theme, "Paste import", 60, lines);
}
