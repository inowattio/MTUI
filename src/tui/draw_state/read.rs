use crate::app::App;
use crate::config::Column;
use crate::constants::keybind;
use crate::state::{LabelParams, Popup, ReadPanel, ReadParams, SearchParams, WriteParams};
use crate::tui::theme::{spinner_frame, Theme};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row, Table, Wrap};
use ratatui::Frame;
use crate::register::{RegisterCell, RegisterType};

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

fn pinned_table(params: &ReadParams, app: &App, visible: u16, header: String, theme: &Theme) -> Table<'static> {
    // Always one row per pinned register; rows not yet read show a placeholder.
    let pins = &app.pinned_registers;
    let len = pins.len();
    let top = (params.pinned_top as usize).min(len.saturating_sub(1));
    let end = (top + visible as usize).min(len);

    // Pins can mix Holding/Input, so prefix each row with a type marker.
    let header = format!("{:<2}{header}", "T");

    let mut table_rows = Vec::with_capacity(end - top);
    for (i, &(kind, address)) in pins.iter().enumerate().take(end).skip(top) {
        let (text, changed) = if i < params.pinned_rows.len() {
            (
                params.pinned_rows[i].clone(),
                params.pinned_changed.get(i).copied().unwrap_or(false),
            )
        } else {
            let label = app.label_text(kind, address);
            (app.interpreter.placeholder(address, label.as_deref()), false)
        };

        let marker = match kind {
            RegisterType::Holding => "H",
            RegisterType::Input => "I",
        };
        let text = format!("{marker:<2}{text}");

        let style = if i as u16 == params.pinned_index {
            theme.selected_style()
        } else if changed {
            theme.changed_style()
        } else if (i - top) % 2 == 1 {
            theme.zebra_style()
        } else {
            theme.base()
        };
        table_rows.push(Row::new([Cell::from(text)]).style(style));
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
    // On the Pinned panel the focus is the selected pin, not the Main cursor.
    let (info_type, info_addr) = if params.panel == ReadPanel::Pinned {
        app.pinned_registers
            .get(params.pinned_index as usize)
            .copied()
            .unwrap_or((params.register_type, params.position))
    } else {
        (params.register_type, params.position)
    };
    let is_pinned = app
        .pinned_registers
        .iter()
        .any(|&(kind, address)| kind == info_type && address == info_addr);

    let show_ascii = app.interpreter.shows_ascii() && !params.graph;
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
    let mut info_spans = Vec::new();
    if app.config.read_only {
        info_spans.push(Span::styled("READ-ONLY  ", theme.err_style()));
    }
    info_spans.extend([
        Span::styled("Device: ", theme.dim_style()),
        Span::styled(device.to_string(), theme.base()),
        Span::styled("  slave ", theme.dim_style()),
        Span::styled(app.config.device.slave_id.to_string(), theme.base()),
        Span::styled("   @ ", theme.dim_style()),
        Span::styled(info_addr.to_string(), theme.accent_style()),
    ]);
    if is_pinned {
        info_spans.push(Span::styled(" (pinned)", theme.changed_style()));
    }
    info_spans.push(Span::styled(
        format!("   {info_type:?} "),
        theme.base(),
    ));
    let (access, access_style) = match info_type {
        RegisterType::Holding => ("RW", theme.ok_style()),
        RegisterType::Input => ("RO", theme.warn_style()),
    };
    info_spans.push(Span::styled(access, access_style));
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

    if params.graph {
        draw_graph(frame, rows[1], theme, app, (info_type, info_addr));
        if let Some(popup) = &params.popup {
            draw_popup(frame, area, theme, app, popup);
        }
        return;
    }

    let ascii_string = match params.panel {
        ReadPanel::Main => {
            frame.render_widget(main_table(params, app, visible, header, theme), rows[1]);
            &params.ascii_string
        }
        ReadPanel::Pinned => {
            let table = if app.pinned_registers.is_empty() {
                rows_to_table(
                    "Pinned",
                    header,
                    &["No pinned registers.".to_string()],
                    &[],
                    None,
                    theme,
                )
            } else {
                pinned_table(params, app, visible, header, theme)
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
            None,
            Some("esc"),
        ),
        Popup::Dump(d) => draw_confirm(
            frame,
            area,
            theme,
            "Dump",
            &format!("Dump {} read register(s) to a file?", app.read_count()),
            &d.result,
            None,
            Some("esc"),
        ),
        Popup::Search(s) => draw_search(frame, area, theme, s),
        Popup::Label(l) => draw_label(frame, area, theme, l),
        Popup::Columns(selected) => draw_picker(frame, area, theme, app, *selected),
        Popup::Write(write) => draw_write(frame, area, theme, write),
        Popup::Slave(value) => draw_slave(frame, area, theme, *value),
        Popup::Quit => draw_confirm(
            frame,
            area,
            theme,
            "Unsaved changes",
            "Unsaved labels/pins. Quit anyway?",
            &None,
            Some("esc"),
            None
        ),
    }
}

fn draw_slave(frame: &mut Frame, area: Rect, theme: &Theme, value: u16) {
    let lines = vec![
        Line::from(vec![
            Span::styled("Slave ID: ", theme.dim_style()),
            Span::styled(value.min(u8::MAX as u16).to_string(), theme.accent_style()),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(Span::styled(
            " enter \u{b7} set   esc \u{b7} cancel",
            theme.dim_style(),
        )),
    ];

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(36, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Slave")), rect);
}

fn draw_help(frame: &mut Frame, area: Rect, theme: &Theme) {
    use keybind::*;
    let entries: &[(String, &str)] = &[
        (format!("{MOVE_UP}/{MOVE_DOWN}"), "Move cursor"),
        ("PgUp/Dn".to_string(), "Jump page"),
        (format!("{ACTION}"), "Read at cursor"),
        (format!("{REFRESH}"), "Refresh"),
        ("space".to_string(), "Pause / resume auto-refresh"),
        (format!("{TOGGLE}"), "Switch register type"),
        (format!("{WORD_ORDER}"), "Cycle word order"),
        (format!("{SWITCH_VIEW}"), "Switch Main / Pinned"),
        (format!("{JUMP}"), "Go to address / label"),
        (format!("{CYCLE_POSITION}"), "Toggle previous position"),
        (format!("{GRAPH}"), "Toggle value graph"),
        (format!("{WRITE}"), "Write register"),
        (format!("{SLAVE}"), "Set slave id"),
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
        " Graph might disallow some operations".to_string(),
        theme.dim_style(),
    )));
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
    additional_confirm: Option<&str>,
    additional_cancel: Option<&str>,
) {
    let mut info_line = String::new();
    info_line.push_str(" enter");
    if let Some(key) = additional_confirm {
        info_line.push_str(&format!("/{key}"));
    }
    info_line.push_str(" \u{b7} confirm   backspace");
    if let Some(key) = additional_cancel {
        info_line.push_str(&format!("/{key}"));
    }
    info_line.push_str(" cancel");

    let mut lines = vec![
        Line::from(Span::styled(prompt.to_string(), theme.base())),
        Line::from(Span::styled(
            info_line,
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
        Span::styled(" index/label: ", theme.dim_style()),
        Span::styled(search.query.clone(), theme.accent_style()),
        Span::styled("_", theme.accent_style()),
        Span::styled(format!("   ({len})"), theme.dim_style()),
    ]);

    let mut lines = vec![query_line, Line::default()];

    if search.matches.is_empty() {
        lines.push(Line::from(Span::styled(
            "Type an address or a label.",
            theme.dim_style(),
        )));
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
        " address or label \u{b7} \u{2191}/\u{2193} select \u{b7} enter go \u{b7} esc close",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(54, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Go to")), rect);
}

fn draw_write(frame: &mut Frame, area: Rect, theme: &Theme, write: &WriteParams) {
    let value = write
        .value
        .map_or_else(|| "(none)".to_string(), |n| n.to_string());

    let bits: u16 = match write.write_type {
        crate::app::WriteType::Word => 16,
        crate::app::WriteType::DWord => 32,
    };
    let raw = write.value.unwrap_or(0) as u32;

    // Bit grid, MSB on the left, grouped in nibbles; the cursor bit is highlighted.
    let mut bit_spans = vec![Span::styled("Bits:  ", theme.dim_style())];
    for i in (0..bits).rev() {
        let set = (raw >> i) & 1 == 1;
        let style = if i == write.bit_cursor {
            theme.selected_style()
        } else if set {
            theme.accent_style()
        } else {
            theme.dim_style()
        };
        bit_spans.push(Span::styled(if set { "1" } else { "0" }, style));
        if i % 4 == 0 && i != 0 {
            bit_spans.push(Span::raw(" "));
        }
    }

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("[{:?}] ", write.write_type), theme.dim_style()),
            Span::styled("to ", theme.dim_style()),
            Span::styled(write.position.to_string(), theme.accent_style()),
        ]),
        Line::from(vec![
            Span::styled("Value: ", theme.dim_style()),
            Span::styled(value, theme.base()),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(bit_spans),
        Line::from(Span::styled(
            format!("       bit {}", write.bit_cursor),
            theme.dim_style(),
        )),
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
        " enter write \u{b7} esc exit \u{b7} w word/dword \u{b7} - negate",
        theme.dim_style(),
    )));
    lines.push(Line::from(Span::styled(
        " \u{2190}/\u{2192} bit \u{b7} space toggle",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(58, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Write")), rect);
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

fn draw_graph(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, cell: RegisterCell) {
    let (kind, address) = cell;
    let label = app.label_text(kind, address);
    let title = match &label {
        Some(l) => format!(" Graph  \u{201c}{l}\u{201d} "),
        None => " Graph ".to_string(),
    };

    let points: Vec<(f64, f64)> = app
        .value_history(cell)
        .map(|h| h.iter().enumerate().map(|(i, &v)| (i as f64, v as f64)).collect())
        .unwrap_or_default();

    let block = theme.panel(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if points.len() < 2 {
        let msg = Paragraph::new(Line::from(Span::styled(
            "Collecting samples\u{2026} read this register a few times  (enter/r read \u{b7} space pause \u{b7} esc/g close)",
            theme.dim_style(),
        )));
        frame.render_widget(msg, inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    // Stats.
    let count = points.len();
    let last = points[count - 1].1;
    let prev = points[count - 2].1;
    let delta = last - prev;
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    let mut sum = 0.0;
    for &(_, y) in &points {
        min = min.min(y);
        max = max.max(y);
        sum += y;
    }
    let avg = sum / count as f64;
    let span = max - min;
    let (y_lo, y_hi) = if span < f64::EPSILON {
        (min - 1.0, max + 1.0)
    } else {
        let pad = span * 0.08;
        (min - pad, max + pad)
    };
    let x_hi = (count - 1) as f64;

    // Y labels: five evenly spaced ticks, bottom -> top.
    let y_labels: Vec<Span> = (0..=4)
        .map(|i| {
            let v = y_lo + (y_hi - y_lo) * (i as f64 / 4.0);
            Span::styled(format!("{v:.0}"), theme.dim_style())
        })
        .collect();
    let x_labels = vec![
        Span::styled(format!("-{}", count - 1), theme.dim_style()),
        Span::styled(format!("-{}", (count - 1) / 2), theme.dim_style()),
        Span::styled("now", theme.accent_style()),
    ];

    let datasets = vec![Dataset::default()
        .name(format!("{address}"))
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(theme.accent_style())
        .data(&points)];

    let chart = Chart::new(datasets)
        .x_axis(
            Axis::default()
                .title(Span::styled("samples", theme.dim_style()))
                .style(theme.dim_style())
                .bounds([0.0, x_hi])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title(Span::styled("value", theme.dim_style()))
                .style(theme.dim_style())
                .bounds([y_lo, y_hi])
                .labels(y_labels),
        );
    frame.render_widget(chart, chunks[0]);

    // Footer: live stats.
    let delta_style = if delta > 0.0 {
        theme.ok_style()
    } else if delta < 0.0 {
        theme.warn_style()
    } else {
        theme.dim_style()
    };
    let footer = Line::from(vec![
        Span::styled("cur ", theme.dim_style()),
        Span::styled(format!("{last:.0}"), theme.accent_style()),
        Span::styled(format!("  \u{0394}{delta:+.0}"), delta_style),
        Span::styled("   min ", theme.dim_style()),
        Span::styled(format!("{min:.0}"), theme.base()),
        Span::styled("  max ", theme.dim_style()),
        Span::styled(format!("{max:.0}"), theme.base()),
        Span::styled("  avg ", theme.dim_style()),
        Span::styled(format!("{avg:.1}"), theme.base()),
        Span::styled("  span ", theme.dim_style()),
        Span::styled(format!("{span:.0}"), theme.base()),
        Span::styled("  n ", theme.dim_style()),
        Span::styled(format!("{count}"), theme.base()),
    ]);
    frame.render_widget(Paragraph::new(footer), chunks[1]);
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
