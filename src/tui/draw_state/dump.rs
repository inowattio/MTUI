use crate::app::App;
use crate::state::DumpParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, Paragraph};
use ratatui::Frame;

pub fn draw(
    params: &DumpParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &str,
) {
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
        .split(area);

    let info = Line::from(vec![
        Span::styled("Device: ", theme.dim_style()),
        Span::styled(device.to_string(), theme.base()),
        Span::styled("   start ", theme.dim_style()),
        Span::styled(params.start_position.to_string(), theme.accent_style()),
        Span::styled(format!("   {:?}", params.register_type), theme.base()),
    ]);
    frame.render_widget(Paragraph::new(info).alignment(Alignment::Left), rows[0]);

    let ratio = match params.total_batches {
        Some(total) if total > 0 => {
            (params.completed_batches as f64 / total as f64).clamp(0.0, 1.0)
        }
        _ => 0.0,
    };
    let progress_text = match params.total_batches {
        Some(total) => format!(
            "{}/{} batches ({}/{} registers)",
            params.completed_batches,
            total,
            params.completed_batches as usize * app.config.registers_batch as usize,
            total as usize * app.config.registers_batch as usize
        ),
        None => "Set batch count to start".to_string(),
    };
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(theme.accent))
        .label(Span::styled(progress_text, theme.base()))
        .ratio(ratio);
    frame.render_widget(gauge, rows[1]);

    let (status, status_style) = if params.started {
        ("Running", theme.ok_style())
    } else {
        ("Idle", theme.dim_style())
    };
    let mut detail_lines = vec![Line::from(vec![
        Span::styled("Batch size: ", theme.dim_style()),
        Span::styled(app.config.registers_batch.to_string(), theme.base()),
        Span::styled("   Status: ", theme.dim_style()),
        Span::styled(status, status_style),
    ])];
    if let Some(err) = &params.error {
        detail_lines.push(Line::from(Span::styled(
            format!("Error: {err}"),
            Style::default().fg(theme.err),
        )));
    }
    frame.render_widget(
        Paragraph::new(detail_lines).alignment(Alignment::Left),
        rows[2],
    );
}
