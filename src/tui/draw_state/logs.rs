use crate::app::App;
use crate::logger::{self, LogLevel};
use crate::state::LogViewParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(params: &LogViewParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let entries = logger::snapshot();
    let len = entries.len();

    let visible = area.height.saturating_sub(1).max(1);
    app.visible_rows.set(visible);
    let visible = visible as usize;

    let max_top = len.saturating_sub(visible);
    let top = if params.follow {
        max_top
    } else {
        (params.top as usize).min(max_top)
    };
    let end = (top + visible).min(len);

    let shown = if len == 0 { 0 } else { end };
    let kb = &app.config.keybinds;
    let header = Line::from(Span::styled(
        format!(
            " {shown}/{len} events   {}/{} scroll \u{b7} {} back",
            kb.move_up, kb.move_down, kb.exit
        ),
        theme.dim_style(),
    ));

    let mut lines = vec![header];

    if len == 0 {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            " (no activity yet)",
            theme.dim_style(),
        )));
    } else {
        for entry in entries.iter().skip(top).take(end - top) {
            let (tag, tag_style) = match entry.level {
                LogLevel::Info => ("INFO", theme.ok_style()),
                LogLevel::Warn => ("WARN", theme.warn_style()),
                LogLevel::Error => ("ERR ", theme.err_style()),
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.time.format("%H:%M:%S")),
                    theme.dim_style(),
                ),
                Span::styled(format!("{tag} "), tag_style),
                Span::styled(entry.message.clone(), theme.base()),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}
