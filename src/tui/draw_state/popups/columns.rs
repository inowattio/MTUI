use crate::app::App;
use crate::input::KeyCode;
use crate::state::ColumnsParams;
use crate::tui::draw_state::dim_line;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    app: &App,
    params: &ColumnsParams,
) {
    let matches = app.column_matches();
    let count = matches.len();
    let selected = params.selected as usize;

    const CELL: usize = 14;

    let query_line = super::query_line(theme, &params.query, count);

    let mut lines: Vec<Line> = vec![query_line, Line::default()];

    if count == 0 {
        lines.push(dim_line(theme, " no matching columns"));
    } else {
        let cell = |i: usize| -> Vec<Span<'static>> {
            let column = matches[i];
            let on = app.interpreter.is_enabled(column);
            let mark = if on { "[x]" } else { "[ ]" };
            let style = if i == selected {
                theme.selected_style()
            } else if on {
                theme.base()
            } else {
                theme.dim_style()
            };
            vec![Span::styled(
                format!(" {mark} {:<CELL$} ", column.name()),
                style,
            )]
        };
        lines.extend(super::two_column(count, cell));
    }

    lines.push(Line::default());
    let kb = &app.config.keybinds;
    let footer = [
        Hint::pair(kb.move_down, KeyCode::Right, "Move"),
        Hint::key(kb.action, "Toggle"),
        Hint::key(kb.exit, "Close"),
    ];
    let width = hints::min_width((CELL as u16 + 6) * 2 + 3, &footer);
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Columns", width, lines);
}
