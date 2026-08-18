use crate::app::{ApiBindState, App};
use crate::config::KeybindAction;
use crate::input::KeyCode;
use crate::state::{SettingsCategory, SettingsField, SettingsFocus, SettingsParams};
use crate::tui::draw_state::{edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

const CATEGORY_WIDTH: u16 = 18;

fn on_off(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

fn color_view(name: &'static str, color: Color) -> (&'static str, String, Option<Color>) {
    (name, color.to_string(), Some(color))
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
    let (name, value, color) = field_view(app, params, field);

    let value_text = if selected && field.is_action() {
        format!("{value}  \u{2190} enter")
    } else {
        edit_value(value, selected, field.is_toggle() || field.is_theme_color())
    };

    match color {
        Some(color) => color_row(theme, name, value_text, color, selected),
        None => field_row(theme, name, 24, value_text, selected),
    }
}

fn color_row(
    theme: &Theme,
    label: &str,
    value: String,
    color: Color,
    selected: bool,
) -> Line<'static> {
    let mut line = field_row(theme, label, 24, format!("{value:<18}"), selected);
    line.spans.push(Span::styled(
        "\u{2588}\u{2588}\u{2588}",
        Style::default().fg(color),
    ));
    line
}

fn field_view(
    app: &App,
    params: &SettingsParams,
    field: SettingsField,
) -> (&'static str, String, Option<Color>) {
    let device = &app.config;
    match field {
        SettingsField::Name => ("Config name", device.name.clone(), None),
        SettingsField::RegistersBatch => {
            ("Registers batch", device.registers_batch.to_string(), None)
        }
        SettingsField::AutoUpdate => (
            "Auto-update (ms)",
            device
                .update_interval_ms
                .map_or_else(|| "off".to_string(), |n| n.to_string()),
            None,
        ),
        SettingsField::HistoryCap => (
            "Graph history cap",
            device.graph_history_cap.to_string(),
            None,
        ),
        SettingsField::MatrixCols => ("Matrix columns", device.matrix_cols.to_string(), None),
        SettingsField::IgnoreDirty => ("Ignore unsaved warning", on_off(device.ignore_dirty), None),
        SettingsField::ShowMock => ("Show mock device", on_off(device.show_mock), None),
        SettingsField::ReadOnly => ("Read-only", on_off(device.read_only), None),
        SettingsField::ApiPort => (
            "API port",
            match device.port {
                None => "off".to_string(),
                Some(0) if app.api_bind_state() == ApiBindState::Failed => {
                    "any (bind failed)".to_string()
                }
                Some(n) if app.api_bind_state() == ApiBindState::Failed => {
                    format!("{n} (bind failed)")
                }
                Some(0) => match app.api_bound_port() {
                    Some(bound) => format!("any (:{bound})"),
                    None => "any".to_string(),
                },
                Some(n) => n.to_string(),
            },
            None,
        ),
        SettingsField::ApiSlaveOverride => (
            "API slave id override",
            on_off(device.allow_api_slave_id),
            None,
        ),
        SettingsField::LogWrites => ("Log writes to file", on_off(device.log_writes), None),
        SettingsField::StartupPanel => (
            "Startup panel",
            device.startup.panel.name().to_string(),
            None,
        ),
        SettingsField::StartupType => (
            "Startup type",
            device.startup.register_type.name().to_string(),
            None,
        ),
        SettingsField::StartupAddress => {
            ("Startup address", device.startup.address.to_string(), None)
        }
        SettingsField::CycleHoldings => {
            ("Cycle holdings", on_off(device.cycle_types.holdings), None)
        }
        SettingsField::CycleInputs => ("Cycle inputs", on_off(device.cycle_types.inputs), None),
        SettingsField::CycleCoils => ("Cycle coils", on_off(device.cycle_types.coils), None),
        SettingsField::CycleDiscretes => (
            "Cycle discretes",
            on_off(device.cycle_types.discretes),
            None,
        ),
        SettingsField::ClearPins => (
            "Clear pinned registers",
            format!("{} pinned", app.pinned_registers.len()),
            None,
        ),
        SettingsField::ClearLabels => (
            "Clear labels",
            format!("{} labels", app.label_count()),
            None,
        ),
        SettingsField::ClearCustom => (
            "Clear custom rules",
            format!("{} rules", app.custom_count()),
            None,
        ),
        SettingsField::ShowContinuation => (
            "Show \"part of\" marker",
            on_off(device.custom_rules.show_continuation),
            None,
        ),
        SettingsField::ShowClock => ("Show clock", on_off(device.show_clock), None),
        SettingsField::ShowFrameTime => (
            "Show frame render time",
            on_off(device.show_frame_time),
            None,
        ),
        SettingsField::ShowRam => ("Show RAM usage", on_off(device.show_ram), None),
        SettingsField::ShowAscii => ("Show ASCII of all data", on_off(device.show_ascii), None),
        SettingsField::ShowInactiveTabs => (
            "Show inactive tabs",
            on_off(device.show_inactive_tabs),
            None,
        ),
        SettingsField::ShowReadWindow => {
            ("Show read window", on_off(device.show_read_window), None)
        }
        SettingsField::GraphTimeAxis => (
            "Graph X axis",
            if device.graph_time_axis {
                "time".to_string()
            } else {
                "samples".to_string()
            },
            None,
        ),
        SettingsField::PaddingHorizontal => (
            "Horizontal padding",
            device.padding_horizontal.to_string(),
            None,
        ),
        SettingsField::PaddingVertical => (
            "Vertical padding",
            device.padding_vertical.to_string(),
            None,
        ),
        SettingsField::ChangedExpiry => (
            "Changed highlight (ms)",
            device
                .changed_expiry_ms
                .map_or_else(|| "never".to_string(), |n| n.to_string()),
            None,
        ),
        SettingsField::ThemePreset => (
            "Preset",
            Theme::PRESETS
                .iter()
                .find(|&&(_, t)| t == device.theme)
                .map_or_else(|| "custom".to_string(), |&(name, _)| name.to_string()),
            None,
        ),
        SettingsField::ThemeBorder => color_view("Frame border", device.theme.border),
        SettingsField::ThemeAccent => color_view("Accent / titles", device.theme.accent),
        SettingsField::ThemeText => color_view("Text", device.theme.text),
        SettingsField::ThemeBg => color_view("Background", device.theme.bg),
        SettingsField::ThemeDim => color_view("Dim / muted", device.theme.dim),
        SettingsField::ThemeChanged => color_view("Changed value", device.theme.changed),
        SettingsField::ThemeZebra => color_view("Zebra stripe", device.theme.zebra),
        SettingsField::ThemeOk => color_view("OK / connected", device.theme.ok),
        SettingsField::ThemeWarn => color_view("Warning", device.theme.warn),
        SettingsField::ThemeErr => color_view("Error", device.theme.err),
        SettingsField::ThemeSelectedFg => color_view("Selected text", device.theme.selected_fg),
        SettingsField::ThemeSelectedBg => color_view("Selected bg", device.theme.selected_bg),
        SettingsField::Save => ("Save configuration", app.config_path().to_string(), None),
        SettingsField::LoadConfig => ("Load configuration", params.load_path.clone(), None),
        SettingsField::NextConfig => ("Next configuration", device.next_config.clone(), None),
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
