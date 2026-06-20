use super::popups::draw_popup;
use crate::app::App;
use crate::register::RegisterCell;
use crate::state::{ReadPanel, ReadParams};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::{spinner_frame, Theme};
use chrono::Local;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table};
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

fn ascii_title(ascii: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(" ASCII ", theme.dim_style()),
        Span::styled(format!("'{ascii}' "), theme.base()),
    ])
    .left_aligned()
}

fn hscroll(text: &str, prefix: u16, offset: u16) -> String {
    if offset == 0 {
        return text.to_string();
    }
    let prefix = prefix as usize;
    let mut out: String = text.chars().take(prefix).collect();
    out.extend(text.chars().skip(prefix + offset as usize));
    out
}

fn with_hscroll_hint(
    block: Block<'static>,
    theme: &Theme,
    offset: u16,
    max_offset: u16,
) -> Block<'static> {
    if max_offset == 0 {
        return block;
    }
    let left = if offset > 0 { '\u{25c2}' } else { ' ' };
    let right = if offset < max_offset { '\u{25b8}' } else { ' ' };
    block.title_bottom(
        Line::styled(format!(" {left} cols {right} "), theme.dim_style()).right_aligned(),
    )
}

struct TableCtx<'a> {
    params: &'a ReadParams,
    app: &'a App,
    theme: &'a Theme,
    inner_width: u16,
}

impl TableCtx<'_> {
    fn horizontal_offset(&self, rows: &[(String, Style)], header: &str, prefix: u16) -> u16 {
        let prefix = prefix as usize;
        let content_rest = rows
            .iter()
            .map(|(t, _)| t.chars().count())
            .chain(std::iter::once(header.chars().count()))
            .max()
            .unwrap_or(0)
            .saturating_sub(prefix);
        let visible_rest = (self.inner_width as usize).saturating_sub(prefix);
        let max_offset = content_rest.saturating_sub(visible_rest) as u16;
        self.app.h_max_offset.set(max_offset);
        self.params.col_offset.min(max_offset)
    }

    fn scrollable_table(
        &self,
        rows: Vec<(String, Style)>,
        header: &str,
        prefix: u16,
        block: Block<'static>,
    ) -> Table<'static> {
        let h_off = self.horizontal_offset(&rows, header, prefix);
        let table_rows: Vec<Row> = rows
            .into_iter()
            .map(|(text, style)| Row::new([Cell::from(hscroll(&text, prefix, h_off))]).style(style))
            .collect();
        let block = with_hscroll_hint(block, self.theme, h_off, self.app.h_max_offset.get());

        Table::new(table_rows, [Constraint::Percentage(100)])
            .header(
                Row::new([Cell::from(hscroll(header, prefix, h_off))])
                    .style(self.theme.header_style()),
            )
            .block(block)
    }

    fn main_table(&self, visible: u16, header: &str, ascii: Option<&str>) -> Table<'static> {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Local::now();
        let mut rows: Vec<(String, Style)> = Vec::with_capacity(visible as usize);

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

            rows.push((text, style));
        }

        let mut block = theme.panel("Main");
        if let Some(error) = &params.read_error {
            block = block.title_bottom(
                Line::styled(format!(" \u{26a0} {error} "), theme.err_style()).left_aligned(),
            );
        } else if let Some(ascii) = ascii {
            block = block.title_bottom(ascii_title(ascii, theme));
        }

        self.scrollable_table(rows, header, app.interpreter.prefix_width(), block)
    }

    fn list_table(
        &self,
        cells: &[RegisterCell],
        top: usize,
        title: &str,
        ascii: Option<&str>,
    ) -> Table<'static> {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Local::now();
        let header = format!("{:<2}{}", "T", app.interpreter.header());

        let mut rows: Vec<(String, Style)> = Vec::with_capacity(cells.len());
        for (row_idx, &(kind, address)) in cells.iter().enumerate() {
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

            let text = format!("{:<2}{text}", kind.marker());

            let style = if (top + row_idx) as u16 == params.pinned_index {
                theme.selected_style()
            } else if changed {
                theme.changed_style()
            } else if row_idx % 2 == 1 {
                theme.zebra_style()
            } else {
                theme.base()
            };
            rows.push((text, style));
        }

        let mut block = theme.panel(title);
        if let Some(ascii) = ascii {
            block = block.title_bottom(ascii_title(ascii, theme));
        }

        // 2-char type marker alongside the index.
        self.scrollable_table(rows, &header, 2 + app.interpreter.prefix_width(), block)
    }

    fn matrix_table(&self, visible: u16) -> Table<'static> {
        let (params, app, theme) = (self.params, self.app, self.theme);
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
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
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
    let access_style = if info_type.is_writable() {
        theme.ok_style()
    } else {
        theme.warn_style()
    };
    info_spans.push(Span::styled(
        format!(" {}", info_type.access()),
        access_style,
    ));
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
    if app.sweep.active {
        let mode = if app.sweep.continuous { " (loop)" } else { "" };
        let span = app.sweep.to.saturating_sub(app.sweep.from);
        let done = app.sweep.current.saturating_sub(app.sweep.from);
        let percent = if span == 0 {
            100
        } else {
            (done as u32 * 100 / span as u32).min(100)
        };
        info_spans.push(Span::styled(
            format!(
                "   {} SWEEP {}\u{2192}{} @{} ({}%){}",
                spinner_frame(app.frame),
                app.sweep.from,
                app.sweep.to,
                app.sweep.current,
                percent,
                mode,
            ),
            theme.accent_style(),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(info_spans)).alignment(Alignment::Left),
        rows[0],
    );

    if let Some(status) = params.active_status() {
        if rows[0].height > 1 {
            let status_area = Rect {
                x: rows[0].x,
                y: rows[0].y + 1,
                width: rows[0].width,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    format!(" {}", status.text),
                    theme.message_style(status.kind),
                ))),
                status_area,
            );
        }
    }

    let header = app.interpreter.header();

    let visible = rows[1].height.saturating_sub(3).max(1);
    app.visible_rows.set(visible);
    // Inner table width. Panels without interpretation columns leave the offset at zero.
    let inner_width = rows[1].width.saturating_sub(2);
    app.h_max_offset.set(0);

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

    let ctx = TableCtx {
        params,
        app,
        theme,
        inner_width,
    };

    match params.panel {
        ReadPanel::Main => {
            let ascii = show_ascii.then(|| {
                app.ascii_string_for(
                    (0..visible)
                        .filter_map(|i| params.window_start.checked_add(i))
                        .map(|addr| (params.register_type, addr)),
                )
            });
            frame.render_widget(ctx.main_table(visible, header, ascii.as_deref()), rows[1]);
        }
        ReadPanel::Matrix => {
            frame.render_widget(ctx.matrix_table(visible), rows[1]);
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
            } else {
                let top = (params.pinned_top as usize).min(len - 1);
                let cells = app.panel_window(top, visible as usize);
                let ascii = show_ascii.then(|| app.ascii_string_for(cells.iter().copied()));
                frame.render_widget(
                    ctx.list_table(&cells, top, title, ascii.as_deref()),
                    rows[1],
                );
            }
        }
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
    // Coils/discrete inputs are 0/1; plot them as a square wave on a fixed
    // 0..1 axis. DWord graphing combines two registers, so it never applies.
    let bit_plot = kind.is_bit() && !dword;
    let width = if dword {
        "DWord"
    } else if bit_plot {
        "Bit"
    } else {
        "Word"
    };
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
        let mut spans = vec![Span::styled(
            "Collecting samples\u{2026} read this register a few times  ",
            theme.dim_style(),
        )];
        spans.extend(
            hints::footer(
                theme,
                &[
                    Hint::keys(hints::pair(kb.action, kb.refresh), "Read"),
                    Hint::key(kb.pause, "Pause"),
                    Hint::keys(hints::pair(kb.exit, kb.graph), "Close"),
                ],
            )
            .spans,
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
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
    let (y_lo, y_hi) = if bit_plot {
        (0.0, 1.0)
    } else if span < f64::EPSILON {
        (min - 1.0, max + 1.0)
    } else {
        let pad = span * 0.08;
        (min - pad, max + pad)
    };
    let x_hi = (count - 1) as f64;

    let y_labels: Vec<Span> = if bit_plot {
        vec![
            Span::styled("0", theme.dim_style()),
            Span::styled("1", theme.dim_style()),
        ]
    } else {
        (0..=4)
            .map(|i| {
                let v = y_lo + (y_hi - y_lo) * (i as f64 / 4.0);
                Span::styled(format!("{v:.0}"), theme.dim_style())
            })
            .collect()
    };
    let x_labels = vec![
        Span::styled(format!("-{}", count - 1), theme.dim_style()),
        Span::styled(format!("-{}", (count - 1) / 2), theme.dim_style()),
        Span::styled("now", theme.accent_style()),
    ];

    // For a square wave, hold each sample's value until the next sample so
    // the connecting line steps rather than ramps between levels.
    let stepped: Vec<(f64, f64)>;
    let plot_data: &[(f64, f64)] = if bit_plot {
        let mut steps = Vec::with_capacity(points.len() * 2);
        for pair in points.windows(2) {
            let (x0, y0) = pair[0];
            let (x1, _) = pair[1];
            steps.push((x0, y0));
            steps.push((x1, y0));
        }
        if let Some(&last) = points.last() {
            steps.push(last);
        }
        stepped = steps;
        &stepped
    } else {
        &points
    };

    let datasets = vec![Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(theme.accent_style())
        .data(plot_data)];

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
