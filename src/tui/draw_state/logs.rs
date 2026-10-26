use crate::app::App;
use crate::config::AddressMode;
use crate::frames::decode::{self, Field, Tone};
use crate::frames::{self, Direction};
use crate::logger::{self, LogLevel};
use crate::state::{LogMode, LogViewParams, ScreenLayout};
use crate::tui::draw_state::dim_line;
use crate::tui::draw_state::popups::truncate;
use crate::tui::theme::Theme;
use crate::writes_log::WriteEntry;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

const DETAIL_W: u16 = 46;
const DIVIDER_W: u16 = 2;
const DETAIL_MIN_TOTAL: u16 = 100;
const HEX_W: usize = 11;

const TIME_W: usize = 23;
const UNIT_W: usize = 5;
const ADDR_W: usize = 5;
const TYPE_W: usize = 8;
const FUNC_W: usize = 4;
const PREV_W: usize = 10;
const GAP: &str = "  ";

struct Row {
    head: Vec<Span<'static>>,
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
                head: vec![
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

fn frame_rows(snapshot: &[frames::Frame], theme: &Theme) -> Vec<Row> {
    snapshot
        .iter()
        .map(|frame| {
            let tag_style = match frame.direction {
                Direction::Tx => theme.accent_style(),
                Direction::Rx => theme.ok_style(),
            };
            Row {
                head: vec![
                    Span::styled(
                        format!(" {} ", frame.time.format("%H:%M:%S%.3f")),
                        theme.dim_style(),
                    ),
                    Span::styled(format!("{} ", frame.direction.tag()), tag_style),
                    Span::styled(format!("{} ", frame.direction.arrow()), theme.dim_style()),
                ],
                text: frame.hex(),
            }
        })
        .collect()
}

fn write_rows(entries: &[WriteEntry], theme: &Theme, address_mode: AddressMode) -> Vec<Row> {
    entries
        .iter()
        .map(|entry| {
            let time = entry.timestamp.replacen('T', " ", 1);
            Row {
                head: vec![
                    Span::styled(
                        format!(" {:<TIME_W$}{GAP}", truncate(&time, TIME_W)),
                        theme.dim_style(),
                    ),
                    Span::styled(format!("{:>UNIT_W$}{GAP}", entry.unit), theme.base()),
                    Span::styled(
                        format!("{:>ADDR_W$}{GAP}", address_mode.display(entry.address)),
                        theme.base(),
                    ),
                    Span::styled(
                        format!("{:<TYPE_W$}{GAP}", truncate(&entry.kind, TYPE_W)),
                        theme.accent_style(),
                    ),
                    Span::styled(
                        format!("{:<FUNC_W$}{GAP}", entry.display_function()),
                        theme.dim_style(),
                    ),
                    Span::styled(
                        format!(
                            "{:>PREV_W$}{GAP}",
                            truncate(&entry.display_previous(), PREV_W)
                        ),
                        theme.dim_style(),
                    ),
                ],
                text: entry.display_value(),
            }
        })
        .collect()
}

fn write_header(theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!(
            " {:<TIME_W$}{GAP}{:>UNIT_W$}{GAP}{:>ADDR_W$}{GAP}{:<TYPE_W$}{GAP}{:<FUNC_W$}{GAP}{:>PREV_W$}{GAP}VALUE",
            "TIME", "UNIT", "ADDR", "TYPE", "FUNC", "PREVIOUS"
        ),
        theme.header_style(),
    ))
}

fn highlight(spans: Vec<Span<'static>>, theme: &Theme, selected: bool) -> Line<'static> {
    if selected {
        let style = theme.selected_style();
        Line::from(
            spans
                .into_iter()
                .map(|span| span.patch_style(style))
                .collect::<Vec<_>>(),
        )
        .style(style)
    } else {
        Line::from(spans)
    }
}

fn entry_rows(
    row: &Row,
    theme: &Theme,
    wrap: bool,
    msg_width: usize,
    h_off: usize,
    selected: bool,
) -> Vec<Line<'static>> {
    if !wrap {
        let shown: String = row.text.chars().skip(h_off).collect();
        let mut spans = row.head.clone();
        spans.push(Span::styled(shown, theme.base()));
        return vec![highlight(spans, theme, selected)];
    }

    let chars: Vec<char> = row.text.chars().collect();
    let indent = row.head_width();
    chars
        .chunks(msg_width.max(1))
        .map(|piece| piece.iter().collect::<String>())
        .enumerate()
        .map(|(i, piece)| {
            let spans = if i == 0 {
                let mut spans = row.head.clone();
                spans.push(Span::styled(piece, theme.base()));
                spans
            } else {
                vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(piece, theme.base()),
                ]
            };
            highlight(spans, theme, selected)
        })
        .collect()
}

fn detail_lines(fields: &[Field], theme: &Theme, height: usize) -> Vec<Line<'static>> {
    let line = |field: &Field| match field.tone {
        Tone::Heading => Line::from(Span::styled(
            format!(" {}", field.label),
            theme.header_style(),
        )),
        tone => {
            let style = match tone {
                Tone::Good => theme.ok_style(),
                Tone::Bad => theme.err_style(),
                _ => theme.base(),
            };
            Line::from(vec![
                Span::styled(format!(" {:<HEX_W$}  ", field.hex()), theme.dim_style()),
                Span::styled(field.label.clone(), style),
            ])
        }
    };
    if fields.len() <= height {
        return fields.iter().map(line).collect();
    }
    let shown = height.saturating_sub(1);
    let mut lines: Vec<Line> = fields.iter().take(shown).map(line).collect();
    lines.push(dim_line(theme, format!(" … {} more", fields.len() - shown)));
    lines
}

/// The "shown/total events" counter for the top bar; relies on the row count
/// published by the previous draw, like the other status cells.
pub fn counter(params: &LogViewParams, app: &App) -> String {
    let len = match params.mode {
        LogMode::Events => logger::count(),
        LogMode::Frames => frames::count(),
        LogMode::Writes => params.writes.len(),
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
    format!("{shown}/{len} {}", params.mode.noun())
}

pub fn draw(
    params: &LogViewParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    layout: &mut ScreenLayout,
) {
    let address_mode = app.interpreter.address_mode();
    let shows_frames = params.mode == LogMode::Frames;
    let snapshot = if shows_frames {
        frames::snapshot()
    } else {
        Vec::new()
    };
    let entries = match params.mode {
        LogMode::Events => event_rows(theme),
        LogMode::Frames => frame_rows(&snapshot, theme),
        LogMode::Writes => write_rows(&params.writes, theme, address_mode),
    };
    let len = entries.len();
    let selected = if shows_frames && len > 0 {
        Some(if params.follow {
            len - 1
        } else {
            (params.selected as usize).min(len - 1)
        })
    } else {
        None
    };

    let area = if shows_frames && area.width >= DETAIL_MIN_TOTAL {
        let [list, divider, detail] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(DIVIDER_W),
            Constraint::Length(DETAIL_W),
        ])
        .areas(area);
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(theme.dim_style()),
            divider,
        );
        let lines = match selected {
            Some(index) => {
                let request = decode::request_for(&snapshot, index);
                let fields = decode::breakdown(&snapshot[index], request, address_mode);
                detail_lines(&fields, theme, detail.height as usize)
            }
            None => vec![dim_line(theme, " (nothing selected)")],
        };
        frame.render_widget(Paragraph::new(lines), detail);
        list
    } else {
        area
    };

    let area = if params.mode == LogMode::Writes {
        let [head, list] =
            Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
        frame.render_widget(
            Paragraph::new(vec![
                dim_line(theme, format!(" {}", params.writes_path)),
                write_header(theme),
            ]),
            head,
        );
        list
    } else {
        area
    };

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
    let msg_width = if shows_frames {
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
        let empty = match params.mode {
            LogMode::Events => " (no activity yet)",
            LogMode::Frames => " (no frames yet)",
            LogMode::Writes if app.config.write_log.enabled => " (no writes logged yet)",
            LogMode::Writes => " (logging is off, enable \"Log writes\" in settings)",
        };
        lines.push(dim_line(theme, empty));
    } else if params.wrap && params.follow {
        let mut rows: Vec<Line> = entries
            .iter()
            .enumerate()
            .rev()
            .flat_map(|(i, entry)| {
                entry_rows(entry, theme, true, msg_width, 0, selected == Some(i))
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
                .enumerate()
                .skip(top)
                .flat_map(|(i, entry)| {
                    entry_rows(
                        entry,
                        theme,
                        params.wrap,
                        msg_width,
                        h_off,
                        selected == Some(i),
                    )
                })
                .take(visible),
        );
    }

    frame.render_widget(Paragraph::new(lines), area);
}
