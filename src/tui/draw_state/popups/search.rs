use crate::config::Keybinds;
use crate::state::SearchParams;
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
    search: &SearchParams,
) {
    let visible = 10usize;
    let len = search.matches.len();
    let (top, end) = super::window(search.top as usize, visible, len);

    let query_line = Line::from(vec![
        Span::styled(" address/label: ", theme.dim_style()),
        Span::styled(search.query.clone(), theme.accent_style()),
        Span::styled("_", theme.accent_style()),
        Span::styled(format!("   ({len})"), theme.dim_style()),
    ]);

    let mut lines = vec![query_line, Line::default()];

    if search.matches.is_empty() {
        lines.push(Line::from(Span::styled(
            "Type an address or a label.",
            theme.dim_style(),
        )));
    } else {
        for i in top..end {
            let ((kind, address), text) = &search.matches[i];
            let row = format!("{address:>5}  {:<8} {text}", format!("{kind:?}"));
            let style = theme.line_style(i as u16 == search.selected);
            lines.push(Line::from(Span::styled(row, style)));
        }
    }

    let footer = [
        Hint::pair(kb.move_up, kb.move_down, "Select"),
        Hint::key(kb.action, "Go"),
        Hint::key(kb.exit, "Close"),
    ];
    lines.push(hints::more(theme, top, len.saturating_sub(end)));
    let width = hints::min_width(54, &footer);
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Go to", width, lines);
}
