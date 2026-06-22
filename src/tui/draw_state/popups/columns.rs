use crate::app::App;
use crate::input::KeyCode;
use crate::state::ColumnsParams;
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

    let query_line = Line::from(vec![
        Span::styled(" > ", theme.accent_style()),
        Span::styled(params.query.clone(), theme.base()),
        Span::styled("_", theme.accent_style()),
        Span::styled(format!("   ({count})"), theme.dim_style()),
    ]);

    let mut lines: Vec<Line> = vec![query_line, Line::default()];

    if count == 0 {
        lines.push(Line::from(Span::styled(
            " no matching columns",
            theme.dim_style(),
        )));
    } else {
        let rows = count.div_ceil(2);
        let cell = |i: usize| -> Span<'static> {
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
            Span::styled(format!(" {mark} {:<CELL$} ", column.name()), style)
        };

        for r in 0..rows {
            let mut spans = vec![cell(r)];
            let right = r + rows;
            if right < count {
                spans.push(Span::raw(" "));
                spans.push(cell(right));
            }
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::default());
    let kb = &app.config.keybinds;
    let footer = [
        Hint::pair(kb.move_down, KeyCode::Right, "Move"),
        Hint::key(kb.action, "Toggle"),
        Hint::key(kb.exit, "Close"),
    ];
    let width = ((CELL as u16 + 6) * 2 + 3).max(hints::width(&footer) as u16);
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Columns", width, lines);
}
