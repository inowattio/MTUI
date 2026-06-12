use super::popups::draw_popup;
use crate::app::App;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{ReadPanel, ReadParams};
use crate::tui::theme::{spinner_frame, Theme};
use chrono::Local;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table, Wrap};
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
    header: &str,
    theme: &Theme,
) -> Table<'static> {
    let now = Local::now();
    let mut table_rows = Vec::with_capacity(visible as usize);

    for i in 0..visible {
        let Some(addr) = params.window_start.checked_add(i) else {
            break;
        };
        let selected = addr == params.position;
        let zebra = i % 2 == 1;

        let (text, style) = match app.cell_row((params.register_type, addr), now) {
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

    if let Some(error) = &params.read_error {
        if let Some(first) = table_rows.first_mut() {
            *first = Row::new([Cell::from(error.clone())]).style(theme.err_style());
        }
    }

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header.to_string())]).style(theme.header_style()))
        .block(theme.panel("Main"))
}

fn list_table(
    params: &ReadParams,
    app: &App,
    header: &str,
    theme: &Theme,
    cells: &[RegisterCell],
    top: usize,
    title: &str,
) -> Table<'static> {
    let now = Local::now();
    let header = format!("{:<2}{header}", "T");

    let mut table_rows = Vec::with_capacity(cells.len());
    for (offset, &(kind, address)) in cells.iter().enumerate() {
        let (text, changed) = match app.cell_row((kind, address), now) {
            Some(row) => row,
            None => {
                let label = app.label_text(kind, address);
                (
                    app.interpreter.placeholder(address, label.as_deref()),
                    false,
                )
            }
        };

        let marker = match kind {
            RegisterType::Holding => "H",
            RegisterType::Input => "I",
        };
        let text = format!("{marker:<2}{text}");

        let style = if (top + offset) as u16 == params.pinned_index {
            theme.selected_style()
        } else if changed {
            theme.changed_style()
        } else if offset % 2 == 1 {
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
    if !app.config.name.is_empty() {
        info_spans.push(Span::styled(" ", theme.dim_style()));
        info_spans.push(Span::styled(app.config.name.clone(), theme.accent_style()));
        info_spans.push(Span::styled(" \u{b7} ", theme.dim_style()));
    }
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

    let ascii_string: String = match params.panel {
        ReadPanel::Main => {
            frame.render_widget(main_table(params, app, visible, header, theme), rows[1]);
            if show_ascii {
                app.ascii_string_for(
                    (0..visible)
                        .filter_map(|i| params.window_start.checked_add(i))
                        .map(|addr| (params.register_type, addr)),
                )
            } else {
                String::new()
            }
        }
        ReadPanel::Matrix => {
            frame.render_widget(matrix_table(params, app, visible, theme), rows[1]);
            String::new()
        }
        _ => {
            let (title, empty_message) = match params.panel {
                ReadPanel::Labeled => ("Labeled", "No labeled registers."),
                ReadPanel::Custom => ("Custom", "No custom rules."),
                _ => ("Pinned", "No pinned registers."),
            };
            let len = app.panel_len() as usize;
            if len == 0 {
                frame.render_widget(
                    rows_to_table(
                        title,
                        header.to_string(),
                        &[empty_message.to_string()],
                        &[],
                        None,
                        theme,
                    ),
                    rows[1],
                );
                String::new()
            } else {
                let top = (params.pinned_top as usize).min(len - 1);
                let cells = app.panel_window(top, visible as usize);
                frame.render_widget(
                    list_table(params, app, header, theme, &cells, top, title),
                    rows[1],
                );
                if show_ascii {
                    app.ascii_string_for(cells.iter().copied())
                } else {
                    String::new()
                }
            }
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

    let block = theme.panel(&title).title_bottom(
        Line::styled(" some keybinds are unavailable here ", theme.dim_style()).right_aligned(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if points.len() < 2 {
        let kb = &app.config.keybinds;
        let msg = Paragraph::new(Line::from(Span::styled(
            format!(
                "Collecting samples\u{2026} read this register a few times  ({}/{} read \u{b7} {} pause \u{b7} {}/{} close)",
                kb.action, kb.refresh, kb.pause, kb.exit, kb.graph
            ),
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
