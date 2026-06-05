use crate::app::App;
use crate::state::{no_data_text, ReadParams};
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

/// Builds a panel's text, highlighting register rows whose value changed since the previous read.
/// `changed` is aligned 1:1 with the register rows in `body`; the title/header rows are never highlighted.
fn build_panel<'a>(
    title: &str,
    header: &str,
    body: &str,
    changed: &[bool],
    base_style: Style,
    changed_style: Style,
) -> Text<'a> {
    let mut lines = vec![
        Line::styled(title.to_string(), base_style),
        Line::styled(header.to_string(), base_style),
    ];

    for (i, row) in body.split('\n').enumerate() {
        let style = if changed.get(i).copied().unwrap_or(false) {
            changed_style
        } else {
            base_style
        };
        lines.push(Line::styled(row.to_string(), style));
    }

    Text::from(lines)
}

pub fn draw(
    params: &ReadParams,
    app: &App,
    frame: &mut Frame,
    outer: Block,
    base_style: Style,
    device: String,
) {
    let is_pinned = app
        .pinned_registers
        .iter()
        .position(|(kind, address)| kind == &params.register_type && *address == params.position)
        .is_some();
    let pinned_string = if is_pinned { " (Pinned)" } else { "" };

    let outer_area = frame.area();
    let inner_area = outer.inner(outer_area);
    frame.render_widget(outer, outer_area);

    let read_time = if params.loading {
        "(loading)".to_string()
    } else {
        params
            .read_duration
            .map(|d| format!("({d:.2?})"))
            .unwrap_or_default()
    };
    let info = format!(
        "Device: {} at: {}{} on {:?} {}",
        device, params.position, pinned_string, params.register_type, read_time
    );
    let show_ascii = app.interpreter.shows_ascii();
    let row_constraints = if show_ascii {
        vec![Constraint::Length(2), Constraint::Min(0), Constraint::Length(1)]
    } else {
        vec![Constraint::Length(2), Constraint::Min(0)]
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(row_constraints)
        .split(inner_area);

    frame.render_widget(
        Paragraph::new(info)
            .style(base_style)
            .alignment(Alignment::Left),
        rows[0],
    );

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(rows[1]);

    let header = app.interpreter.header();
    let changed_style = base_style.fg(Color::Yellow).add_modifier(Modifier::BOLD);

    frame.render_widget(
        Paragraph::new(build_panel(
            "Main data",
            &header,
            &params.main_data,
            &params.main_changed,
            base_style,
            changed_style,
        ))
        .alignment(Alignment::Left),
        columns[0],
    );

    let pinned_state = if params.pinned_data.is_empty() {
        if app.pinned_registers.is_empty() {
            "No pinned registers.".into()
        } else {
            no_data_text()
        }
    } else {
        params.pinned_data.clone()
    };
    frame.render_widget(
        Paragraph::new(build_panel(
            "Pinned data",
            &header,
            &pinned_state,
            &params.pinned_changed,
            base_style,
            changed_style,
        ))
        .alignment(Alignment::Left),
        columns[1],
    );

    if show_ascii {
        let ascii_columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(rows[2]);

        let main_ascii_text = format!("ASCII: '{}'", params.ascii_string);
        frame.render_widget(
            Paragraph::new(main_ascii_text)
                .style(base_style)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            ascii_columns[0],
        );

        let pinned_ascii_text = format!("ASCII: '{}'", params.pinned_ascii_string);
        frame.render_widget(
            Paragraph::new(pinned_ascii_text)
                .style(base_style)
                .alignment(Alignment::Left)
                .wrap(Wrap { trim: false }),
            ascii_columns[1],
        );
    }
}
