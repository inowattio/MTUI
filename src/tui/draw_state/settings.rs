use crate::app::App;
use crate::config::KeybindAction;
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
    if params.editing_keybinds {
        draw_keybinds(params, app, frame, area, theme);
        return;
    }

    let mut lines: Vec<Line> = vec![Line::default()];

    for (i, &field) in SettingsField::ALL.iter().enumerate() {
        if matches!(
            field,
            SettingsField::ClearPins | SettingsField::EditKeybinds | SettingsField::Save
        ) {
            lines.push(Line::default());
        }
        lines.push(render_field(
            app,
            params,
            field,
            i as u16 == params.selected,
            theme,
        ));
        if field == SettingsField::LogWrites {
            lines.push(Line::from(Span::styled(
                format!("  {:<24} {}", "", app.writes_log_path_string()),
                theme.dim_style(),
            )));
        }
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

    if area.height > 0 {
        let footer = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                concat!("  ", env!("CARGO_PKG_REPOSITORY")),
                theme.dim_style(),
            ))),
            footer,
        );
        if app.dirty {
            frame.render_widget(
                Paragraph::new(
                    Line::from(Span::styled(
                        "\u{25cf} unsaved changes  ",
                        theme.warn_style(),
                    ))
                    .right_aligned(),
                ),
                footer,
            );
        }
    }
}

fn render_field(
    app: &App,
    params: &SettingsParams,
    field: SettingsField,
    selected: bool,
    theme: &Theme,
) -> Line<'static> {
    let (name, value, kind) = field_view(app, params, field);

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

fn field_view(
    app: &App,
    params: &SettingsParams,
    field: SettingsField,
) -> (&'static str, String, Kind) {
    let device = &app.config;
    match field {
        SettingsField::RegistersBatch => (
            "Registers batch",
            device.registers_batch.to_string(),
            Kind::Number,
        ),
        SettingsField::AutoUpdate => (
            "Auto-update (ms)",
            device
                .update_interval_ms
                .map_or_else(|| "off".to_string(), |n| n.to_string()),
            Kind::Number,
        ),
        SettingsField::HistoryCap => (
            "Graph history cap",
            device.graph_history_cap.to_string(),
            Kind::Number,
        ),
        SettingsField::MatrixCols => (
            "Matrix columns",
            device.matrix_cols.to_string(),
            Kind::Number,
        ),
        SettingsField::IgnoreDirty => (
            "Ignore unsaved warning",
            if device.ignore_dirty { "on" } else { "off" }.to_string(),
            Kind::Toggle,
        ),
        SettingsField::ReadOnly => (
            "Read-only",
            if device.read_only { "on" } else { "off" }.to_string(),
            Kind::Toggle,
        ),
        SettingsField::ApiPort => (
            "API port",
            match device.port {
                None => "off".to_string(),
                Some(0) => match app.api_bound_port() {
                    Some(bound) => format!("any (:{bound})"),
                    None => "any".to_string(),
                },
                Some(n) => n.to_string(),
            },
            Kind::Number,
        ),
        SettingsField::LogWrites => (
            "Log writes to file",
            if device.log_writes { "on" } else { "off" }.to_string(),
            Kind::Toggle,
        ),
        SettingsField::StartupPanel => (
            "Startup panel",
            device.startup.panel.name().to_string(),
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
        SettingsField::ClearCustom => (
            "Clear custom rules",
            format!("{} rules", app.custom_count()),
            Kind::Action,
        ),
        SettingsField::ShowContinuation => (
            "Show \"part of\" marker",
            if device.custom_rules.show_continuation {
                "on"
            } else {
                "off"
            }
            .to_string(),
            Kind::Toggle,
        ),
        SettingsField::EditKeybinds => ("Edit keybinds", "open".to_string(), Kind::Action),
        SettingsField::Save => (
            "Save configuration",
            app.config_path().to_string(),
            Kind::Action,
        ),
        SettingsField::LoadConfig => ("Load configuration", params.load_path.clone(), Kind::Number),
    }
}

fn draw_keybinds(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let kb = &app.config.keybinds;
    let actions = KeybindAction::ALL;
    let count = actions.len() as u16;

    let mut lines: Vec<Line> = vec![Line::default()];
    lines.push(Line::from(Span::styled(
        format!("  Keybinds  ({}/{})", params.kb_selected + 1, count),
        theme.base(),
    )));
    lines.push(Line::default());

    let top = params.kb_top;
    let end = (top + SettingsParams::KB_VISIBLE).min(count);
    for idx in top..end {
        let action = actions[idx as usize];
        let key = kb.get(action);
        let selected = idx == params.kb_selected;
        let capturing = selected && params.kb_capturing;

        let marker = if selected { "> " } else { "  " };
        let style = if selected {
            theme.selected_style()
        } else {
            theme.base()
        };

        let value = if capturing {
            "press a key\u{2026}".to_string()
        } else {
            key.to_string()
        };

        let mut spans = vec![
            Span::styled(format!("{marker}{:<22} ", action.label()), theme.dim_style()),
            Span::styled(value, style),
        ];

        let duplicate = actions.iter().filter(|&&a| kb.get(a) == key).count() > 1;
        if duplicate && !capturing {
            spans.push(Span::styled(" \u{b7} duplicate", theme.warn_style()));
        }

        lines.push(Line::from(spans));
    }

    lines.push(Line::default());
    let hint = if params.kb_capturing {
        "Esc \u{b7} cancel".to_string()
    } else {
        "Enter \u{b7} rebind   Backspace \u{b7} reset to default   Esc \u{b7} back".to_string()
    };
    lines.push(Line::from(Span::styled(format!("  {hint}"), theme.dim_style())));

    if app.dirty {
        lines.push(Line::from(Span::styled(
            "  \u{25cf} unsaved changes",
            theme.warn_style(),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}
