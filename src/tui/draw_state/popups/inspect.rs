use crate::app::App;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App) {
    let (_, entries) = app.inspect_lines();

    const NAME: usize = 9;
    const VALUE: usize = 21;

    let mut lines: Vec<Line> = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            " no data read yet",
            theme.dim_style(),
        )));
    } else {
        let rows = entries.len().div_ceil(2);
        let cell = |i: usize| -> [Span<'static>; 2] {
            let (name, value) = &entries[i];
            let value: String = value.chars().take(VALUE).collect();
            [
                Span::styled(format!(" {name:<NAME$} "), theme.dim_style()),
                Span::styled(format!("{value:<VALUE$} "), theme.base()),
            ]
        };
        for r in 0..rows {
            let mut spans = cell(r).to_vec();
            let right = r + rows;
            if right < entries.len() {
                spans.push(Span::raw(" "));
                spans.extend(cell(right));
            }
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} move \u{b7} r refresh \u{b7} esc close",
        theme.dim_style(),
    )));

    let width = ((NAME + VALUE + 3) as u16) * 2 + 3;
    super::render(frame, area, theme, "Inspect", width, lines);
}
