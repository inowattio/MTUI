use crate::app::App;
use crate::state::{no_data_text, ReadPanel, ReadParams};
use crate::tui::theme::{spinner_frame, Theme};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
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

fn main_table(params: &ReadParams, visible: u16, header: String, theme: &Theme) -> Table<'static> {
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
                (format!("{addr: >5}:  --"), style)
            }
        };

        table_rows.push(Row::new([Cell::from(text)]).style(style));
    }

    Table::new(table_rows, [Constraint::Percentage(100)])
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel("Main data"))
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
    if let Some(interval) = app.config.auto_update_interval_seconds {
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

    let ascii_string = match params.panel {
        ReadPanel::Main => {
            let visible = rows[1].height.saturating_sub(3).max(1);
            app.visible_rows.set(visible);
            frame.render_widget(main_table(params, visible, header, theme), rows[1]);
            &params.ascii_string
        }
        ReadPanel::Pinned => {
            let pinned_rows: Vec<String> = if params.pinned_rows.is_empty() {
                if app.pinned_registers.is_empty() {
                    vec!["No pinned registers.".to_string()]
                } else {
                    vec![no_data_text()]
                }
            } else {
                params.pinned_rows.clone()
            };
            frame.render_widget(
                rows_to_table("Pinned", header, &pinned_rows, &params.pinned_changed, None, theme),
                rows[1],
            );
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
}
