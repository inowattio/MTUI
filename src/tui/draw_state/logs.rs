use crate::app::App;
use crate::frames::{self, Direction};
use crate::logger::{self, LogLevel};
use crate::state::{LogViewParams, ScreenLayout};
use crate::tui::draw_state::dim_line;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

struct Row {
    head: [Span<'static>; 2],
    text: String,
}

impl Row {
    fn head_width(&self) -> usize {
        self.head.iter().map(Span::width).sum()
    }
}

fn event_rows(theme: &Theme) -> Vec<Row> {
    logger::snapshot()
        .into_iter()
        .map(|entry| {
            let (tag, tag_style) = match entry.level {
                LogLevel::Info => ("INFO", theme.ok_style()),
                LogLevel::Warn => ("WARN", theme.warn_style()),
                LogLevel::Error => ("ERR ", theme.err_style()),
            };
            Row {
                head: [
                    Span::styled(
                        format!(" {} ", entry.time.format("%H:%M:%S")),
                        theme.dim_style(),
                    ),
                    Span::styled(format!("{tag} "), tag_style),
                ],
                text: entry.message,
            }
        })
        .collect()
}

fn frame_rows(theme: &Theme) -> Vec<Row> {
    frames::snapshot()
        .into_iter()
        .map(|frame| {
            let tag_style = match frame.direction {
                Direction::Tx => theme.accent_style(),
                Direction::Rx => theme.ok_style(),
            };
            Row {
                head: [
                    Span::styled(
                        format!(" {} ", frame.time.format("%H:%M:%S%.3f")),
                        theme.dim_style(),
                    ),
                    Span::styled(format!("{} ", frame.direction.tag()), tag_style),
                ],
                text: frame.hex(),
            }
        })
        .collect()
}

fn entry_rows(
    row: &Row,
    theme: &Theme,
    wrap: bool,
    msg_width: usize,
    h_off: usize,
) -> Vec<Line<'static>> {
    if !wrap {
        let shown: String = row.text.chars().skip(h_off).collect();
        let mut spans = row.head.to_vec();
        spans.push(Span::styled(shown, theme.base()));
        return vec![Line::from(spans)];
    }

    let chars: Vec<char> = row.text.chars().collect();
    let indent = row.head_width();
    chars
        .chunks(msg_width.max(1))
        .map(|piece| piece.iter().collect::<String>())
        .enumerate()
        .map(|(i, piece)| {
            if i == 0 {
                let mut spans = row.head.to_vec();
                spans.push(Span::styled(piece, theme.base()));
                Line::from(spans)
            } else {
                Line::from(vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(piece, theme.base()),
                ])
            }
        })
        .collect()
}

/// The "shown/total events" counter for the top bar; relies on the row count
/// published by the previous draw, like the other status cells.
pub fn counter(params: &LogViewParams, app: &App) -> String {
    let len = if params.frames {
        frames::count()
    } else {
        logger::count()
    };
    let visible = app.layout.visible_rows.max(1) as usize;
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
    let noun = if params.frames { "frames" } else { "events" };
    format!("{shown}/{len} {noun}")
}

pub fn draw(
    params: &LogViewParams,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    layout: &mut ScreenLayout,
) {
    let entries = if params.frames {
        frame_rows(theme)
    } else {
        event_rows(theme)
    };
    let len = entries.len();

    let visible = area.height.max(1);
    layout.visible_rows = visible;
    let visible = visible as usize;

    let max_top = len.saturating_sub(visible);
    let top = if params.follow {
        max_top
    } else {
        (params.top as usize).min(max_top)
    };
    let end = (top + visible).min(len);

    let prefix_w = entries.first().map_or(0, Row::head_width);
    let msg_width = (area.width as usize).saturating_sub(prefix_w).max(10);
    let msg_width = if params.frames {
        (msg_width / 3 * 3).max(3)
    } else {
        msg_width
    };
    let h_max = if params.wrap {
        0
    } else {
        entries
            .iter()
            .skip(top)
            .take(end - top)
            .map(|e| e.text.chars().count())
            .max()
            .unwrap_or(0)
            .saturating_sub(msg_width) as u16
    };
    layout.h_max_offset = h_max;
    let h_off = params.h_offset.min(h_max) as usize;

    let mut lines: Vec<Line> = Vec::new();

    if len == 0 {
        let empty = if params.frames {
            " (no frames yet)"
        } else {
            " (no activity yet)"
        };
        lines.push(dim_line(theme, empty));
    } else if params.wrap && params.follow {
        let mut rows: Vec<Line> = entries
            .iter()
            .rev()
            .flat_map(|entry| {
                entry_rows(entry, theme, true, msg_width, 0)
                    .into_iter()
                    .rev()
            })
            .take(visible)
            .collect();
        rows.reverse();
        lines.extend(rows);
    } else {
        lines.extend(
            entries
                .iter()
                .skip(top)
                .flat_map(|entry| entry_rows(entry, theme, params.wrap, msg_width, h_off))
                .take(visible),
        );
    }

    frame.render_widget(Paragraph::new(lines), area);
}
