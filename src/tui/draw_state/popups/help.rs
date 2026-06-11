use crate::constants::keybind;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme) {
    use keybind::*;
    let entries: &[(String, &str)] = &[
        (format!("{MOVE_UP}/{MOVE_DOWN}"), "Move cursor"),
        ("PgUp/Dn".to_string(), "Jump page"),
        (format!("{ACTION}"), "Read at cursor"),
        (format!("{REFRESH}"), "Refresh"),
        ("space".to_string(), "Pause/resume"),
        (format!("{TOGGLE}"), "Switch reg type"),
        (format!("{WORD_ORDER}"), "Cycle word order"),
        (format!("{SWITCH_VIEW}"), "Cycle panel"),
        (format!("{JUMP}"), "Go to addr/label"),
        (format!("{CYCLE_POSITION}"), "Prev position"),
        (format!("{COPY_ADDRESS}"), "Copy address"),
        (format!("{GRAPH}"), "Value graph"),
        (format!("{INSPECT}"), "Inspect register"),
        (format!("{WRITE}"), "Write register"),
        (format!("{SLAVE}"), "Set slave id"),
        (format!("{DISCOVERY}"), "Switch device"),
        (format!("{PIN}"), "Add/remove pin"),
        (format!("{LABEL}"), "Label register"),
        (format!("{CUSTOM}"), "Custom rule"),
        (format!("{COLUMNS}"), "Toggle columns"),
        (format!("{DUMP}"), "Dump read data"),
        (format!("{SETTINGS}"), "Settings"),
        (format!("{LOGS}"), "View write log"),
        (format!("{APP_LOGS}"), "App log"),
    ];

    const COLS: usize = 3;
    let rows = entries.len().div_ceil(COLS);

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 3 + 1);
    lines.push(Line::default());

    for r in 0..rows {
        let mut spans = Vec::new();
        for c in 0..COLS {
            let idx = r * COLS + c;
            if let Some((key, desc)) = entries.get(idx) {
                spans.push(Span::styled(format!(" {key:<7} "), theme.accent_style()));
                spans.push(Span::styled(format!("{desc:<17}"), theme.base()));
            }
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Graph might disallow some operations",
        theme.dim_style(),
    )));
    lines.push(Line::from(Span::styled(
        format!(" {SETTINGS} \u{b7} settings (save / clear)   {EXIT} \u{b7} quit"),
        theme.dim_style(),
    )));

    let width = (COLS as u16 * 26) + 3;
    super::render(frame, area, theme, "Help", width, lines);
}
