use super::popups::draw_popup;
use crate::app::App;
use crate::config::{Column, Config};
use crate::constants::{NO_VALUE, UNINTERPRETABLE};
use crate::input::KeyCode;
use crate::interpretator::fmt_num;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{ReadPanel, ReadParams};
use crate::tui::hints::{self, Hint};
use crate::tui::rows_table::{RowsTable, TableRow};
use crate::tui::theme::{Theme, spinner_frame};
use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, LegendPosition, Paragraph};

fn panel_block(theme: &Theme, active: ReadPanel, config: &Config) -> Block<'static> {
    if !config.show_inactive_tabs {
        return theme.tabbed_panel(&[active.name()], 0);
    }
    let panels: Vec<ReadPanel> = ReadPanel::ALL
        .into_iter()
        .filter(|&p| config.cycle_panels.enabled(p) || p == active)
        .collect();
    let names: Vec<&'static str> = panels.iter().map(|&p| p.name()).collect();
    let index = panels.iter().position(|&p| p == active);
    theme.tabbed_panel(&names, index.unwrap_or(0))
}

fn ascii_title(ascii: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(format!(" '{ascii}'"), theme.base())).right_aligned()
}

fn rows_table(
    rows: Vec<TableRow>,
    header: String,
    theme: &Theme,
    block: Block<'static>,
) -> RowsTable {
    RowsTable::new(block, header, theme.header_style(), rows)
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
    ) -> RowsTable {
        let h_off = self.horizontal_offset(&rows, header, prefix);
        let table_rows = rows
            .into_iter()
            .map(|(text, style)| TableRow::plain(text, style))
            .collect();
        rows_table(table_rows, header.to_string(), self.theme, block).hscroll(prefix, h_off)
    }

    fn main_table(&self, visible: u16, header: &str, ascii: Option<&str>) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Utc::now();
        let mut rows: Vec<(String, Style)> = Vec::with_capacity(visible as usize);

        let show_window = app.config.show_read_window;
        let (read_start, read_amount) = app.read_window();
        let read_end = read_start.saturating_add(read_amount - 1);

        for i in 0..visible {
            let Some(addr) = params.window_start.checked_add(i) else {
                break;
            };
            let selected = addr == params.position;
            let zebra = i % 2 == 1;

            let (text, base_style) = match app.cell_row((params.register_type, addr), now) {
                Some((text, changed)) => (text, theme.row_style(zebra, changed)),
                None => (
                    app.interpreter
                        .placeholder(addr, app.label((params.register_type, addr))),
                    theme.dim_style(),
                ),
            };
            let style = if selected {
                theme.selected_style()
            } else {
                base_style
            };

            if show_window {
                let marker = if (read_start..=read_end).contains(&addr) {
                    "|"
                } else {
                    " "
                };
                rows.push((format!("{marker}{text}"), style));
            } else {
                rows.push((text, style));
            }
        }

        let mut block = panel_block(theme, ReadPanel::Main, &app.config);
        if let Some(error) = &params.read_error {
            block = block
                .title_bottom(Line::styled(format!("! {error}"), theme.err_style()).left_aligned());
        } else if let Some(ascii) = ascii {
            block = block.title_top(ascii_title(ascii, theme));
        }

        if show_window {
            let header = format!(" {header}");
            self.scrollable_table(rows, &header, 1 + app.interpreter.prefix_width(), block)
        } else {
            self.scrollable_table(rows, header, app.interpreter.prefix_width(), block)
        }
    }

    fn list_table(&self, cells: &[RegisterCell], top: usize, ascii: Option<&str>) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Utc::now();
        let show_window = app.config.show_read_window;
        let read_cells = show_window.then(|| app.panel_read_cells());
        let mut header = format!("{:<2}{}", "T", app.interpreter.header());
        if show_window {
            header.insert(0, ' ');
        }

        let mut rows: Vec<(String, Style)> = Vec::with_capacity(cells.len() + 3);
        let mut prev_kind: Option<RegisterType> = None;
        for (ord, &(kind, address)) in cells.iter().enumerate() {
            if prev_kind.is_some_and(|pk| pk != kind) {
                rows.push((String::new(), theme.base()));
            }
            prev_kind = Some(kind);

            let (text, changed) = match app.cell_row((kind, address), now) {
                Some(row) => row,
                None => (
                    app.interpreter
                        .placeholder(address, app.label((kind, address))),
                    false,
                ),
            };

            let text = match &read_cells {
                Some(read_cells) => {
                    let marker = if read_cells.contains(&(kind, address)) {
                        "|"
                    } else {
                        " "
                    };
                    format!("{marker}{:<2}{text}", kind.marker())
                }
                None => format!("{:<2}{text}", kind.marker()),
            };

            let style = if (top + ord) as u16 == params.pinned_index {
                theme.selected_style()
            } else {
                theme.row_style(ord % 2 == 1, changed)
            };
            rows.push((text, style));
        }

        let mut block = panel_block(theme, params.panel, &app.config);
        if let Some(ascii) = ascii {
            block = block.title_top(ascii_title(ascii, theme));
        }

        // 2-char type marker alongside the address, plus the read-window marker
        let prefix = 2 + u16::from(show_window) + app.interpreter.prefix_width();
        self.scrollable_table(rows, &header, prefix, block)
    }

    fn matrix_table(&self, visible: u16) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let cols = app.config.matrix_cols.max(1);
        let base = params.window_start - (params.window_start % cols);

        let show_window = app.config.show_read_window;
        let (read_start, read_amount) = app.read_window();
        let read_end = read_start.saturating_add(read_amount - 1);

        let mut header = format!("{: >5}  ", "");
        for c in 0..cols {
            header.push_str(&format!("{: >5} ", format!("+{c}")));
        }

        let mut table_rows: Vec<TableRow> = Vec::with_capacity(visible as usize);
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
                        let style = theme.row_style(zebra, app.cell_changed(cell));
                        (format!("{value: >5}"), style)
                    }
                    None => (format!("{NO_VALUE: >5}"), theme.dim_style()),
                };
                if addr == params.position {
                    style = theme.selected_style();
                }
                if show_window && (read_start..=read_end).contains(&addr) {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                spans.push(Span::styled(text, style));
                spans.push(Span::raw(" "));
            }
            table_rows.push(TableRow {
                spans,
                style: Style::default(),
            });
        }

        rows_table(
            table_rows,
            header,
            theme,
            panel_block(theme, ReadPanel::Matrix, &app.config),
        )
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

    let show_ascii = app.config.show_ascii && !params.graph;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    let read_only = if app.config.read_only {
        Some(theme.err_style())
    } else if info_type.is_writable() {
        None
    } else {
        Some(theme.warn_style())
    };

    let mut identity: Vec<Vec<Span>> = Vec::new();
    if !app.config.name.is_empty() {
        identity.push(vec![Span::styled(
            app.config.name.clone(),
            theme.accent_style(),
        )]);
    }
    identity.push(vec![
        Span::styled("device: ", theme.dim_style()),
        Span::styled(device.to_string(), theme.base()),
    ]);
    identity.push(vec![
        Span::styled("slave ", theme.dim_style()),
        Span::styled(app.config.device.slave_id.to_string(), theme.base()),
    ]);
    if let Some(style) = read_only {
        identity.push(vec![Span::styled("RO", style)]);
    }
    identity.push(vec![
        Span::styled("order ", theme.dim_style()),
        Span::styled(format!("{:?}", app.config.device.word_order), theme.base()),
    ]);
    identity.push(vec![
        Span::styled("batch ", theme.dim_style()),
        Span::styled(app.config.registers_batch.to_string(), theme.base()),
    ]);

    let cycle = &app.config.cycle_types;
    let types: Vec<RegisterType> = if app.config.show_inactive_tabs {
        RegisterType::ALL
            .into_iter()
            .filter(|&t| cycle.enabled(t) || t == info_type)
            .collect()
    } else {
        vec![info_type]
    };
    let active = types.iter().position(|&t| t == info_type).unwrap_or(0);

    let names: Vec<String> = types
        .iter()
        .enumerate()
        .map(|(i, t)| {
            if i == active {
                format!("{} @ {info_addr}", t.name())
            } else {
                t.marker().to_string()
            }
        })
        .collect();

    let mut cell_seg = theme.tab_spans(names, active);
    if is_pinned {
        cell_seg.push(Span::styled(" (pinned)", theme.changed_style()));
    }
    let mut right: Vec<Vec<Span>> = Vec::new();
    if let Some(d) = params.read_duration {
        right.push(vec![Span::styled(format!("{d:.2?}"), theme.dim_style())]);
    }
    right.push(cell_seg);

    let left_line = theme.join_dotted(right.into_iter().rev());
    let right_line = theme.join_dotted(identity.into_iter().rev());

    let info_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(rows[0]);

    frame.render_widget(Line::from(left_line), info_rows[0]);
    frame.render_widget(Line::from(right_line).right_aligned(), info_rows[0]);

    if let Some(status) = params.active_status() {
        frame.render_widget(theme.status_line(status), info_rows[1]);
    }

    let header = app.interpreter.header();

    // Tab line + header row; the read-error message adds a bottom title row.
    let error_row = params.panel == ReadPanel::Main && params.read_error.is_some();
    let visible = rows[1]
        .height
        .saturating_sub(2 + u16::from(error_row))
        .max(1);
    app.visible_rows.set(visible);
    // Inner table width. Panels without interpretation columns leave the offset at zero.
    let inner_width = rows[1].width;
    app.h_max_offset.set(0);

    if params.graph {
        draw_graph(
            frame,
            rows[1],
            theme,
            app,
            (info_type, info_addr),
            app.active_graph_column(),
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
            let len = app.panel_len() as usize;
            if len == 0 {
                let t = rows_table(
                    Vec::new(),
                    header.to_string(),
                    theme,
                    panel_block(theme, params.panel, &app.config),
                );
                frame.render_widget(t, rows[1]);

                let kb = &app.config.keybinds;
                let (message, hint) = match params.panel {
                    ReadPanel::Labeled => (
                        "no labeled registers yet -",
                        Hint::key(kb.label, "label the selected register"),
                    ),
                    ReadPanel::Custom => (
                        "no custom rules yet -",
                        Hint::key(kb.custom, "add a rule for the selected register"),
                    ),
                    _ => (
                        "nothing pinned yet -",
                        Hint::key(kb.pin, "pin the selected register"),
                    ),
                };
                let mut spans = vec![Span::styled(message, theme.dim_style())];
                spans.extend(hints::footer(theme, [hint]).spans);

                let body_top = rows[1].y + 2;
                let body_height = rows[1].height.saturating_sub(2);
                if body_height > 0 {
                    let hint_area = Rect {
                        x: rows[1].x,
                        y: body_top + body_height / 3,
                        width: rows[1].width,
                        height: 1,
                    };
                    frame.render_widget(
                        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
                        hint_area,
                    );
                }
            } else {
                let top = (params.pinned_top as usize).min(len - 1);
                let mut cells = app.panel_window(top, visible as usize);

                let mut used = 0usize;
                let mut prev_kind: Option<RegisterType> = None;
                cells.retain(|&(kind, _)| {
                    used += 1 + usize::from(prev_kind.is_some_and(|pk| pk != kind));
                    prev_kind = Some(kind);
                    used <= visible as usize
                });
                let ascii = show_ascii.then(|| app.ascii_string_for(cells.iter().copied()));
                frame.render_widget(ctx.list_table(&cells, top, ascii.as_deref()), rows[1]);
            }
        }
    }

    if let Some(popup) = &params.popup {
        draw_popup(frame, area, theme, app, popup);
    }
}

pub fn live_status(app: &App, params: &ReadParams, theme: &Theme) -> Vec<Span<'static>> {
    let mut fields: Vec<Vec<Span<'static>>> = Vec::new();

    if app.paused {
        fields.push(vec![Span::styled("|| paused", theme.warn_style())]);
    } else if let Some(interval) = app.config.update_interval_ms.filter(|_| !app.sweep.active) {
        let remaining = if params.loading {
            0
        } else {
            (interval as u128).saturating_sub(params.refresh_timer.elapsed().as_millis())
        };
        fields.push(vec![Span::styled(
            format!(" {:>4.1}s", remaining as f64 / 1000.0),
            theme.ok_style(),
        )]);
    }
    if app.sweep.active {
        let mode = if app.sweep.continuous { " loop" } else { "" };
        let span = app.sweep.to.saturating_sub(app.sweep.from);
        let done = app.sweep.current.saturating_sub(app.sweep.from);
        let percent = if span == 0 {
            100
        } else {
            (done as u32 * 100 / span as u32).min(100)
        };
        fields.push(vec![Span::styled(
            format!(
                " {}{} {}->{} ({:>2}%)",
                spinner_frame(app.frame),
                mode,
                app.sweep.from,
                app.sweep.to,
                percent.min(99),
            ),
            theme.accent_style(),
        )]);
    }

    let mut spans = theme.join_dotted(fields);
    if !spans.is_empty() {
        spans.push(Span::raw("  "));
    }
    spans
}

const SERIES_COLORS: [Color; 3] = [Color::LightBlue, Color::LightMagenta, Color::LightYellow];

/// A series of (sample time, value) points.
type Series = Vec<(DateTime<Utc>, f64)>;

fn series_history(app: &App, cell: RegisterCell, column: Option<Column>) -> Series {
    if cell.0.is_bit() {
        app.value_history(cell)
            .map(|h| h.iter().map(|&(v, t)| (t, v as f64)).collect())
            .unwrap_or_default()
    } else {
        column.map_or_else(Vec::new, |c| app.column_history(cell, c))
    }
}

fn split_gaps(points: Vec<(f64, f64)>) -> Vec<Vec<(f64, f64)>> {
    if points.len() < 3 {
        return vec![points];
    }
    let mut deltas: Vec<f64> = points.windows(2).map(|w| w[1].0 - w[0].0).collect();
    deltas.sort_by(f64::total_cmp);
    let gap = (deltas[deltas.len() / 2] * 2.5).max(1.0);

    let mut segments = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    for point in points {
        if current.last().is_some_and(|&(x, _)| point.0 - x > gap) {
            segments.push(std::mem::take(&mut current));
        }
        current.push(point);
    }
    segments.push(current);
    segments
}

fn segment_dataset(segment: &[(f64, f64)], style: Style) -> Dataset<'_> {
    Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(if segment.len() == 1 {
            GraphType::Scatter
        } else {
            GraphType::Line
        })
        .style(style)
        .data(segment)
}

fn fmt_secs_ago(x: f64) -> String {
    let ago = -x;
    if ago >= 120.0 {
        format!("-{:.0}m", ago / 60.0)
    } else {
        format!("-{ago:.0}s")
    }
}

fn step_points(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
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
    steps
}

fn draw_graph(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    app: &App,
    cell: RegisterCell,
    column: Option<Column>,
) {
    let (kind, address) = cell;
    let bit_plot = kind.is_bit();

    let mode = if bit_plot {
        "bit"
    } else {
        column.map_or(UNINTERPRETABLE, Column::name)
    };
    let label = app.label_text(kind, address);
    let title = match &label {
        Some(l) => format!("Graph [{mode}] \"{l}\""),
        None => format!("Graph [{mode}]"),
    };

    let block = theme.panel(&title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if !bit_plot && column.is_none() {
        let hint = Paragraph::new(Line::from(Span::styled(
            "Enable a numeric column (u16, i16, f32, ...) to graph.",
            theme.dim_style(),
        )));
        frame.render_widget(hint, inner);
        return;
    }

    let primary = series_history(app, cell, column);

    let is_float = match column {
        _ if bit_plot => false,
        Some(Column::Custom) => primary.iter().any(|&(_, y)| y.fract() != 0.0),
        Some(c) => c.graph_is_float(),
        None => false,
    };

    if primary.len() < 2 {
        let kb = &app.config.keybinds;
        let mut spans = vec![Span::styled(
            "Collecting samples... read this register a few times  ",
            theme.dim_style(),
        )];
        spans.extend(
            hints::footer(
                theme,
                [
                    Hint::pair(kb.action, kb.refresh, "Read"),
                    Hint::key(kb.pause, "Pause"),
                    Hint::pair(KeyCode::Esc, kb.graph, "Close"),
                ],
            )
            .spans,
        );
        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let held: Vec<(RegisterCell, Series)> = app
        .read()
        .graph_series
        .iter()
        .filter(|&&c| c != cell)
        .map(|&c| (c, series_history(app, c, column)))
        .filter(|(_, history)| history.len() >= 2)
        .collect();

    let last = primary[primary.len() - 1].1;
    let delta = last - primary[primary.len() - 2].1;
    let mut min = f64::MAX;
    let mut max = f64::MIN;
    let mut sum = 0.0;
    let mut count = 0usize;
    for &(_, y) in primary
        .iter()
        .chain(held.iter().flat_map(|(_, history)| history.iter()))
    {
        min = min.min(y);
        max = max.max(y);
        sum += y;
        count += 1;
    }
    let avg = sum / count as f64;
    let span = max - min;
    let (lo, hi) = (min, max);

    let (y_lo, y_hi) = if bit_plot && held.is_empty() {
        (0.0, 1.0)
    } else if (hi - lo).abs() < f64::EPSILON {
        (lo - 1.0, hi + 1.0)
    } else {
        let pad = (hi - lo) * 0.08;
        (lo - pad, hi + pad)
    };

    let time_axis = app.config.graph_time_axis;
    let now = Utc::now();
    let rel = |t: DateTime<Utc>| -(now.signed_duration_since(t).num_milliseconds() as f64 / 1000.0);
    let max_len = held
        .iter()
        .map(|(_, history)| history.len())
        .max()
        .unwrap_or(0)
        .max(primary.len());
    let x_lo = if time_axis {
        let mut x_lo = rel(primary[0].0);
        for (_, history) in &held {
            if let Some(&(t, _)) = history.first() {
                x_lo = x_lo.min(rel(t));
            }
        }
        x_lo.min(-1.0)
    } else {
        -((max_len - 1) as f64)
    };

    let plot_h = chunks[0].height.max(2) as usize;
    let y_count = [5usize, 6, 4, 7, 3]
        .into_iter()
        .find(|n| (plot_h - 1).is_multiple_of(n - 1))
        .unwrap_or(5);
    let y_labels: Vec<Span> = if bit_plot {
        vec![
            Span::styled("0", theme.dim_style()),
            Span::styled("1", theme.dim_style()),
        ]
    } else {
        (0..y_count)
            .map(|i| {
                let v = y_lo + (y_hi - y_lo) * (i as f64 / (y_count - 1) as f64);
                Span::styled(fmt_num(v, is_float), theme.dim_style())
            })
            .collect()
    };
    let y_gutter = y_labels.iter().map(Span::width).max().unwrap_or(0);

    let to_points = |history: &[(DateTime<Utc>, f64)]| -> Vec<(f64, f64)> {
        if time_axis {
            history.iter().map(|&(t, v)| (rel(t), v)).collect()
        } else {
            let base = 1.0 - history.len() as f64;
            history
                .iter()
                .enumerate()
                .map(|(i, &(_, v))| (base + i as f64, v))
                .collect()
        }
    };
    let series_segments = |c: RegisterCell, history: &[(DateTime<Utc>, f64)]| {
        let points = to_points(history);
        let segments = if time_axis {
            split_gaps(points)
        } else {
            vec![points]
        };
        segments
            .into_iter()
            .map(|segment| {
                if c.0.is_bit() {
                    step_points(segment)
                } else {
                    segment
                }
            })
            .collect::<Vec<_>>()
    };
    let series_name = |c: RegisterCell| {
        app.label_text(c.0, c.1)
            .unwrap_or_else(|| format!("{}{}", c.0.marker(), c.1))
    };

    let primary_segments = series_segments(cell, &primary);
    type Segments = Vec<Vec<(f64, f64)>>;
    let held_segments: Vec<(RegisterCell, Segments)> = held
        .iter()
        .map(|(c, history)| (*c, series_segments(*c, history)))
        .collect();

    let mut names: Vec<String> = held_segments
        .iter()
        .map(|&(c, _)| series_name(c))
        .chain(std::iter::once(series_name(cell)))
        .collect();
    let name_width = names.iter().map(|n| n.chars().count()).max().unwrap_or(0);
    let latest = held
        .iter()
        .map(|(_, history)| history[history.len() - 1].1)
        .chain(std::iter::once(last));
    for (name, value) in names.iter_mut().zip(latest) {
        while name.chars().count() < name_width {
            name.push(' ');
        }
        name.push(' ');
        name.push_str(&fmt_num(value, is_float));
    }
    let primary_name = names.pop().unwrap_or_default();

    let mut datasets: Vec<Dataset> = Vec::new();
    for (i, ((_, segments), name)) in held_segments.iter().zip(names).enumerate() {
        let style = Style::default().fg(SERIES_COLORS[i % SERIES_COLORS.len()]);
        for (j, segment) in segments.iter().enumerate() {
            let mut dataset = segment_dataset(segment, style);
            if j == 0 {
                dataset = dataset.name(name.clone());
            }
            datasets.push(dataset);
        }
    }
    for (j, segment) in primary_segments.iter().enumerate() {
        let mut dataset = segment_dataset(segment, theme.accent_style());
        if j == 0 && !held_segments.is_empty() {
            dataset = dataset.name(primary_name.clone());
        }
        datasets.push(dataset);
    }

    let chart = Chart::new(datasets)
        .hidden_legend_constraints((Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)))
        .legend_position(Some(LegendPosition::BottomLeft))
        .x_axis(Axis::default().style(theme.dim_style()).bounds([x_lo, 0.0]))
        .y_axis(
            Axis::default()
                .title(Span::styled("value", theme.dim_style()))
                .style(theme.dim_style())
                .bounds([y_lo, y_hi])
                .labels(y_labels),
        );
    frame.render_widget(chart, chunks[0]);

    let width = chunks[1].width as usize;
    let mut axis_line = " ".repeat(y_gutter.min(width));
    axis_line.push('+');
    axis_line.push_str(&"-".repeat(width.saturating_sub(y_gutter + 1)));
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(axis_line, theme.dim_style()))),
        chunks[1],
    );

    let plot_left = y_gutter + 1;
    let plot_w = width.saturating_sub(plot_left).max(2);
    let mut row: Vec<char> = vec![' '; width];
    let mut now_start = width;
    const X_TICKS: usize = 5;
    for i in 0..X_TICKS {
        let f = i as f64 / (X_TICKS - 1) as f64;
        let text = if i == X_TICKS - 1 {
            "now".to_string()
        } else if time_axis {
            fmt_secs_ago(x_lo * (1.0 - f))
        } else {
            format!("{:.0}", x_lo * (1.0 - f))
        };
        let len = text.chars().count();
        let center = plot_left + (f * (plot_w - 1) as f64).round() as usize;
        let start = center
            .saturating_sub(len / 2)
            .min(width.saturating_sub(len));
        if i == X_TICKS - 1 {
            now_start = start;
        }
        for (k, ch) in text.chars().enumerate() {
            row[start + k] = ch;
        }
    }
    let row: String = row.into_iter().collect();
    let (dim_part, now_part) = row.split_at(now_start.min(row.len()));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(dim_part.to_string(), theme.dim_style()),
            Span::styled(now_part.to_string(), theme.accent_style()),
        ])),
        chunks[2],
    );

    let delta_style = if delta > 0.0 {
        theme.ok_style()
    } else if delta < 0.0 {
        theme.warn_style()
    } else {
        theme.dim_style()
    };
    let delta_str = if is_float {
        format!(
            "{}{}",
            if delta < 0.0 { "-" } else { "+" },
            fmt_num(delta.abs(), true)
        )
    } else {
        format!("{delta:+.0}")
    };
    let avg_str = if is_float {
        fmt_num(avg, true)
    } else {
        format!("{avg:.1}")
    };
    let mut spans = Vec::new();
    if held.is_empty() {
        spans.extend([
            Span::styled("cur ", theme.dim_style()),
            Span::styled(fmt_num(last, is_float), theme.accent_style()),
            Span::styled(format!("  delta {delta_str}   "), delta_style),
        ]);
    }
    spans.extend([
        Span::styled("min ", theme.dim_style()),
        Span::styled(fmt_num(min, is_float), theme.base()),
        Span::styled("  max ", theme.dim_style()),
        Span::styled(fmt_num(max, is_float), theme.base()),
        Span::styled("  avg ", theme.dim_style()),
        Span::styled(avg_str, theme.base()),
        Span::styled("  span ", theme.dim_style()),
        Span::styled(fmt_num(span, is_float), theme.base()),
        Span::styled("  n ", theme.dim_style()),
        Span::styled(format!("{count}"), theme.base()),
    ]);
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[3]);
}
