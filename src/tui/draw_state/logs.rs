use crate::app::App;
use crate::logger::{self, LogEntry, LogLevel};
use crate::state::LogViewParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

// " HH:MM:SS " + "TAG " before each message.
const PREFIX_W: usize = 15;

fn entry_rows(
    entry: &LogEntry,
    theme: &Theme,
    wrap: bool,
    msg_width: usize,
    h_off: usize,
) -> Vec<Line<'static>> {
    let (tag, tag_style) = match entry.level {
        LogLevel::Info => ("INFO", theme.ok_style()),
        LogLevel::Warn => ("WARN", theme.warn_style()),
        LogLevel::Error => ("ERR ", theme.err_style()),
    };
    let head = vec![
        Span::styled(
            format!(" {} ", entry.time.format("%H:%M:%S")),
            theme.dim_style(),
        ),
        Span::styled(format!("{tag} "), tag_style),
    ];

    if !wrap {
        let shown: String = entry.message.chars().skip(h_off).collect();
        let mut spans = head;
        spans.push(Span::styled(shown, theme.base()));
        return vec![Line::from(spans)];
    }

    let chars: Vec<char> = entry.message.chars().collect();
    chars
        .chunks(msg_width.max(1))
        .map(|piece| piece.iter().collect::<String>())
        .enumerate()
        .map(|(i, piece)| {
            if i == 0 {
                let mut spans = head.clone();
                spans.push(Span::styled(piece, theme.base()));
                Line::from(spans)
            } else {
                Line::from(vec![
                    Span::raw(" ".repeat(PREFIX_W)),
                    Span::styled(piece, theme.base()),
                ])
            }
        })
        .collect()
}

/// The "shown/total events" counter for the top bar; relies on the row count
/// published by the previous draw, like the other status cells.
pub fn counter(params: &LogViewParams, app: &App) -> String {
    let len = logger::count();
    let visible = app.visible_rows.get().max(1) as usize;
    let max_top = len.saturating_sub(visible);
    let top = if params.follow {
        max_top
    } else {
        (params.top as usize).min(max_top)
    };
    let shown = if len == 0 {
        0
    } else {
        (top + visible).min(len)
    };
    format!("{shown}/{len} events")
}

pub fn draw(params: &LogViewParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let entries = logger::snapshot();
    let len = entries.len();

    let visible = area.height.max(1);
    app.visible_rows.set(visible);
    let visible = visible as usize;

    let max_top = len.saturating_sub(visible);
    let top = if params.follow {
        max_top
    } else {
        (params.top as usize).min(max_top)
    };
    let end = (top + visible).min(len);

    let msg_width = (area.width as usize).saturating_sub(PREFIX_W).max(10);
    let h_max = if params.wrap {
        0
    } else {
        entries
            .iter()
            .skip(top)
            .take(end - top)
            .map(|e| e.message.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(msg_width) as u16
    };
    app.h_max_offset.set(h_max);
    let h_off = params.h_offset.min(h_max) as usize;

    let mut lines: Vec<Line> = Vec::new();

    if len == 0 {
        lines.push(Line::from(Span::styled(
            " (no activity yet)",
            theme.dim_style(),
        )));
    } else if params.wrap && params.follow {
        // Fill from the bottom so the newest entry is fully visible
        let mut rows: Vec<Line> = Vec::new();
        'fill: for entry in entries.iter().rev() {
            for row in entry_rows(entry, theme, true, msg_width, 0)
                .into_iter()
                .rev()
            {
                rows.push(row);
                if rows.len() >= visible {
                    break 'fill;
                }
            }
        }
        rows.reverse();
        lines.extend(rows);
    } else {
        'take: for entry in entries.iter().skip(top) {
            for row in entry_rows(entry, theme, params.wrap, msg_width, h_off) {
                lines.push(row);
                if lines.len() >= visible {
                    break 'take;
                }
            }
        }
    }

    frame.render_widget(Paragraph::new(lines), area);
}
