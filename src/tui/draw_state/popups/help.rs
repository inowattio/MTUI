use crate::config::Keybinds;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds) {
    let entries: &[(String, &str)] = &[
        (format!("{}/{}", kb.move_up, kb.move_down), "Move cursor"),
        ("PgUp/Dn".to_string(), "Jump page"),
        (format!("{}", kb.action), "Read at cursor"),
        (format!("{}", kb.refresh), "Refresh"),
        (format!("{}", kb.pause), "Pause/resume"),
        (format!("{}", kb.toggle), "Switch reg type"),
        (format!("{}", kb.word_order), "Cycle word order"),
        (format!("{}", kb.switch_view), "Cycle panel"),
        (format!("{}", kb.jump), "Go to addr/label"),
        (format!("{}", kb.cycle_position), "Prev position"),
        (format!("{}", kb.copy_address), "Copy address"),
        (format!("{}", kb.graph), "Value graph"),
        (format!("{}", kb.inspect), "Inspect register"),
        (format!("{}", kb.write), "Write register"),
        (format!("{}", kb.slave), "Set slave id"),
        (format!("{}", kb.discovery), "Switch device"),
        (format!("{}", kb.pin), "Add/remove pin"),
        (format!("{}", kb.label), "Label register"),
        (format!("{}", kb.custom), "Custom rule"),
        (format!("{}", kb.columns), "Toggle columns"),
        (format!("{}", kb.dump), "Dump read data"),
        (format!("{}", kb.settings), "Settings"),
        (format!("{}", kb.logs), "View write log"),
        (format!("{}", kb.app_logs), "App log"),
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
        format!(
            " {} \u{b7} settings (save / clear)   {} \u{b7} quit",
            kb.settings, kb.exit
        ),
        theme.dim_style(),
    )));

    let width = (COLS as u16 * 26) + 3;
    super::render(frame, area, theme, "Help", width, lines);
}
