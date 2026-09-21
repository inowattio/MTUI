mod about;
mod columns;
mod custom;
mod device_id;
mod discovery;
mod dump;
mod help;
mod import;
mod inspect;
mod label;
mod logs;
mod raw;
mod search;
mod stats;
mod sweep_config;
mod unit;
mod unsaved;
mod write;

use crate::app::App;
use crate::state::Popup;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Clear, Paragraph};

pub fn draw_popup(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, popup: &Popup) {
    let kb = &app.config.keybinds;
    match popup {
        Popup::Discovery(d) => discovery::draw(d, app, frame, area, theme),
        Popup::Help(h) => help::draw(frame, area, theme, kb, app, h),
        Popup::Dump(d) => dump::draw(frame, area, theme, app.read_count(), &d.result),
        Popup::Search(s) => search::draw(frame, area, theme, app, s),
        Popup::Label(l) => label::draw(frame, area, theme, l),
        Popup::Custom(c) => custom::draw(frame, area, theme, app, c),
        Popup::Columns(params) => columns::draw(frame, area, theme, app, params),
        Popup::Write(write) => {
            write::draw(
                frame,
                area,
                theme,
                kb,
                write,
                app.write_custom_preview(write),
            );
        }
        Popup::Unit(params) => {
            unit::draw(frame, area, theme, kb, params, app.config.device.unit_id)
        }
        Popup::SweepConfig(s) => sweep_config::draw(frame, area, theme, *s, app.sweep.active),
        Popup::Logs(logs) => logs::draw(frame, area, theme, kb, logs, app.config.write_log.enabled),
        Popup::Inspect(mode) => inspect::draw(frame, area, theme, app, *mode),
        Popup::About => about::draw(frame, area, theme),
        Popup::Stats => stats::draw(frame, area, theme, app),
        Popup::DeviceId(params) => device_id::draw(frame, area, theme, app, params),
        Popup::Raw(params) => raw::draw(frame, area, theme, params),
        Popup::Import(params) => import::draw(frame, area, theme, params),
        Popup::CycleConfig | Popup::Quit => {
            unsaved::draw(frame, area, theme, matches!(popup, Popup::Quit))
        }
    }
}

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    width: u16,
    lines: Vec<Line<'static>>,
) {
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(theme.background))
            .block(theme.panel(&format!(" {title}")).borders(Borders::ALL)),
        rect,
    );
}

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

pub(super) fn query_line(theme: &Theme, query: &str, count: usize) -> Line<'static> {
    Line::from(vec![
        Span::styled(" > ", theme.accent_style()),
        Span::styled(query.to_string(), theme.base()),
        cursor_span(theme),
        Span::styled(format!("   ({count})"), theme.dim_style()),
    ])
}

pub(super) fn cursor_span(theme: &Theme) -> Span<'static> {
    Span::styled("_", theme.accent_style())
}

pub(super) fn window(top: usize, visible: usize, len: usize) -> (usize, usize) {
    let top = top.min(len.saturating_sub(1));
    let end = (top + visible).min(len);
    (top, end)
}

pub(super) fn push_status(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    status: Option<&crate::state::StatusMessage>,
) {
    if let Some(status) = status {
        lines.push(Line::default());
        let mut line = theme.status_line(status);
        line.spans.insert(0, Span::raw(" "));
        lines.push(line);
    }
}

pub(super) fn push_footer<const N: usize>(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    items: [Hint; N],
) {
    lines.push(Line::default());
    lines.push(hints::footer(theme, items));
}

pub(super) fn two_column(
    count: usize,
    mut cell: impl FnMut(usize) -> Vec<Span<'static>>,
) -> Vec<Line<'static>> {
    let rows = count.div_ceil(2);
    (0..rows)
        .map(|r| {
            let mut spans = cell(r);
            let right = r + rows;
            if right < count {
                spans.push(Span::raw(" "));
                spans.extend(cell(right));
            }
            Line::from(spans)
        })
        .collect()
}

pub(super) fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(crate::constants::ELLIPSIS.len());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(crate::constants::ELLIPSIS);
    out
}
