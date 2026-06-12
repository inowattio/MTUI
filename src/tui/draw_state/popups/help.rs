use crate::config::Keybinds;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds) {
    let entries: &[(String, &str)] = &[
        (format!("{}/{}", kb.move_up, kb.move_down), "Move cursor"),
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
        (format!("{}", kb.sweep), "Sweep"),
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
        (format!("{}/{}", kb.page_up, kb.page_down), "Jump page"),
    ];

    let desc_w = entries
        .iter()
        .map(|(_, d)| d.chars().count())
        .max()
        .unwrap_or(0);

    let cell_w = |key_w: usize| (1 + key_w + 1) + (desc_w + 2);

    let footer = format!(
        " {} \u{b7} settings (save / clear)   {} \u{b7} quit",
        kb.settings, kb.exit
    );

    let max_key = entries
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let cols = ((area.width.saturating_sub(2) as usize) / cell_w(max_key).max(1))
        .clamp(1, 3)
        .min(entries.len());
    let rows = entries.len().div_ceil(cols);

    let col_key_w: Vec<usize> = (0..cols)
        .map(|c| {
            (0..rows)
                .filter_map(|r| entries.get(r * cols + c))
                .map(|(k, _)| k.chars().count())
                .max()
                .unwrap_or(0)
                .max(3)
        })
        .collect();

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 3);
    lines.push(Line::default());

    for r in 0..rows {
        let mut spans = Vec::new();
        for (c, &kw) in col_key_w.iter().enumerate() {
            let idx = r * cols + c;
            if let Some((key, desc)) = entries.get(idx) {
                spans.push(Span::styled(format!(" {key:<kw$} "), theme.accent_style()));
                spans.push(Span::styled(
                    format!("{desc:<width$}", width = desc_w + 2),
                    theme.base(),
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(footer.clone(), theme.dim_style())));

    let grid_w = 2 + col_key_w.iter().map(|&kw| cell_w(kw)).sum::<usize>();
    let width = grid_w.max(footer.chars().count() + 2) as u16;
    super::render(frame, area, theme, "Help", width, lines);
}
