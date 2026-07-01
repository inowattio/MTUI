use crate::app::App;
use crate::tui::hints::{self, Hint};
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
        let cell = |i: usize| -> Vec<Span<'static>> {
            let (name, value) = &entries[i];
            let value: String = value.chars().take(VALUE).collect();
            vec![
                Span::styled(format!(" {name:<NAME$} "), theme.dim_style()),
                Span::styled(format!("{value:<VALUE$} "), theme.base()),
            ]
        };
        lines.extend(super::two_column(entries.len(), cell));
    }
    lines.push(Line::default());
    let kb = &app.config.keybinds;
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(kb.move_up, kb.move_down, "Move"),
            Hint::key(kb.refresh, "Refresh"),
            Hint::key(kb.word_order, "Cycle order"),
            Hint::key(kb.exit, "Close"),
        ],
    ));

    let width = ((NAME + VALUE + 3) as u16) * 2 + 3;
    super::render(frame, area, theme, "Inspect", width, lines);
}
