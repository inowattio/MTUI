use crate::app::App;
use crate::config::Column;
use crate::constants::keybind;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{
    CustomField, CustomParams, LabelParams, LogsParams, Popup, ReadPanel, ReadParams, SearchParams,
    WriteParams,
};
use crate::tui::theme::{spinner_frame, Theme};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Cell, Chart, Clear, Dataset, GraphType, Paragraph, Row, Table, Wrap};
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
        let Some(addr) = params.window_start.checked_add(i) else {
            break;
        };
        let selected = addr == params.position;
        let zebra = i % 2 == 1;

        let cached = (addr >= params.data_start)
            .then(|| (addr - params.data_start) as usize)
            .and_then(|idx| {
                params.main_rows.get(idx).map(|text| {
                    (
                        text.clone(),
                        params.main_changed.get(idx).copied().unwrap_or(false),
                    )
                })
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

fn list_table(
    params: &ReadParams,
    app: &App,
    visible: u16,
    header: String,
    theme: &Theme,
    pins: &[RegisterCell],
    title: &str,
) -> Table<'static> {
    let len = pins.len();
    let top = (params.pinned_top as usize).min(len.saturating_sub(1));
    let end = (top + visible as usize).min(len);

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
            (
                app.interpreter.placeholder(address, label.as_deref()),
                false,
            )
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
        .block(theme.panel(title))
}

fn matrix_table(params: &ReadParams, app: &App, visible: u16, theme: &Theme) -> Table<'static> {
    let cols = app.config.matrix_cols.max(1);
    let base = params.window_start - (params.window_start % cols);

    let mut header = format!("{: >5}  ", "");
    for c in 0..cols {
        header.push_str(&format!("{: >5} ", format!("+{c}")));
    }

    let mut table_rows: Vec<Row> = Vec::with_capacity(visible as usize);
    for r in 0..visible {
        let row_base = (base as u32) + (r as u32) * (cols as u32);
        if row_base > u16::MAX as u32 {
            break;
        }
        let row_base = row_base as u16;
        let zebra = r % 2 == 1;

        let mut spans = vec![Span::styled(format!("{row_base: >5}: "), theme.dim_style())];
        for c in 0..cols {
            let Some(addr) = row_base.checked_add(c) else {
                break;
            };
            let cell = (params.register_type, addr);
            let (text, mut style) = match app.cell_value(cell) {
                Some(value) => {
                    let style = if app.cell_changed(cell) {
                        theme.changed_style()
                    } else if zebra {
                        theme.zebra_style()
                    } else {
                        theme.base()
                    };
                    (format!("{value: >5}"), style)
                }
                None => (format!("{: >5}", "--"), theme.dim_style()),
            };
            if addr == params.position {
                style = theme.selected_style();
            }
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" "));
        }
        table_rows.push(Row::new([Cell::from(Line::from(spans))]));
    }

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel("Matrix"))
}

pub fn draw(
    params: &ReadParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
    let (info_type, info_addr) = app.cursor_cell();
    let is_pinned = app
        .pinned_registers
        .iter()
        .any(|&(kind, address)| kind == info_type && address == info_addr);

    let show_ascii = app.interpreter.shows_ascii() && !params.graph;
    let row_constraints = if show_ascii {
        vec![
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(1),
        ]
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
        Span::styled(format!("   {info_type:?}"), theme.base()),
        Span::styled(" @ ", theme.dim_style()),
        Span::styled(info_addr.to_string(), theme.accent_style()),
    ]);
    if is_pinned {
        info_spans.push(Span::styled(" (pinned)", theme.changed_style()));
    }
    let (access, access_style) = match info_type {
        RegisterType::Holding => ("RW", theme.ok_style()),
        RegisterType::Input => ("RO", theme.warn_style()),
    };
    info_spans.push(Span::styled(format!(" {access}"), access_style));
    info_spans.push(Span::styled("   order ", theme.dim_style()));
    info_spans.push(Span::styled(
        format!("{:?}", app.config.device.word_order),
        theme.base(),
    ));
    info_spans.push(Span::styled("   batch ", theme.dim_style()));
    info_spans.push(Span::styled(
        format!("{}   ", app.config.registers_batch),
        theme.base(),
    ));
    info_spans.push(Span::styled(read_time, theme.dim_style()));
    if app.paused {
        info_spans.push(Span::styled("   \u{23f8} paused", theme.warn_style()));
    } else if let Some(interval) = app.config.update_interval_ms {
        if !params.loading {
            let remaining =
                (interval as u128).saturating_sub(params.refresh_timer.elapsed().as_millis());
            info_spans.push(Span::styled(
                format!("   ⟳ {:.1}s", remaining as f64 / 1000.0),
                theme.ok_style(),
            ));
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
        draw_graph(
            frame,
            rows[1],
            theme,
            app,
            (info_type, info_addr),
            params.graph_dword,
        );
        if let Some(popup) = &params.popup {
            draw_popup(frame, area, theme, app, popup);
        }
        return;
    }

    let ascii_string: &str = match params.panel {
        ReadPanel::Main => {
            frame.render_widget(main_table(params, app, visible, header, theme), rows[1]);
            &params.ascii_string
        }
        ReadPanel::Matrix => {
            frame.render_widget(matrix_table(params, app, visible, theme), rows[1]);
            ""
        }
        _ => {
            let (title, empty_message) = match params.panel {
                ReadPanel::Labeled => ("Labeled", "No labeled registers."),
                ReadPanel::Custom => ("Custom", "No custom rules."),
                _ => ("Pinned", "No pinned registers."),
            };
            let cells = app.panel_cells();
            let table = if cells.is_empty() {
                rows_to_table(
                    title,
                    header,
                    &[empty_message.to_string()],
                    &[],
                    None,
                    theme,
                )
            } else {
                list_table(params, app, visible, header, theme, &cells, title)
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
        Popup::Dump(d) => draw_confirm(
            frame,
            area,
            theme,
            "Dump",
            &format!("Dump {} read register(s) to a file?", app.read_count()),
            &d.result,
            (None, Some("esc")),
        ),
        Popup::Search(s) => draw_search(frame, area, theme, s),
        Popup::Label(l) => draw_label(frame, area, theme, l),
        Popup::Custom(c) => draw_custom(frame, area, theme, app, c),
        Popup::Columns(selected) => draw_picker(frame, area, theme, app, *selected),
        Popup::Write(write) => draw_write(frame, area, theme, write),
        Popup::Slave(value) => draw_slave(frame, area, theme, *value),
        Popup::Logs(logs) => draw_logs(frame, area, theme, logs),
        Popup::Inspect => draw_inspect(frame, area, theme, app),
        Popup::Quit => draw_confirm(
            frame,
            area,
            theme,
            "Unsaved changes",
            " Quit anyway?",
            &None,
            (Some("esc"), None),
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
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);

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
    additionals: (Option<&str>, Option<&str>),
) {
    let (additional_confirm, additional_cancel) = additionals;
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
        Line::from(Span::styled(info_line, theme.dim_style())),
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

fn draw_custom(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, c: &CustomParams) {
    let sel = c.current_field();

    let field_line = |label: &str, value: String, selected: bool| -> Line<'static> {
        let marker = if selected { "> " } else { "  " };
        let style = if selected {
            theme.selected_style()
        } else {
            theme.base()
        };
        Line::from(vec![
            Span::styled(format!("{marker}{label:<12} "), theme.dim_style()),
            Span::styled(value, style),
        ])
    };

    let mut lines: Vec<Line> = vec![];

    lines.push(match app.custom_preview(c) {
        Ok((input, output)) => Line::from(vec![
            Span::styled(" Preview  ", theme.dim_style()),
            Span::styled(input.to_string(), theme.base()),
            Span::styled(" \u{2192} ".to_string(), theme.dim_style()),
            Span::styled(output, theme.accent_style()),
        ]),
        Err(reason) => Line::from(vec![
            Span::styled(" Preview  ", theme.dim_style()),
            Span::styled(reason, theme.dim_style()),
        ]),
    });
    lines.push(Line::default());

    let repr_val = format!("{}  ({} reg)", c.repr.label(), c.repr.register_count());
    let repr_val = if sel == CustomField::Repr {
        format!("\u{2039} {repr_val} \u{203a}")
    } else {
        repr_val
    };
    lines.push(field_line("Type", repr_val, sel == CustomField::Repr));

    let ops_str = if c.ops.is_empty() {
        "(none)".to_string()
    } else {
        c.ops
            .iter()
            .map(|o| o.display())
            .collect::<Vec<_>>()
            .join(" ")
    };
    lines.push(field_line("Operations", ops_str, sel == CustomField::Ops));
    if sel == CustomField::Ops {
        lines.push(Line::from(Span::styled(
            "    (enter adds, backspace removes)".to_string(),
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            format!("    add: {}_   e.g. *0.1  +5  /10  ^2", c.op_buffer),
            theme.dim_style(),
        )));
    }

    let enum_str = if c.enum_map.is_empty() {
        "(none)".to_string()
    } else {
        c.enum_map
            .iter()
            .map(|e| format!("{}\u{2192}{}", e.value, e.text))
            .collect::<Vec<_>>()
            .join("  ")
    };
    lines.push(field_line("Enum", enum_str, sel == CustomField::Enum));
    if sel == CustomField::Enum {
        lines.push(Line::from(Span::styled(
            "    (enter adds, backspace removes)".to_string(),
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            format!("    add: {}_   e.g. 3=Running", c.enum_buffer),
            theme.dim_style(),
        )));
    }

    let dec = if c.decimals.is_empty() {
        "auto".to_string()
    } else {
        c.decimals.clone()
    };
    lines.push(field_line("Decimals", dec, sel == CustomField::Decimals));
    if sel == CustomField::Decimals {
        lines.push(Line::from(Span::styled(
            "    auto; 0 for none; numerical for amount".to_string(),
            theme.dim_style(),
        )));
    }

    let pfx = if sel == CustomField::Prefix {
        format!("{} ", c.prefix)
    } else {
        c.suffix.to_string()
    };
    lines.push(field_line("Prefix", pfx, sel == CustomField::Prefix));

    let sfx = if sel == CustomField::Suffix {
        format!("{} ", c.suffix)
    } else {
        c.suffix.to_string()
    };
    lines.push(field_line("Suffix", sfx, sel == CustomField::Suffix));

    lines.push(Line::default());
    let save_hint = if sel == CustomField::Save {
        "\u{2190} enter".to_string()
    } else {
        String::new()
    };
    lines.push(field_line("Save rule", save_hint, sel == CustomField::Save));
    let remove_hint = if sel == CustomField::Remove {
        "\u{2190} enter".to_string()
    } else {
        String::new()
    };
    lines.push(field_line(
        "Remove rule",
        remove_hint,
        sel == CustomField::Remove,
    ));

    if let Some(err) = &c.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {}", err.clone()),
            theme.err_style(),
        )));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} field \u{b7} \u{2190}/\u{2192} change",
        theme.dim_style(),
    )));
    lines.push(Line::from(Span::styled(
        " type to edit \u{b7} esc close",
        theme.dim_style(),
    )));

    let height = lines.len() as u16 + 2;
    let rect = centered_rect(48, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).block(theme.panel("Custom rule")),
        rect,
    );
}

fn draw_search(frame: &mut Frame, area: Rect, theme: &Theme, search: &SearchParams) {
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

fn draw_logs(frame: &mut Frame, area: Rect, theme: &Theme, logs: &LogsParams) {
    let visible = LogsParams::VISIBLE as usize;
    let len = logs.lines.len();
    let top = (logs.top as usize).min(len.saturating_sub(1));
    let end = (top + visible).min(len);

    let mut lines = vec![
        Line::from(Span::styled(format!(" {}", logs.path), theme.dim_style())),
        Line::default(),
    ];

    for line in &logs.lines[top..end] {
        lines.push(Line::from(Span::styled(format!(" {line}"), theme.base())));
    }
    for _ in end..top + visible {
        lines.push(Line::default());
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!(
            " {}/{}   \u{2191}/\u{2193} scroll \u{b7} PgUp/Dn page \u{b7} esc close",
            end.min(len),
            len
        ),
        theme.dim_style(),
    )));

    let width = 78;
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Write log")), rect);
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
    let columns = Column::ALL;
    let count = columns.len();

    let rows = count.div_ceil(2);
    const CELL: usize = 14;

    let cell = |i: usize| -> Span<'static> {
        let column = columns[i];
        let on = app.interpreter.is_enabled(column);
        let mark = if on { "[x]" } else { "[ ]" };
        let style = if i as u16 == selected {
            theme.selected_style()
        } else if on {
            theme.base()
        } else {
            theme.dim_style()
        };
        Span::styled(format!(" {mark} {:<CELL$} ", column.name()), style)
    };

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 1);
    for r in 0..rows {
        let mut spans = vec![cell(r)];
        let right = r + rows;
        if right < count {
            spans.push(Span::raw(" "));
            spans.push(cell(right));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} move \u{b7} space toggle \u{b7} esc close",
        theme.dim_style(),
    )));

    let width = (CELL as u16 + 6) * 2 + 3;
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Columns")), rect);
}

fn draw_inspect(frame: &mut Frame, area: Rect, theme: &Theme, app: &App) {
    let (_, entries) = app.inspect_lines();

    const NAME: usize = 9;
    const VALUE: usize = 21;

    let mut lines: Vec<Line> = Vec::new();
    if entries.is_empty() {
        lines.push(Line::from(Span::styled(
            " no data read yet",
            theme.dim_style(),
        )));
    } else {
        let rows = entries.len().div_ceil(2);
        let cell = |i: usize| -> [Span<'static>; 2] {
            let (name, value) = &entries[i];
            let value: String = value.chars().take(VALUE).collect();
            [
                Span::styled(format!(" {name:<NAME$} "), theme.dim_style()),
                Span::styled(format!("{value:<VALUE$} "), theme.base()),
            ]
        };
        for r in 0..rows {
            let mut spans = cell(r).to_vec();
            let right = r + rows;
            if right < entries.len() {
                spans.push(Span::raw(" "));
                spans.extend(cell(right));
            }
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " \u{2191}/\u{2193} move \u{b7} r refresh \u{b7} esc close",
        theme.dim_style(),
    )));

    let width = ((NAME + VALUE + 3) as u16) * 2 + 3;
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel("Inspect")), rect);
}

fn draw_graph(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    app: &App,
    cell: RegisterCell,
    dword: bool,
) {
    let (kind, address) = cell;
    let width = if dword { "DWord" } else { "Word" };
    let label = app.label_text(kind, address);
    let title = match &label {
        Some(l) => format!(" Graph [{width}] \u{201c}{l}\u{201d} "),
        None => format!(" Graph [{width}] "),
    };

    let points: Vec<(f64, f64)> = if dword {
        let order = app.config.device.word_order;
        match (
            app.value_history(cell),
            app.value_history((kind, address.wrapping_add(1))),
        ) {
            (Some(low), Some(high)) => {
                let n = low.len().min(high.len());
                low.iter()
                    .skip(low.len() - n)
                    .zip(high.iter().skip(high.len() - n))
                    .enumerate()
                    .map(|(i, (&a, &b))| (i as f64, order.make_word(a, b) as f64))
                    .collect()
            }
            _ => Vec::new(),
        }
    } else {
        app.value_history(cell)
            .map(|h| {
                h.iter()
                    .enumerate()
                    .map(|(i, &v)| (i as f64, v as f64))
                    .collect()
            })
            .unwrap_or_default()
    };

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
