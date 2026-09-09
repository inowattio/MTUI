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

    let mut selected_line = None;
    let mut field_lines = Vec::new();
    let mut index = 0u16;
    for (g, group) in params.current_category().groups().iter().enumerate() {
        if g > 0 {
            lines.push(Line::default());
        }
        for &field in group.iter() {
            let selected = focused && index == params.field;
            if selected {
                selected_line = Some(lines.len());
            }
            field_lines.push(lines.len());
            lines.push(render_field(app, params, field, selected, theme));
            if field == SettingsField::LogWrites {
                lines.push(Line::from(Span::styled(
                    format!("  {:<24} {}", "", app.writes_log_path_string()),
                    theme.dim_style(),
                )));
            }
            index += 1;
        }
    }

    let mut footer_lines = Vec::new();
    if let Some(field) = params.current_field().filter(|_| focused) {
        footer_lines.push(Line::from(Span::styled(
            format!("  {}", description(field)),
            theme.dim_style(),
        )));
    }
    if matches!(params.current_category(), SettingsCategory::Theme) {
        footer_lines.push(hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Left, KeyCode::Right, "Cycle"),
                Hint::pair(KeyCode::Char('0'), KeyCode::Char('9'), "256-color index"),
                Hint::key(KeyCode::Backspace, "Delete / reset"),
            ],
        ));
    }
    let (mut list, footer) = footer_split(area, footer_lines.len() as u16);
    let more_row = more_row(&mut list, footer.is_some(), lines.len());

    let height = list.height as usize;
    let top = scroll_offset(selected_line.unwrap_or(0), lines.len(), height);
    frame.render_widget(Paragraph::new(lines).scroll((top as u16, 0)), list);
    if let Some(row) = more_row {
        let above = field_lines.iter().filter(|&&l| l < top).count();
        let below = field_lines.iter().filter(|&&l| l >= top + height).count();
        frame.render_widget(Paragraph::new(hints::more(theme, above, below)), row);
    }
    render_footer(frame, footer, footer_lines);
}

fn footer_split(area: Rect, rows: u16) -> (Rect, Option<Rect>) {
    if rows == 0 || area.height < rows + 2 {
        return (area, None);
    }
    let list = Rect {
        height: area.height - rows - 1,
        ..area
    };
    let footer = Rect {
        y: area.y + area.height - rows,
        height: rows,
        ..area
    };
    (list, Some(footer))
}

fn more_row(list: &mut Rect, has_hint: bool, len: usize) -> Option<Rect> {
    if len <= list.height as usize {
        return None;
    }
    if !has_hint {
        if list.height < 2 {
            return None;
        }
        list.height -= 1;
    }
    Some(Rect {
        y: list.y + list.height,
        height: 1,
        ..*list
    })
}

fn render_footer(frame: &mut Frame, footer: Option<Rect>, lines: Vec<Line<'static>>) {
    if let Some(footer) = footer {
        frame.render_widget(Paragraph::new(lines), footer);
    }
}

fn description(field: SettingsField) -> &'static str {
    match field {
        SettingsField::Name => "Name shown in the title bar for this configuration",
        SettingsField::RegistersBatch => {
            "How many registers each read request fetches around the cursor"
        }
        SettingsField::BatchAnchor => {
            "Where the cursor sits inside the read batch: start, middle or end"
        }
        SettingsField::ReadFullCustoms => {
            "Also read every register a custom rule spans, even outside the batch"
        }
        SettingsField::CustomBatchBySize => {
            "In the Custom panel, size the batch by registers instead of rules"
        }
        SettingsField::AutoUpdate => "Delay between automatic reads, 0 turns auto-refresh off",
        SettingsField::ReconnectOnTimeout => "Reconnect to the device after a read times out",
        SettingsField::HistoryCap => "Samples kept per register for the value graph",
        SettingsField::MatrixCols => "Registers per row in the Matrix panel",
        SettingsField::ReadOnly => "Refuse all writes from the UI and the API",
        SettingsField::LogWrites => "Append every write to a log file",
        SettingsField::ApiPort => "Port for the HTTP API, 0 picks any free port, off disables it",
        SettingsField::ApiSlaveOverride => {
            "Let API requests target a slave id other than the configured one"
        }
        SettingsField::SavePositionOnExit => {
            "Store the cursor position as the startup position when quitting"
        }
        SettingsField::StartupPanel => "Panel opened on start",
        SettingsField::StartupType => "Register type selected on start",
        SettingsField::StartupAddress => "Address the cursor starts on",
        SettingsField::CycleHoldings => "Include holding registers when cycling register types",
        SettingsField::CycleInputs => "Include input registers when cycling register types",
        SettingsField::CycleCoils => "Include coils when cycling register types",
        SettingsField::CycleDiscretes => "Include discrete inputs when cycling register types",
        SettingsField::CyclePinned => "Include the Pinned panel when cycling panels",
        SettingsField::CycleLabeled => "Include the Labeled panel when cycling panels",
        SettingsField::CycleCustom => "Include the Custom panel when cycling panels",
        SettingsField::CycleMatrix => "Include the Matrix panel when cycling panels",
        SettingsField::IgnoreDirty => {
            "Quit or switch configuration without asking about unsaved changes"
        }
        SettingsField::ShowMock => "Offer the built-in mock device in Discovery",
        SettingsField::ClearPins => "Remove every pinned register",
        SettingsField::ClearLabels => "Remove every label",
        SettingsField::ClearCustom => "Remove every custom rule",
        SettingsField::CopyData => {
            "Copy pins, labels and custom rules as JSON, paste into another MTUI to import"
        }
        SettingsField::CopyConfig => "Copy the whole configuration as JSON, as Save would write it",
        SettingsField::ShowContinuation => {
            "Mark registers that belong to a multi-register custom rule"
        }
        SettingsField::ShowClock => "Show the current time in the bottom bar",
        SettingsField::ShowFrameTime => "Show how long each frame takes to render",
        SettingsField::ShowRam => "Show the memory used by the application",
        SettingsField::ShowAscii => "Show the read registers decoded as an ASCII string",
        SettingsField::ShowInactiveTabs => {
            "Show every panel and register type tab, not just the active one"
        }
        SettingsField::ShowReadWindow => {
            "Highlight the address range covered by the current read batch"
        }
        SettingsField::GraphTimeAxis => "Plot the graph against time instead of sample count",
        SettingsField::ChangedExpiry => {
            "How long a changed value stays highlighted, 0 never clears it"
        }
        SettingsField::PaddingHorizontal => "Empty columns kept on both sides of the interface",
        SettingsField::PaddingVertical => "Empty rows kept above and below the interface",
        SettingsField::ThemePreset => "Switch between the built-in color schemes",
        SettingsField::ThemeBg => "Background color",
        SettingsField::ThemeBorder => "Color of the frame borders",
        SettingsField::ThemeAccent => "Color for titles, keys and highlights",
        SettingsField::ThemeText => "Main text color",
        SettingsField::ThemeDim => "Color for secondary and muted text",
        SettingsField::ThemeChanged => "Color for values that changed recently",
        SettingsField::ThemeZebra => "Background of alternating table rows",
        SettingsField::ThemeOk => "Color for success and connected states",
        SettingsField::ThemeWarn => "Color for warnings",
        SettingsField::ThemeErr => "Color for errors",
        SettingsField::ThemeSelectedFg => "Text color of the selected row",
        SettingsField::ThemeSelectedBg => "Background color of the selected row",
        SettingsField::Save => "Write the current settings to the configuration file",
        SettingsField::LoadConfig => "Path of a configuration file to load now",
        SettingsField::NextConfig => "Configuration file loaded by the cycle config key",
    }
}

fn scroll_offset(selected: usize, len: usize, height: usize) -> usize {
    let height = height.max(1);
    selected
        .saturating_sub(height / 2)
        .min(len.saturating_sub(height))
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

    if app.settings_field_disabled(field) {
        return disabled_row(theme, name, value, selected);
    }

    let value_text = if selected && field.is_action() && value.is_empty() {
        "\u{2190} enter".to_string()
    } else if selected && field.is_action() {
        format!("{value}  \u{2190} enter")
    } else {
        edit_value(value, selected, field.is_toggle() || field.is_theme_color())
    };

    match color {
        Some(color) => color_row(theme, name, value_text, color, selected),
        None => field_row(theme, name, 24, value_text, selected),
    }
}

fn disabled_row(theme: &Theme, label: &str, value: String, selected: bool) -> Line<'static> {
    Line::from(Span::styled(
        format!("{}{label:<24} {value}", marker(selected)),
        theme.dim_style(),
    ))
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
        SettingsField::BatchAnchor => (
            "Batch anchor",
            device.batch_anchor.label().to_string(),
            None,
        ),
        SettingsField::ReadFullCustoms => (
            "Read full custom values",
            on_off(device.read_full_customs),
            None,
        ),
        SettingsField::CustomBatchBySize => (
            "Custom batch by size",
            on_off(device.custom_batch_by_size),
            None,
        ),
        SettingsField::AutoUpdate => (
            "Auto-update (ms)",
            device
                .update_interval_ms
                .map_or_else(|| "off".to_string(), |n| n.to_string()),
            None,
        ),
        SettingsField::ReconnectOnTimeout => (
            "Reconnect on timeout",
            on_off(device.reconnect_on_timeout),
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
        SettingsField::SavePositionOnExit => (
            "Save position on exit",
            on_off(device.save_position_on_exit),
            None,
        ),
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
        SettingsField::CyclePinned => ("Cycle pinned", on_off(device.cycle_panels.pinned), None),
        SettingsField::CycleLabeled => ("Cycle labeled", on_off(device.cycle_panels.labeled), None),
        SettingsField::CycleCustom => ("Cycle custom", on_off(device.cycle_panels.custom), None),
        SettingsField::CycleMatrix => ("Cycle matrix", on_off(device.cycle_panels.matrix), None),
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
        SettingsField::CopyData => ("Copy all", String::new(), None),
        SettingsField::CopyConfig => ("Copy configuration", String::new(), None),
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

    let (list, footer) = footer_split(area, 1);
    let visible = list.height.saturating_sub(3).max(1);
    let top = scroll_offset(
        params.kb_selected as usize,
        count as usize,
        visible as usize,
    ) as u16;
    let end = (top + visible).min(count);
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

    frame.render_widget(Paragraph::new(lines), list);
    if footer.is_some() && (top > 0 || end < count) {
        let row = Rect {
            y: list.y + list.height,
            height: 1,
            ..list
        };
        let more = hints::more(theme, top as usize, (count - end) as usize);
        frame.render_widget(Paragraph::new(more), row);
    }
    render_footer(frame, footer, vec![hint]);
}

#[cfg(test)]
mod tests {
    use super::scroll_offset;

    #[test]
    fn selection_is_centered_once_the_list_overflows() {
        assert_eq!(scroll_offset(3, 30, 10), 0);
        assert_eq!(scroll_offset(5, 30, 10), 0);
        assert_eq!(scroll_offset(12, 30, 10), 7);
        assert_eq!(scroll_offset(24, 30, 10), 19);
    }

    #[test]
    fn offset_never_scrolls_past_the_end_or_below_zero() {
        assert_eq!(scroll_offset(29, 30, 10), 20);
        assert_eq!(scroll_offset(0, 30, 10), 0);
        assert_eq!(scroll_offset(2, 5, 10), 0);
        assert_eq!(scroll_offset(7, 30, 0), 7);
    }

    #[test]
    fn selection_stays_inside_the_window() {
        for height in 1..12usize {
            for selected in 0..40usize {
                let top = scroll_offset(selected, 40, height);
                assert!(
                    top <= selected && selected < top + height,
                    "{selected} {height}"
                );
            }
        }
    }
}
