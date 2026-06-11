use crate::app::App;
use crate::config::Column;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, selected: u16) {
    let columns = Column::ALL;
    let count = columns.len();

    let rows = count.div_ceil(2);
    const CELL: usize = 14;

    let cell = |i: usize| -> Span<'static> {
        let column = columns[i];
        let on = app.interpreter.is_enabled(column);
        let mark = if on { "[x]" } else { "[ ]" };
        let style = if i as u16 == selected {
            theme.selected_style()
        } else if on {
            theme.base()
        } else {
            theme.dim_style()
        };
        Span::styled(format!(" {mark} {:<CELL$} ", column.name()), style)
    };

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 1);
    for r in 0..rows {
        let mut spans = vec![cell(r)];
        let right = r + rows;
        if right < count {
            spans.push(Span::raw(" "));
            spans.push(cell(right));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} move \u{b7} space toggle \u{b7} esc close",
        theme.dim_style(),
    )));

    let width = (CELL as u16 + 6) * 2 + 3;
    super::render(frame, area, theme, "Columns", width, lines);
}
