use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Gauge, Paragraph};
use crate::app::App;
use crate::state::DumpParams;

pub fn draw(params: &DumpParams, app: &App, frame: &mut Frame, outer: Block, base_style: Style, device: String) {
    let outer_area = frame.area();
    let inner_area = outer.inner(outer_area);
    frame.render_widget(outer, outer_area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
            ]
                .as_ref(),
        )
        .split(inner_area);

    let info = format!(
        "Device: {}\nStart at {} on {:?}",
        device,
        params.start_position,
        params.register_type
    );
    frame.render_widget(
        Paragraph::new(info).style(base_style).alignment(Alignment::Left),
        rows[0],
    );

    let ratio = if let Some(total) = params.total_batches {
        if total == 0 {
            0.0
        } else {
            (params.completed_batches as f64 / total as f64).clamp(0.0, 1.0)
        }
    } else {
        0.0
    };
    let progress_text = match params.total_batches {
        Some(total) => format!("{}/{} batches ({}/{} registers)", params.completed_batches, total, params.completed_batches as usize * app.config.registers_batch as usize, total as usize * app.config.registers_batch as usize),
        None => "Set batch count to start".to_string(),
    };
    let gauge = Gauge::default()
        .gauge_style(base_style)
        .label(progress_text)
        .ratio(ratio);
    frame.render_widget(gauge, rows[1]);

    let status = if params.started { "Running" } else { "Idle" };
    let mut details = format!(
        "Batch size: {} | Status: {}",
        app.config.registers_batch, status
    );
    if let Some(err) = &params.error {
        details.push_str(&format!("\nError: {err}"));
    }
    frame.render_widget(
        Paragraph::new(details).style(base_style).alignment(Alignment::Left),
        rows[2],
    );
}