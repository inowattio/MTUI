use crate::app::App;
use crate::constants::CONFIG_PATH;
use crate::state::{SettingsField, SettingsParams};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

enum Kind {
    Number,
    Toggle,
    Action,
}

pub fn draw(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = vec![
        Line::default(),
    ];

    for (i, &field) in SettingsField::ALL.iter().enumerate() {
        if matches!(field, SettingsField::ClearPins | SettingsField::Save) {
            lines.push(Line::default());
        }
        lines.push(render_field(app, field, i as u16 == params.selected, theme));
    }

    if let Some(status) = &params.status {
        lines.push(Line::default());
        let style = if status.contains("failed") {
            theme.err_style()
        } else {
            theme.ok_style()
        };
        lines.push(Line::from(Span::styled(status.clone(), style)));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_field(app: &App, field: SettingsField, selected: bool, theme: &Theme) -> Line<'static> {
    let (name, value, kind) = field_view(app, field);

    let marker = if selected { "> " } else { "  " };
    let style = if selected {
        theme.selected_style()
    } else {
        theme.base()
    };

    let value_text = match (selected, kind) {
        (true, Kind::Toggle) => format!("\u{2039} {value} \u{203a}"),
        (true, Kind::Number) => format!("{value}_"),
        (true, Kind::Action) => format!("{value}  \u{2190} enter"),
        (false, _) => value,
    };

    Line::from(vec![
        Span::styled(format!("{marker}{name:<24} "), theme.dim_style()),
        Span::styled(value_text, style),
    ])
}

fn field_view(app: &App, field: SettingsField) -> (&'static str, String, Kind) {
    let device = &app.config;
    match field {
        SettingsField::RegistersBatch => (
            "Registers batch",
            device.registers_batch.to_string(),
            Kind::Number,
        ),
        SettingsField::AutoUpdate => (
            "Auto-update (seconds)",
            device
                .auto_update_interval_seconds
                .map_or_else(|| "off".to_string(), |n| n.to_string()),
            Kind::Number,
        ),
        SettingsField::HistoryCap => (
            "Graph history cap",
            device.graph_history_cap.to_string(),
            Kind::Number,
        ),
        SettingsField::ReadOnly => (
            "Read-only",
            if device.read_only { "on" } else { "off" }.to_string(),
            Kind::Toggle,
        ),
        SettingsField::ClearPins => (
            "Clear pinned registers",
            format!("{} pinned", app.pinned_registers.len()),
            Kind::Action,
        ),
        SettingsField::ClearLabels => (
            "Clear labels",
            format!("{} labels", app.label_count()),
            Kind::Action,
        ),
        SettingsField::Save => ("Save configuration", CONFIG_PATH.to_string(), Kind::Action),
    }
}
