use crate::app::App;
use crate::config::{KeybindAction, Keybinds};
use crate::state::HelpParams;
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
    app: &App,
    help: &HelpParams,
) {
    let entries: Vec<(KeybindAction, String, &'static str)> = KeybindAction::ALL
        .iter()
        .copied()
        .map(|a| (a, kb.get(a).to_string(), a.label()))
        .collect();

    let matches = app.help_matches();
    let selected = app.help_selected_action();

    let desc_w = entries
        .iter()
        .map(|(_, _, d)| d.chars().count())
        .max()
        .unwrap_or(0);
    let cell_w = |key_w: usize| (1 + key_w + 1) + (desc_w + 2);

    let max_key = entries
        .iter()
        .map(|(_, k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let cols = ((area.width.saturating_sub(2) as usize) / cell_w(max_key).max(1))
        .clamp(1, 3)
        .min(entries.len().max(1));
    let rows = entries.len().div_ceil(cols);

    let col_key_w: Vec<usize> = (0..cols)
        .map(|c| {
            (0..rows)
                .filter_map(|r| entries.get(r * cols + c))
                .map(|(_, k, _)| k.chars().count())
                .max()
                .unwrap_or(0)
                .max(3)
        })
        .collect();

    let query_line = super::query_line(theme, &help.query, matches.len());

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 4);
    lines.push(query_line);
    lines.push(Line::default());

    for r in 0..rows {
        let mut spans = Vec::new();
        for (c, &kw) in col_key_w.iter().enumerate() {
            let idx = r * cols + c;
            if let Some((action, key, desc)) = entries.get(idx) {
                let (key_style, desc_style) = if Some(*action) == selected {
                    (theme.selected_style(), theme.selected_style())
                } else if matches.contains(action) {
                    (theme.accent_style(), theme.base())
                } else {
                    (theme.dim_style(), theme.dim_style())
                };
                spans.push(Span::styled(format!(" {key:<kw$} "), key_style));
                spans.push(Span::styled(
                    format!("{desc:<width$}", width = desc_w + 2),
                    desc_style,
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    let footer = [
        Hint::pair(kb.move_up, kb.move_down, "Select"),
        Hint::key(kb.action, "Run"),
        Hint::key(kb.exit, "Close"),
    ];
    let grid_w = 2 + col_key_w.iter().map(|&kw| cell_w(kw)).sum::<usize>();
    let width = grid_w.max(hints::width(&footer)) as u16;
    lines.push(Line::default());
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Help", width, lines);
}
