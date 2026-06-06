use crate::app::App;
use crate::state::{no_data_text, ReadParams};
use crate::tui::theme::{spinner_frame, Theme};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

/// Builds a bordered, header-topped table for a register panel.
///
/// `changed` is aligned 1:1 with `rows`. Style precedence per row is
/// selected (the cursor bar) > changed > zebra > base.
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

    // Info line: device, active address, register type, read time / spinner,
    // and a live auto-refresh countdown.
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
        format!("   {:?}   ", params.register_type),
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

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(rows[1]);

    let header = app.interpreter.header();

    // Cursor only when we have register data (changed is non-empty on a
    // successful read; empty for the placeholder / error states) AND the active
    // register falls within the displayed block. Navigating the cursor off the
    // last-read block hides the bar until the next read re-anchors it.
    let selected = if !params.main_changed.is_empty()
        && params.position >= params.window_start
        && ((params.position - params.window_start) as usize) < params.main_rows.len()
    {
        Some((params.position - params.window_start) as usize)
    } else {
        None
    };

    frame.render_widget(
        rows_to_table(
            "Main data",
            header.clone(),
            &params.main_rows,
            &params.main_changed,
            selected,
            theme,
        ),
        columns[0],
    );

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
        rows_to_table(
            "Pinned",
            header,
            &pinned_rows,
            &params.pinned_changed,
            None,
            theme,
        ),
        columns[1],
    );

    if show_ascii {
        let ascii_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(rows[2]);

        let main_ascii = Line::from(vec![
            Span::styled("ASCII: ", theme.dim_style()),
            Span::styled(format!("'{}'", params.ascii_string), theme.base()),
        ]);
        frame.render_widget(
            Paragraph::new(main_ascii)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            ascii_columns[0],
        );

        let pinned_ascii = Line::from(vec![
            Span::styled("ASCII: ", theme.dim_style()),
            Span::styled(format!("'{}'", params.pinned_ascii_string), theme.base()),
        ]);
        frame.render_widget(
            Paragraph::new(pinned_ascii)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            ascii_columns[1],
        );
    }
}
