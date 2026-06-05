use crate::app::App;
use crate::state::{no_data_text, ReadParams};
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

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
    let main_text = format!("Main data\n{}\n{}", header, params.main_data);
    frame.render_widget(
        Paragraph::new(main_text)
            .style(base_style)
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
    let pinned_text = format!("Pinned data\n{}\n{}", header, pinned_state);
    frame.render_widget(
        Paragraph::new(pinned_text)
            .style(base_style)
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
