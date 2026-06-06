use crate::app::App;
use crate::config::Column;
use crate::constants::keybind;
use crate::state::{no_data_text, LabelParams, Popup, ReadPanel, ReadParams, SearchParams, WriteParams};
use crate::tui::theme::{spinner_frame, Theme};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

fn rows_to_table(
    title: &str,
    header: String,
    rows: &[String],
    changed: &[bool],
    selected: Option<usize>,
    theme: &Theme,
) -> Table<'static> {
    let table_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let style = if selected == Some(i) {
                theme.selected_style()
            } else if changed.get(i).copied().unwrap_or(false) {
                theme.changed_style()
            } else if i % 2 == 1 {
                theme.zebra_style()
            } else {
                theme.base()
            };
            Row::new([Cell::from(text.clone())]).style(style)
        })
        .collect();

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel(title))
}

fn main_table(
    params: &ReadParams,
    app: &App,
    visible: u16,
    header: String,
    theme: &Theme,
) -> Table<'static> {
    let mut table_rows = Vec::with_capacity(visible as usize);

    for i in 0..visible {
        let addr = params.window_start.saturating_add(i);
        let selected = addr == params.position;
        let zebra = i % 2 == 1;

        let cached = (addr >= params.data_start)
            .then(|| (addr - params.data_start) as usize)
            .and_then(|idx| {
                params
                    .main_rows
                    .get(idx)
                    .map(|text| (text.clone(), params.main_changed.get(idx).copied().unwrap_or(false)))
            });

        let (text, style) = match cached {
            Some((text, changed)) => {
                let style = if selected {
                    theme.selected_style()
                } else if changed {
                    theme.changed_style()
                } else if zebra {
                    theme.zebra_style()
                } else {
                    theme.base()
                };
                (text, style)
            }
            None => {
                let style = if selected {
                    theme.selected_style()
                } else {
                    theme.dim_style()
                };
                let label = app.label_text(params.register_type, addr);
                (app.interpreter.placeholder(addr, label.as_deref()), style)
            }
        };

        table_rows.push(Row::new([Cell::from(text)]).style(style));
    }

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel("Main data"))
}

fn pinned_table(params: &ReadParams, visible: u16, header: String, theme: &Theme) -> Table<'static> {
    let len = params.pinned_rows.len();
    let top = (params.pinned_top as usize).min(len.saturating_sub(1));
    let end = (top + visible as usize).min(len);

    let mut table_rows = Vec::with_capacity(end - top);
    for i in top..end {
        let style = if i as u16 == params.pinned_index {
            theme.selected_style()
        } else if params.pinned_changed.get(i).copied().unwrap_or(false) {
            theme.changed_style()
        } else if (i - top) % 2 == 1 {
            theme.zebra_style()
        } else {
            theme.base()
        };
        table_rows.push(Row::new([Cell::from(params.pinned_rows[i].clone())]).style(style));
    }

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel("Pinned"))
}

pub fn draw(
    params: &ReadParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let is_pinned = app
        .pinned_registers
        .iter()
        .any(|(kind, address)| kind == &params.register_type && *address == params.position);

    let show_ascii = app.interpreter.shows_ascii();
    let row_constraints = if show_ascii {
        vec![Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)]
    } else {
        vec![Constraint::Length(2), Constraint::Min(0)]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(area);

    let read_time = if params.loading {
        format!("{} reading", spinner_frame(app.frame))
    } else {
        params
            .read_duration
            .map(|d| format!("({d:.2?})"))
            .unwrap_or_default()
    };
    let mut info_spans = vec![
        Span::styled("Device: ", theme.dim_style()),
        Span::styled(device.to_string(), theme.base()),
        Span::styled("   @ ", theme.dim_style()),
        Span::styled(params.position.to_string(), theme.accent_style()),
    ];
    if is_pinned {
        info_spans.push(Span::styled(" (pinned)", theme.changed_style()));
    }
    info_spans.push(Span::styled(
        format!("   {:?}", params.register_type),
        theme.base(),
    ));
    info_spans.push(Span::styled("   order ", theme.dim_style()));
    info_spans.push(Span::styled(
        format!("{:?}   ", app.config.device.word_order),
        theme.base(),
    ));
    info_spans.push(Span::styled(read_time, theme.dim_style()));
    if app.paused {
        info_spans.push(Span::styled("   \u{23f8} paused", theme.warn_style()));
    } else if let Some(interval) = app.config.auto_update_interval_seconds {
        if !params.loading {
            let remaining = interval.saturating_sub(params.refresh_timer.elapsed().as_secs());
            info_spans.push(Span::styled(format!("   ⟳ {remaining}s"), theme.ok_style()));
        }
    }
    frame.render_widget(
        Paragraph::new(Line::from(info_spans)).alignment(Alignment::Left),
        rows[0],
    );

    let header = app.interpreter.header();

    let visible = rows[1].height.saturating_sub(3).max(1);
    app.visible_rows.set(visible);

    let ascii_string = match params.panel {
        ReadPanel::Main => {
            frame.render_widget(main_table(params, app, visible, header, theme), rows[1]);
            &params.ascii_string
        }
        ReadPanel::Pinned => {
            let table = if params.pinned_rows.is_empty() {
                let message = if app.pinned_registers.is_empty() {
                    "No pinned registers.".to_string()
                } else {
                    no_data_text()
                };
                rows_to_table("Pinned", header, &[message], &[], None, theme)
            } else {
                pinned_table(params, visible, header, theme)
            };
            frame.render_widget(table, rows[1]);
            &params.pinned_ascii_string
        }
    };

    if show_ascii {
        let ascii_line = Line::from(vec![
            Span::styled("ASCII: ", theme.dim_style()),
            Span::styled(format!("'{ascii_string}'"), theme.base()),
        ]);
        frame.render_widget(
            Paragraph::new(ascii_line)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            rows[2],
        );
    }

    if let Some(popup) = &params.popup {
        draw_popup(frame, area, theme, app, popup);
    }
}

fn draw_popup(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, popup: &Popup) {
    match popup {
        Popup::Help => draw_help(frame, area, theme),
        Popup::Save(s) => draw_confirm(
            frame,
            area,
            theme,
            "Save",
            "Save configuration (labels & pins) to file?",
            &s.result,
        ),
        Popup::Dump(d) => draw_confirm(
            frame,
            area,
            theme,
            "Dump",
            &format!("Dump {} read register(s) to a file?", app.read_count()),
            &d.result,
        ),
        Popup::Search(s) => draw_search(frame, area, theme, s),
        Popup::Label(l) => draw_label(frame, area, theme, l),
        Popup::Columns(selected) => draw_picker(frame, area, theme, app, *selected),
        Popup::Jump(target) => draw_jump(frame, area, theme, *target),
        Popup::Write(write) => draw_write(frame, area, theme, write),
    }
}

fn draw_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    use keybind::*;
    let entries: [(String, &str); 13] = [
        (format!("{MOVE_UP}/{MOVE_DOWN}"), "Move cursor"),
        (format!("{ACTION}"), "Read at cursor"),
        (format!("{REFRESH}"), "Refresh"),
        ("space".to_string(), "Pause / resume auto-refresh"),
        (format!("{TOGGLE}"), "Switch register type"),
        (format!("{SWITCH_VIEW}"), "Switch Main / Pinned"),
        (format!("{JUMP}"), "Jump to address"),
        (format!("{SEARCH}"), "Search labels"),
        (format!("{WRITE}"), "Write register"),
        (format!("{PIN}"), "Add / Remove pin"),
        (format!("{LABEL}"), "Label register"),
        (format!("{COLUMNS}"), "Toggle columns"),
        (format!("{DUMP}"), "Dump read data"),
    ];

    let mut lines: Vec<Line> = entries
        .iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled(format!(" {key:<8}"), theme.accent_style()),
                Span::styled(*desc, theme.base()),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(" {SAVE} \u{b7} save config to file   {EXIT} \u{b7} quit"),
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(40, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Help")), rect);
}

fn draw_confirm(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    prompt: &str,
    result: &Option<String>,
) {
    let mut lines = vec![
        Line::from(Span::styled(prompt.to_string(), theme.base())),
        Line::from(Span::styled(
            " enter \u{b7} confirm   esc \u{b7} cancel",
            theme.dim_style(),
        )),
    ];

    if let Some(result) = result {
        let style = if result.starts_with("Saved") || result.starts_with("Dumped") {
            theme.ok_style()
        } else if result.contains("failed") {
            theme.err_style()
        } else {
            theme.dim_style()
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(result.clone(), style)));
    }

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(60, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel(title)), rect);
}

fn draw_label(frame: &mut Frame, area: Rect, theme: &Theme, label: &LabelParams) {
    let (text, text_style) = if label.text.is_empty() {
        ("(empty - will remove)".to_string(), theme.dim_style())
    } else {
        (label.text.clone(), theme.base())
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Label ", theme.dim_style()),
            Span::styled(label.position.to_string(), theme.accent_style()),
            Span::styled(format!("  ({:?})", label.register_type), theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("Text: ", theme.dim_style()),
            Span::styled(text, text_style),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(Span::styled(
            " enter \u{b7} set (empty removes)   esc \u{b7} cancel",
            theme.dim_style(),
        )),
    ];

    if let Some(result) = &label.result {
        lines.push(Line::from(Span::styled(result.clone(), theme.err_style())));
    }

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(48, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Label")), rect);
}

fn draw_search(frame: &mut Frame, area: Rect, theme: &Theme, search: &SearchParams) {
    // Cap the visible match list so the popup stays compact.
    let visible = 10usize;
    let len = search.matches.len();
    let top = (search.top as usize).min(len.saturating_sub(1));
    let end = (top + visible).min(len);

    let query_line = Line::from(vec![
        Span::styled("Search: ", theme.dim_style()),
        Span::styled(search.query.clone(), theme.accent_style()),
        Span::styled("_", theme.accent_style()),
        Span::styled(format!("   ({len})"), theme.dim_style()),
    ]);

    let mut lines = vec![query_line, Line::default()];

    if search.matches.is_empty() {
        lines.push(Line::from(Span::styled("No matching labels.", theme.dim_style())));
    } else {
        for i in top..end {
            let ((kind, address), text) = &search.matches[i];
            let row = format!("{address:>5}  {:<8} {text}", format!("{kind:?}"));
            let style = if i as u16 == search.selected {
                theme.selected_style()
            } else {
                theme.base()
            };
            lines.push(Line::from(Span::styled(row, style)));
        }
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " type to filter \u{b7} \u{2191}/\u{2193} select \u{b7} enter jump \u{b7} esc close",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(54, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Search")), rect);
}

fn draw_write(frame: &mut Frame, area: Rect, theme: &Theme, write: &WriteParams) {
    let value = write
        .value
        .map_or_else(|| "(none)".to_string(), |n| n.to_string());

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Write to ", theme.dim_style()),
            Span::styled(write.position.to_string(), theme.accent_style()),
            Span::styled(format!("  [{:?}]", write.write_type), theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("Value: ", theme.dim_style()),
            Span::styled(value, theme.base()),
            Span::styled("_", theme.accent_style()),
        ]),
    ];

    if let Some(result) = &write.result {
        let style = if result.starts_with("Write OK") {
            theme.ok_style()
        } else if result.starts_with("Write failed") {
            theme.err_style()
        } else {
            theme.dim_style()
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(result.clone(), style)));
    }

    lines.push(Line::from(Span::styled(
        " enter \u{b7} write   w \u{b7} word/dword   - \u{b7} negate   esc \u{b7} close",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(58, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Write")), rect);
}

fn draw_jump(frame: &mut Frame, area: Rect, theme: &Theme, target: u16) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Address: ", theme.dim_style()),
            Span::styled(target.to_string(), theme.accent_style()),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(Span::styled(
            " enter \u{b7} go   esc \u{b7} cancel",
            theme.dim_style(),
        )),
    ];

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(36, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Jump")), rect);
}

fn draw_picker(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, selected: u16) {
    let mut lines: Vec<Line> = Column::ALL
        .iter()
        .enumerate()
        .map(|(i, &column)| {
            let on = app.interpreter.is_enabled(column);
            let mark = if on { "[x]" } else { "[ ]" };
            let style = if i as u16 == selected {
                theme.selected_style()
            } else if on {
                theme.base()
            } else {
                theme.dim_style()
            };
            Line::from(Span::styled(format!(" {mark} {}", column.name()), style))
        })
        .collect();
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} move \u{b7} space toggle \u{b7} esc close",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(42, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Columns")), rect);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
