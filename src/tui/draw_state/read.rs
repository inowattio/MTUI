use super::popups::draw_popup;
use crate::app::{App, Trend};
use crate::config::{Column, Config};
use crate::constants::{NO_VALUE, UNINTERPRETABLE};
use crate::input::KeyCode;
use crate::interpretator::{RowSegment, fmt_num};
use crate::register::{RegisterCell, RegisterType};
use crate::state::ConnectionStatus;
use crate::state::{ReadPanel, ReadParams, ScreenLayout};
use crate::tui::hints::{self, Hint};
use crate::tui::rows_table::{RowsTable, TableRow, max_h_offset};
use crate::tui::theme::{Theme, spinner_frame, status_parts};
use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, LegendPosition, Paragraph};
use std::fmt::Write as _;

fn panel_block(theme: &Theme, active: ReadPanel, config: &Config) -> Block<'static> {
    if !config.display.inactive_tabs {
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

type Shade = (usize, usize, Style);

fn shaded_row(text: String, style: Style, mut shades: Vec<Shade>) -> TableRow {
    if shades.is_empty() {
        return TableRow::plain(text, style);
    }
    shades.sort_by_key(|&(start, _, _)| start);
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::with_capacity(shades.len() * 2 + 1);
    let mut cursor = 0;
    for (start, width, shade) in shades {
        let end = start.saturating_add(width).clamp(cursor, chars.len());
        let start = start.clamp(cursor, end);
        spans.push(Span::styled(
            chars[cursor..start].iter().collect::<String>(),
            style,
        ));
        spans.push(Span::styled(
            chars[start..end].iter().collect::<String>(),
            shade,
        ));
        cursor = end;
    }
    spans.push(Span::styled(
        chars[cursor..].iter().collect::<String>(),
        style,
    ));
    TableRow { spans, style }
}

fn mark_trend(text: &mut String, at: usize, trend: Trend) -> Option<usize> {
    let (index, ' ') = text.char_indices().nth(at)? else {
        return None;
    };
    let mut arrow = [0; 4];
    text.replace_range(index..=index, trend.arrow().encode_utf8(&mut arrow));
    Some(at)
}

fn trend_shade(
    text: &mut String,
    at: Option<usize>,
    trend: Option<(Trend, f32)>,
    colored: bool,
    theme: &Theme,
) -> Option<Shade> {
    let (trend, heat) = trend?;
    let at = mark_trend(text, at?, trend)?;
    colored.then(|| (at, 1, theme.trend_style(trend, heat)))
}

const BITS_WIDTH: usize = 19;

const fn bit_flipped_at(flipped: u16, offset: usize) -> bool {
    if offset % 5 == 4 {
        return false;
    }
    let position = offset - offset / 5;
    flipped >> (15 - position) & 1 == 1
}

fn bit_shades(start: usize, flipped: u16, plain: Style, hot: Style) -> Vec<Shade> {
    let mut shades: Vec<Shade> = Vec::new();
    for offset in 0..BITS_WIDTH {
        let style = if bit_flipped_at(flipped, offset) {
            hot
        } else {
            plain
        };
        match shades.last_mut() {
            Some((_, width, last)) if *last == style => *width += 1,
            _ => shades.push((start + offset, 1, style)),
        }
    }
    shades
}

fn flip_shades(
    segment: Option<RowSegment>,
    lead: u16,
    (flipped, heat): (u16, f32),
    zebra: bool,
    theme: &Theme,
) -> Vec<Shade> {
    match segment {
        Some(segment) if flipped != 0 => bit_shades(
            lead as usize + segment.start,
            flipped,
            theme.row_style(zebra, None),
            theme.row_style(zebra, Some(heat)),
        ),
        _ => Vec::new(),
    }
}

fn selection_shades(
    address: Option<RowSegment>,
    lead: u16,
    show_window: bool,
    theme: &Theme,
) -> Vec<Shade> {
    let style = theme.selected_style();
    let mut shades = Vec::with_capacity(2);
    if show_window {
        shades.push((0, 1, style));
    }
    if let Some(address) = address {
        shades.push((lead as usize + address.start, address.width, style));
    }
    shades
}

fn continuation_shade(segment: RowSegment, lead: u16, style: Style, theme: &Theme) -> Shade {
    (
        segment.start + lead as usize,
        segment.width,
        style.patch(theme.dim_style()),
    )
}

fn trend_column(app: &App, lead: u16) -> Option<usize> {
    let address = app.interpreter.segment(Column::Address)?;
    Some(lead as usize + address.start + address.width)
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
    layout: &'a mut ScreenLayout,
}

impl TableCtx<'_> {
    fn horizontal_offset(
        &mut self,
        rows: &[(String, Style, Vec<Shade>)],
        header: &str,
        prefix: u16,
    ) -> u16 {
        let widths = rows
            .iter()
            .map(|(t, _, _)| t.chars().count())
            .chain(std::iter::once(header.chars().count()));
        let max_offset = max_h_offset(widths, prefix as usize, self.inner_width as usize);
        self.layout.h_max_offset = max_offset;
        self.params.col_offset.min(max_offset)
    }

    fn scrollable_table(
        &mut self,
        rows: Vec<(String, Style, Vec<Shade>)>,
        header: &str,
        prefix: u16,
        block: Block<'static>,
        pick: Option<(usize, usize, usize)>,
    ) -> RowsTable {
        let h_off = self.horizontal_offset(&rows, header, prefix);
        let table_rows = rows
            .into_iter()
            .enumerate()
            .map(|(index, (text, style, mut shades))| {
                if let Some((row, start, width)) = pick
                    && row == index
                {
                    shades.push((start, width, style.add_modifier(Modifier::REVERSED)));
                }
                shaded_row(text, style, shades)
            })
            .collect();
        rows_table(table_rows, header.to_string(), self.theme, block).hscroll(prefix, h_off)
    }

    fn pick_spans(&self, row: Option<usize>, lead: u16) -> Option<(usize, usize, usize)> {
        let segment = self.app.copy_column_segment()?;
        Some((row?, segment.start + lead as usize, segment.width))
    }

    fn main_table(&mut self, visible: u16, header: &str, ascii: Option<&str>) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Utc::now();
        let mut rows: Vec<(String, Style, Vec<Shade>)> = Vec::with_capacity(visible as usize);
        let mut pick_row = None;

        let show_window = app.config.display.read_window;
        let lead = u16::from(show_window);
        let custom_segment = app.interpreter.segment(Column::Custom);
        let trend_at = trend_column(app, lead);
        let bits_segment = app.interpreter.segment(Column::Bits);
        let colored = app.config.display.changed_trend_color;
        let address_segment = app.interpreter.segment(Column::Address);
        let address_only = app.config.display.select_address_only && address_segment.is_some();
        let (read_start, read_amount) = app.read_window();
        let read_end = read_start.saturating_add(read_amount - 1);
        let extra = if show_window {
            app.main_extra_cells()
        } else {
            Vec::new()
        };

        for i in 0..visible {
            let Some(addr) = params.window_start.checked_add(i) else {
                break;
            };
            let selected = addr == params.position;
            let zebra = i % 2 == 1;
            let cell = (params.register_type, addr);

            let mut text = app.interpreter.row_buffer();
            if show_window {
                let in_window = (read_start..=read_end).contains(&addr) || extra.contains(&cell);
                text.push(if in_window { '|' } else { ' ' });
            }
            let (base_style, continuation, changed) =
                match app.cell_row_into(&mut text, cell, now, true) {
                    Some(flags) => {
                        let heat = flags.changed.map(|c| c.heat);
                        (
                            theme.row_style(zebra, heat),
                            flags.continuation,
                            flags.changed,
                        )
                    }
                    None => (
                        theme.placeholder_style(zebra),
                        app.placeholder_row_into(&mut text, cell, true),
                        None,
                    ),
                };
            let full = selected && !address_only;
            let style = if full {
                theme.selected_style()
            } else {
                base_style
            };
            let mut shades: Vec<Shade> = custom_segment
                .filter(|_| continuation && !full)
                .map(|segment| continuation_shade(segment, lead, style, theme))
                .into_iter()
                .collect();
            let trend = changed.and_then(|c| Some((c.trend?, c.heat)));
            shades.extend(trend_shade(
                &mut text,
                trend_at,
                trend,
                colored && !full,
                theme,
            ));
            if let Some(flip) = changed.map(|c| (c.flipped, c.heat))
                && !full
            {
                shades.extend(flip_shades(bits_segment, lead, flip, zebra, theme));
            }
            if selected && !full {
                shades.extend(selection_shades(address_segment, lead, show_window, theme));
            }

            if selected {
                pick_row = Some(rows.len());
            }
            rows.push((text, style, shades));
        }

        let mut block = panel_block(theme, ReadPanel::Main, &app.config);
        if let Some(ascii) = ascii {
            block = block.title_top(ascii_title(ascii, theme));
        }

        let pick = self.pick_spans(pick_row, lead);
        if show_window {
            let header = format!(" {header}");
            self.scrollable_table(
                rows,
                &header,
                1 + app.interpreter.prefix_width(),
                block,
                pick,
            )
        } else {
            self.scrollable_table(rows, header, app.interpreter.prefix_width(), block, pick)
        }
    }

    fn list_table(&mut self, cells: &[RegisterCell], top: usize, ascii: Option<&str>) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let now = Utc::now();
        let show_window = app.config.display.read_window;
        let read_cells = show_window.then(|| app.panel_read_cells());
        let type_marker = !app.config.filter_panels_by_type;
        let mut header = app.interpreter.header().to_string();
        if type_marker {
            header.insert_str(0, "T ");
        }
        if show_window {
            header.insert(0, ' ');
        }

        let lead = 2 * u16::from(type_marker) + u16::from(show_window);
        let custom_segment = app.interpreter.segment(Column::Custom);
        let trend_at = trend_column(app, lead);
        let bits_segment = app.interpreter.segment(Column::Bits);
        let colored = app.config.display.changed_trend_color;
        let address_segment = app.interpreter.segment(Column::Address);
        let address_only = app.config.display.select_address_only && address_segment.is_some();
        let mut rows: Vec<(String, Style, Vec<Shade>)> = Vec::with_capacity(cells.len() + 3);
        let mut pick_row = None;
        let mut prev_kind: Option<RegisterType> = None;
        for (ord, &(kind, address)) in cells.iter().enumerate() {
            if prev_kind.is_some_and(|pk| pk != kind) {
                rows.push((String::new(), theme.base(), Vec::new()));
            }
            prev_kind = Some(kind);

            let mut text = app.interpreter.row_buffer();
            if let Some(read_cells) = &read_cells {
                let marker = if read_cells.contains(&(kind, address)) {
                    '|'
                } else {
                    ' '
                };
                text.push(marker);
            }
            if type_marker {
                let _ = write!(text, "{:<2}", kind.marker());
            }
            let (changed, continuation) =
                match app.cell_row_into(&mut text, (kind, address), now, false) {
                    Some(flags) => (flags.changed, flags.continuation),
                    None => (
                        None,
                        app.placeholder_row_into(&mut text, (kind, address), false),
                    ),
                };

            let selected = (top + ord) as u16 == params.pinned_index;
            let zebra = ord % 2 == 1;
            let full = selected && !address_only;
            if selected {
                pick_row = Some(rows.len());
            }
            let style = if full {
                theme.selected_style()
            } else {
                theme.row_style(zebra, changed.map(|c| c.heat))
            };
            let mut shades: Vec<Shade> = custom_segment
                .filter(|_| continuation && !full)
                .map(|segment| continuation_shade(segment, lead, style, theme))
                .into_iter()
                .collect();
            let trend = changed.and_then(|c| Some((c.trend?, c.heat)));
            shades.extend(trend_shade(
                &mut text,
                trend_at,
                trend,
                colored && !full,
                theme,
            ));
            if let Some(flip) = changed.map(|c| (c.flipped, c.heat))
                && !full
            {
                shades.extend(flip_shades(bits_segment, lead, flip, zebra, theme));
            }
            if selected && !full {
                shades.extend(selection_shades(address_segment, lead, show_window, theme));
            }
            rows.push((text, style, shades));
        }

        let mut block = panel_block(theme, params.panel, &app.config);
        if let Some(ascii) = ascii {
            block = block.title_top(ascii_title(ascii, theme));
        }

        let pick = self.pick_spans(pick_row, lead);
        let prefix = lead + app.interpreter.prefix_width();
        self.scrollable_table(rows, &header, prefix, block, pick)
    }

    fn matrix_table(&self, visible: u16) -> RowsTable {
        let (params, app, theme) = (self.params, self.app, self.theme);
        let cols = app.matrix_cols();
        let base = params.window_start - (params.window_start % cols);

        let show_window = app.config.display.read_window;
        let (read_start, read_amount) = app.read_window();
        let read_end = read_start.saturating_add(read_amount - 1);
        let extra = if show_window {
            app.main_extra_cells()
        } else {
            Vec::new()
        };

        let colored = app.config.display.changed_trend_color;
        let address_mode = app.interpreter.address_mode();
        let mut header = format!("{: >5}  ", "");
        for c in 0..cols {
            header.push_str(&format!("{: >5} ", format!("+{}", address_mode.display(c))));
        }

        let mut table_rows: Vec<TableRow> = Vec::with_capacity(visible as usize);
        for r in 0..visible {
            let row_base = (base as u32) + (r as u32) * (cols as u32);
            if row_base > u16::MAX as u32 {
                break;
            }
            let row_base = row_base as u16;
            let zebra = r % 2 == 1;

            let mut spans = vec![Span::styled(
                format!("{: >5}: ", address_mode.display(row_base)),
                theme.dim_style(),
            )];
            for c in 0..cols {
                let Some(addr) = row_base.checked_add(c) else {
                    break;
                };
                let cell = (params.register_type, addr);
                let changed = app.cell_changed(cell);
                let heat = changed.map(|c| c.heat);
                let (text, mut style) = match app.cell_value(cell) {
                    Some(value) => {
                        let style = theme.row_style(zebra, heat);
                        (format!("{value: >5}"), style)
                    }
                    None => (format!("{NO_VALUE: >5}"), theme.placeholder_style(zebra)),
                };
                if addr == params.position {
                    style = theme.selected_style();
                }
                if show_window
                    && ((read_start..=read_end).contains(&addr)
                        || extra.contains(&(params.register_type, addr)))
                {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                spans.push(Span::styled(text, style));
                spans.push(match changed.and_then(|c| Some((c.trend?, c.heat))) {
                    Some((trend, heat)) if colored => {
                        Span::styled(trend.arrow().to_string(), theme.trend_style(trend, heat))
                    }
                    Some((trend, heat)) => Span::styled(
                        trend.arrow().to_string(),
                        theme.row_style(false, Some(heat)),
                    ),
                    None => Span::raw(" "),
                });
            }
            table_rows.push(TableRow {
                spans,
                style: Style::default(),
            });
        }

        let mut block = panel_block(theme, ReadPanel::Matrix, &app.config);
        if let Some(context) = self.matrix_context_line() {
            block = block.title_bottom(context.left_aligned());
        }
        rows_table(table_rows, header, theme, block)
    }

    fn matrix_context_line(&self) -> Option<Line<'static>> {
        let (params, app, theme) = (self.params, self.app, self.theme);
        if !app.config.matrix.show_context {
            return None;
        }
        let cell = (params.register_type, params.position);
        let custom = app.custom_text(cell);
        let label = app.label(cell);
        if custom.is_none() && label.is_none() {
            return None;
        }
        let mut spans = vec![Span::raw(" ")];
        if let Some(custom) = custom {
            spans.push(Span::styled("Custom: ", theme.dim_style()));
            spans.push(Span::styled(custom, theme.accent_style()));
        }
        if let Some(label) = label {
            if spans.len() > 1 {
                spans.push(Span::styled(" ", theme.dim_style()));
            }
            spans.push(Span::styled("Label: ", theme.dim_style()));
            spans.push(Span::styled(label.to_string(), theme.base()));
        }
        spans.push(Span::raw(" "));
        Some(Line::from(spans))
    }
}

pub fn draw(
    params: &ReadParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
    layout: &mut ScreenLayout,
) {
    let (info_type, info_addr) = app.cursor_cell();
    let is_pinned = app
        .pinned_registers
        .iter()
        .any(|&(kind, address)| kind == info_type && address == info_addr);

    let show_ascii_strip = app.config.display.ascii_strip && !params.graph;
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

    let mut identity: Vec<Vec<Span>> = vec![
        vec![
            Span::styled("batch ", theme.dim_style()),
            Span::styled(app.config.batch.size.to_string(), theme.base()),
        ],
        vec![Span::styled(
            format!("{:?}", app.config.device.word_order),
            theme.base(),
        )],
    ];
    if let Some(style) = read_only {
        identity.push(vec![Span::styled("RO", style)]);
    }
    identity.push(vec![
        Span::styled("unit ", theme.dim_style()),
        Span::styled(app.config.device.unit_id.to_string(), theme.base()),
    ]);
    identity.push(vec![Span::styled(device.to_string(), theme.base())]);

    let cycle = &app.config.cycle_register_types;
    let types: Vec<RegisterType> = if app.config.display.inactive_tabs {
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
                format!(
                    "{} @ {}",
                    t.name(),
                    app.interpreter.address_mode().display(info_addr)
                )
            } else {
                t.marker().to_string()
            }
        })
        .collect();

    let mut cell_seg = theme.tab_spans(names, active);
    if is_pinned {
        cell_seg.push(Span::styled(" (pinned)", theme.changed_style()));
    }
    if let Some(d) = params.read_duration {
        cell_seg.push(Span::styled(format!(" - {d:.2?}"), theme.dim_style()));
    }

    let identity = theme.join_dotted(identity);

    let info_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(rows[0]);

    frame.render_widget(Line::from(cell_seg), info_rows[0]);
    frame.render_widget(Line::from(identity).right_aligned(), info_rows[0]);

    if let Some(status) = params.active_status() {
        frame.render_widget(theme.status_line(status), info_rows[1]);
    } else if let Some(error) = &params.read_error {
        frame.render_widget(
            Line::styled(format!("! {error}"), theme.err_style()),
            info_rows[1],
        );
    }

    let header = app.interpreter.header();

    // Tab line + header row; a bottom title (matrix context) takes one more.
    let bottom_row = params.panel == ReadPanel::Matrix && app.config.matrix.show_context && {
        let cell = (params.register_type, params.position);
        app.custom_text(cell).is_some() || app.label(cell).is_some()
    };
    let visible = rows[1]
        .height
        .saturating_sub(2 + u16::from(bottom_row))
        .max(1);
    layout.visible_rows = visible;
    // Inner table width. Panels without interpretation columns leave the offset at zero.
    let inner_width = rows[1].width;
    layout.h_max_offset = 0;

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
            draw_popup(frame, area, theme, app, popup, layout);
        }
        return;
    }

    let mut ctx = TableCtx {
        params,
        app,
        theme,
        inner_width,
        layout,
    };

    match params.panel {
        ReadPanel::Main => {
            let ascii = show_ascii_strip.then(|| {
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
                frame.render_widget(ctx.list_table(&[], 0, None), rows[1]);

                let kb = &app.config.keybinds;
                let hidden = app
                    .panel_hidden_by_type()
                    .then(|| format!("no {} registers here -", params.register_type.name()));
                let (message, hint) = match params.panel {
                    _ if hidden.is_some() => (
                        hidden.as_deref().unwrap_or_default(),
                        Hint::key(kb.register_type, "switch register type"),
                    ),
                    ReadPanel::Labeled => (
                        "no labeled registers yet -",
                        Hint::key(kb.label, "label the selected register"),
                    ),
                    ReadPanel::Custom => (
                        "no custom rules yet -",
                        Hint::key(kb.custom_rule, "add a rule for the selected register"),
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
                let ascii = show_ascii_strip.then(|| app.ascii_string_for(cells.iter().copied()));
                frame.render_widget(ctx.list_table(&cells, top, ascii.as_deref()), rows[1]);
            }
        }
    }

    if let Some(popup) = &params.popup {
        draw_popup(frame, area, theme, app, popup, ctx.layout);
    }
}

pub fn live_status(app: &App, params: &ReadParams, theme: &Theme) -> Vec<Span<'static>> {
    let mut fields: Vec<Vec<Span<'static>>> = Vec::new();

    let (symbol, label, style) = status_parts(&app.connection, theme);
    let idle = !matches!(
        app.connection,
        ConnectionStatus::Reading | ConnectionStatus::Reconnecting
    );
    let interval = Some(app.config.refresh_interval_ms).filter(|&ms| ms > 0 && !app.sweep.active);
    let seconds = |s: f64| format!("{s:>3.1}s");
    let text = match interval {
        _ if params.loading => Some(seconds(params.read_started.elapsed().as_secs_f64())),
        _ if idle && app.paused => None,
        Some(interval) if idle && app.device.is_some() => {
            let remaining =
                (interval as u128).saturating_sub(params.refresh_timer.elapsed().as_millis());
            Some(seconds(remaining as f64 / 1000.0))
        }
        _ => None,
    };
    let mut field = vec![Span::styled(format!("{symbol} "), style)];
    match text {
        Some(text) => {
            field.push(Span::styled(text, style));
            if app.config.display.connection_label {
                field.push(Span::styled(format!(" {label}"), theme.dim_style()));
            }
        }
        None if idle && app.paused => {
            field.push(Span::styled("paused".to_string(), theme.warn_style()));
        }
        None => field.push(Span::styled(label.to_string(), style)),
    }
    fields.push(field);

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
                app.interpreter.address_mode().display(app.sweep.from),
                app.interpreter.address_mode().display(app.sweep.to),
                percent.min(99),
            ),
            theme.accent_style(),
        )]);
    }

    let mut spans = theme.join_dotted(fields);
    spans.push(Span::raw(" "));
    spans
}

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
                    Hint::key(kb.refresh, "Read"),
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

    let time_axis = app.config.graph.time_axis;
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
                let v = (y_hi - y_lo).mul_add(i as f64 / (y_count - 1) as f64, y_lo);
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
        app.label_text(c.0, c.1).unwrap_or_else(|| {
            format!(
                "{}{}",
                c.0.marker(),
                app.interpreter.address_mode().display(c.1)
            )
        })
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
        let style = theme.series_style(i);
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
    axis_line.push_str(symbols::line::BOTTOM_LEFT);
    axis_line.push_str(&symbols::line::HORIZONTAL.repeat(width.saturating_sub(y_gutter + 1)));
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

#[cfg(test)]
mod tests {
    use super::{bit_flipped_at, bit_shades, mark_trend, selection_shades, shaded_row};
    use crate::app::Trend;
    use crate::config::{Column, InterpretorConfig};
    use crate::interpretator::Interpretor;
    use crate::modbus::WordOrder;
    use crate::tui::theme::Theme;
    use ratatui::style::{Color, Style};

    #[test]
    fn shades_split_the_row_in_order_whatever_order_they_come_in() {
        let (base, red, blue) = (
            Style::default(),
            Style::default().fg(Color::Red),
            Style::default().fg(Color::Blue),
        );
        let row = shaded_row(
            "abcdefgh".to_string(),
            base,
            vec![(5, 2, blue), (1, 1, red)],
        );
        let parts: Vec<(&str, Style)> = row
            .spans
            .iter()
            .map(|span| (span.content.as_ref(), span.style))
            .collect();
        assert_eq!(
            parts,
            [
                ("a", base),
                ("b", red),
                ("cde", base),
                ("fg", blue),
                ("h", base)
            ]
        );
    }

    #[test]
    fn an_overlapping_shade_starts_where_the_previous_one_ended() {
        let style = Style::default().fg(Color::Red);
        let row = shaded_row(
            "abcdef".to_string(),
            Style::default(),
            vec![(1, 3, style), (2, 3, style)],
        );
        let text: Vec<&str> = row.spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, ["a", "bcd", "", "e", "f"]);
    }

    #[test]
    fn the_arrow_only_takes_the_place_of_a_space() {
        let mut text = "   12 34".to_string();
        assert_eq!(mark_trend(&mut text, 5, Trend::Up), Some(5));
        assert_eq!(text, "   12▲34");
        assert_eq!(mark_trend(&mut text, 3, Trend::Down), None);
        assert_eq!(mark_trend(&mut text, 40, Trend::Down), None);
        assert_eq!(text, "   12▲34");
    }

    #[test]
    fn flipped_bits_map_onto_their_digits_in_the_grouped_column() {
        let offsets = |flipped: u16| -> Vec<usize> {
            (0..19).filter(|&o| bit_flipped_at(flipped, o)).collect()
        };
        assert_eq!(offsets(0x8000), [0]);
        assert_eq!(offsets(0x0001), [18]);
        assert_eq!(offsets(0x0010), [13]);
        assert_eq!(offsets(0x0800), [5]);
        assert_eq!(offsets(0xFFFF).len(), 16);
        assert!(offsets(0xFFFF).iter().all(|o| o % 5 != 4));
    }

    #[test]
    fn neighbouring_flipped_bits_share_one_shade() {
        let (plain, hot) = (Style::default(), Style::default().fg(Color::Yellow));
        assert_eq!(
            bit_shades(10, 0b0000_0000_0011_1000, plain, hot),
            [
                (10, 12, plain),
                (22, 2, hot),
                (24, 1, plain),
                (25, 1, hot),
                (26, 3, plain)
            ]
        );
    }

    #[test]
    fn an_address_only_selection_covers_the_read_marker_and_the_address() {
        let interpreter = Interpretor::new(InterpretorConfig::default(), WordOrder::ABCD);
        let address = interpreter.segment(Column::Address).expect("address");
        let theme = Theme::default();
        let style = theme.selected_style();
        assert_eq!(
            selection_shades(Some(address), 1, true, &theme),
            [(0, 1, style), (1 + address.start, address.width, style)]
        );
        assert_eq!(
            selection_shades(Some(address), 3, false, &theme),
            [(3 + address.start, address.width, style)]
        );
        assert!(selection_shades(None, 0, false, &theme).is_empty());
    }
}
