use crate::config::Keybinds;
use crate::state::SearchParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, search: &SearchParams) {
    let visible = 10usize;
    let len = search.matches.len();
    let top = (search.top as usize).min(len.saturating_sub(1));
    let end = (top + visible).min(len);

    let query_line = Line::from(vec![
        Span::styled(" index/label: ", theme.dim_style()),
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
            let style = if i as u16 == search.selected {
                theme.selected_style()
            } else {
                theme.base()
            };
            lines.push(Line::from(Span::styled(row, style)));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(
            " address or label \u{b7} {}/{} select \u{b7} {} go \u{b7} {} close",
            kb.move_up, kb.move_down, kb.action, kb.exit
        ),
        theme.dim_style(),
    )));

    super::render(frame, area, theme, "Go to", 54, lines);
}
