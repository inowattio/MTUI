use crate::app::{ApiBindState, App};
use crate::config::KeybindAction;
use crate::input::KeyCode;
use crate::state::{SettingsCategory, SettingsField, SettingsFocus, SettingsParams};
use crate::tui::draw_state::{edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

const CATEGORY_WIDTH: u16 = 18;

#[derive(Clone, Copy)]
enum Kind {
    Number,
    Toggle,
    Action,
    Color(Color),
}

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

fn color_view(name: &'static str, color: Color) -> (&'static str, String, Kind) {
    (name, color.to_string(), Kind::Color(color))
}

pub fn draw(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let body = Rect::new(area.x, area.y, area.width, area.height - 1);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(CATEGORY_WIDTH), Constraint::Min(0)])
        .split(body);

    let divider = Block::default()
        .borders(Borders::RIGHT)
        .border_style(theme.dim_style());
    let left_inner = divider.inner(cols[0]);
    frame.render_widget(divider, cols[0]);
    draw_categories(params, frame, left_inner, theme);

    let right = cols[1];
    let right = Rect::new(
        right.x.saturating_add(1),
        right.y,
        right.width.saturating_sub(1),
        right.height,
    );
    if params.current_category().is_keybinds() {
        draw_keybinds(params, app, frame, right, theme);
    } else {
        draw_fields(params, app, frame, right, theme);
    }

    draw_footer(params, app, frame, area, theme);
}

fn draw_categories(params: &SettingsParams, frame: &mut Frame, area: Rect, theme: &Theme) {
    let focused = params.focus == SettingsFocus::Categories;
    let mut lines: Vec<Line> = vec![Line::default()];

    for (i, &category) in SettingsCategory::ALL.iter().enumerate() {
        let selected = i as u16 == params.category;
        let style = match (selected, focused) {
            (true, true) => theme.selected_style(),
            (true, false) => theme.accent_style(),
            (false, _) => theme.dim_style(),
        };
        lines.push(Line::from(Span::styled(
            format!("{}{}", marker(selected), category.label()),
            style,
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_fields(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let focused = params.focus == SettingsFocus::Fields;
    let mut lines: Vec<Line> = vec![Line::default()];
    lines.push(Line::from(Span::styled(
        format!("  {}", params.current_category().label().to_uppercase()),
        theme.accent_style(),
    )));
    lines.push(Line::default());

    for (i, &field) in params.current_fields().iter().enumerate() {
        let selected = focused && i as u16 == params.field;
        lines.push(render_field(app, params, field, selected, theme));
        if field == SettingsField::LogWrites {
            lines.push(Line::from(Span::styled(
                format!("  {:<24} {}", "", app.writes_log_path_string()),
                theme.dim_style(),
            )));
        }
    }

    if matches!(params.current_category(), SettingsCategory::Theme) {
        lines.push(Line::default());
        lines.push(hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Left, KeyCode::Right, "Cycle"),
                Hint::pair(KeyCode::Char('0'), KeyCode::Char('9'), "256-color index"),
                Hint::key(KeyCode::Backspace, "Delete / reset"),
            ],
        ));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_footer(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let footer = Rect::new(area.x, area.y + area.height - 1, area.width, 1);
    if let Some(status) = &params.status {
        frame.render_widget(Paragraph::new(theme.status_line(status)), footer);
    }
    if app.dirty {
        frame.render_widget(
            Paragraph::new(
                Line::from(Span::styled("\u{25cf} unsaved changes", theme.warn_style()))
                    .right_aligned(),
            ),
            footer,
        );
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

    let value_text = match (selected, kind) {
        (true, Kind::Action) => format!("{value}  \u{2190} enter"),
        (s, Kind::Color(_)) => edit_value(value, s, true),
        (s, k) => edit_value(value, s, matches!(k, Kind::Toggle)),
    };

    match kind {
        Kind::Color(color) => color_row(theme, name, value_text, color, selected),
        _ => field_row(theme, name, 24, value_text, selected),
    }
}

fn color_row(
    theme: &Theme,
    label: &str,
    value: String,
    color: Color,
    selected: bool,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}{label:<24} ", marker(selected)),
            theme.dim_style(),
        ),
        Span::styled(format!("{value:<18}"), theme.line_style(selected)),
        Span::styled("\u{2588}\u{2588}\u{2588}", Style::default().fg(color)),
    ])
}

fn field_view(
    app: &App,
    params: &SettingsParams,
    field: SettingsField,
) -> (&'static str, String, Kind) {
    let device = &app.config;
    match field {
        SettingsField::Name => ("Config name", device.name.clone(), Kind::Number),
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
            on_off(device.ignore_dirty),
            Kind::Toggle,
        ),
        SettingsField::ShowMock => ("Show mock device", on_off(device.show_mock), Kind::Toggle),
        SettingsField::ReadOnly => ("Read-only", on_off(device.read_only), Kind::Toggle),
        SettingsField::ApiPort => (
            "API port",
            match device.port {
                None => "off".to_string(),
                _ if app.api_bind_state() == ApiBindState::Failed => match device.port {
                    Some(0) => "any (bind failed)".to_string(),
                    Some(n) => format!("{n} (bind failed)"),
                    None => unreachable!(),
                },
                Some(0) => match app.api_bound_port() {
                    Some(bound) => format!("any (:{bound})"),
                    None => "any".to_string(),
                },
                Some(n) => n.to_string(),
            },
            Kind::Number,
        ),
        SettingsField::ApiSlaveOverride => (
            "API slave id override",
            on_off(device.allow_api_slave_id),
            Kind::Toggle,
        ),
        SettingsField::LogWrites => (
            "Log writes to file",
            on_off(device.log_writes),
            Kind::Toggle,
        ),
        SettingsField::StartupPanel => (
            "Startup panel",
            device.startup.panel.name().to_string(),
            Kind::Toggle,
        ),
        SettingsField::StartupType => (
            "Startup type",
            device.startup.register_type.name().to_string(),
            Kind::Toggle,
        ),
        SettingsField::StartupAddress => (
            "Startup address",
            device.startup.address.to_string(),
            Kind::Number,
        ),
        SettingsField::CycleHoldings => (
            "Cycle holdings",
            on_off(device.cycle_types.holdings),
            Kind::Toggle,
        ),
        SettingsField::CycleInputs => (
            "Cycle inputs",
            on_off(device.cycle_types.inputs),
            Kind::Toggle,
        ),
        SettingsField::CycleCoils => (
            "Cycle coils",
            on_off(device.cycle_types.coils),
            Kind::Toggle,
        ),
        SettingsField::CycleDiscretes => (
            "Cycle discretes",
            on_off(device.cycle_types.discretes),
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
            on_off(device.custom_rules.show_continuation),
            Kind::Toggle,
        ),
        SettingsField::ShowClock => ("Show clock", on_off(device.show_clock), Kind::Toggle),
        SettingsField::ShowFrameTime => (
            "Show frame render time",
            on_off(device.show_frame_time),
            Kind::Toggle,
        ),
        SettingsField::ShowRam => ("Show RAM usage", on_off(device.show_ram), Kind::Toggle),
        SettingsField::ShowAscii => (
            "Show ASCII of all data",
            on_off(device.show_ascii),
            Kind::Toggle,
        ),
        SettingsField::ShowInactiveTabs => (
            "Show inactive tabs",
            on_off(device.show_inactive_tabs),
            Kind::Toggle,
        ),
        SettingsField::ShowReadWindow => (
            "Show read window",
            on_off(device.show_read_window),
            Kind::Toggle,
        ),
        SettingsField::GraphTimeAxis => (
            "Graph X axis",
            if device.graph_time_axis {
                "time".to_string()
            } else {
                "samples".to_string()
            },
            Kind::Toggle,
        ),
        SettingsField::ChangedExpiry => (
            "Changed highlight (ms)",
            device
                .changed_expiry_ms
                .map_or_else(|| "never".to_string(), |n| n.to_string()),
            Kind::Number,
        ),
        SettingsField::ThemeBorder => color_view("Border", device.theme.border),
        SettingsField::ThemeAccent => color_view("Accent / titles", device.theme.accent),
        SettingsField::ThemeText => color_view("Text", device.theme.text),
        SettingsField::ThemeDim => color_view("Dim / muted", device.theme.dim),
        SettingsField::ThemeChanged => color_view("Changed value", device.theme.changed),
        SettingsField::ThemeZebra => color_view("Zebra stripe", device.theme.zebra),
        SettingsField::ThemeOk => color_view("OK / connected", device.theme.ok),
        SettingsField::ThemeWarn => color_view("Warning", device.theme.warn),
        SettingsField::ThemeErr => color_view("Error", device.theme.err),
        SettingsField::ThemeSelectedFg => color_view("Selected text", device.theme.selected_fg),
        SettingsField::Save => (
            "Save configuration",
            app.config_path().to_string(),
            Kind::Action,
        ),
        SettingsField::LoadConfig => ("Load configuration", params.load_path.clone(), Kind::Number),
        SettingsField::NextConfig => (
            "Next configuration",
            device.next_config.clone(),
            Kind::Number,
        ),
    }
}

fn draw_keybinds(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let kb = &app.config.keybinds;
    let actions = KeybindAction::ALL;
    let count = actions.len() as u16;

    let mut lines: Vec<Line> = vec![Line::default()];
    lines.push(Line::from(Span::styled(
        format!("  KEYBINDS  ({}/{})", params.kb_selected + 1, count),
        theme.accent_style(),
    )));
    lines.push(Line::default());

    let top = params.kb_top;
    let end = (top + SettingsParams::KB_VISIBLE).min(count);
    for idx in top..end {
        let action = actions[idx as usize];
        let key = kb.get(action);
        let selected = idx == params.kb_selected;
        let capturing = selected && params.kb_capturing;

        let marker = marker(selected);
        let style = theme.line_style(selected);

        let value = if capturing {
            "press a key\u{2026}".to_string()
        } else {
            key.to_string()
        };

        let mut spans = vec![
            Span::styled(
                format!("{marker}{:<22} ", action.label()),
                theme.dim_style(),
            ),
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
        hints::footer(theme, [Hint::key(KeyCode::Esc, "Cancel")])
    } else {
        hints::footer(
            theme,
            [
                Hint::key(kb.action, "Rebind"),
                Hint::key(KeyCode::Backspace, "Reset to default"),
                Hint::key(KeyCode::Esc, "Back"),
            ],
        )
    };
    lines.push(hint);

    frame.render_widget(Paragraph::new(lines), area);
}
