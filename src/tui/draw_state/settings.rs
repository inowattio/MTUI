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

fn color_view(color: Color) -> (String, Option<Color>) {
    (color.to_string(), Some(color))
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
        if category.is_search() {
            lines.push(Line::default());
        }
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
    let search = params.current_category().is_search();
    let mut lines: Vec<Line> = vec![Line::default()];
    if search {
        lines.push(query_line(params, theme));
        lines.push(Line::from(Span::styled(
            "  keybinds and theme are not included",
            theme.dim_style(),
        )));
    }

    let groups = params.current_groups();
    if search && groups.is_empty() {
        let text = if params.query.trim().is_empty() {
            "  type to search"
        } else {
            "  no matches"
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(text, theme.dim_style())));
    }

    let mut selected_line = None;
    let mut field_lines = Vec::new();
    let mut index = 0u16;
    for (g, (category, group)) in groups.iter().enumerate() {
        if g > 0 || search {
            lines.push(Line::default());
        }
        if let Some(category) = category {
            lines.push(Line::from(Span::styled(
                format!("  {}", category.label()),
                theme.accent_style(),
            )));
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
                    format!("  {:<24} {}", "", app.writes_log_path().display()),
                    theme.dim_style(),
                )));
            }
            index += 1;
        }
    }

    let mut footer_lines = Vec::new();
    if let Some(field) = params.current_field().filter(|_| focused) {
        footer_lines.push(Line::from(Span::styled(
            format!("  {}", field.description()),
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

fn query_line(params: &SettingsParams, theme: &Theme) -> Line<'static> {
    let typing = params.focus == SettingsFocus::Categories;
    let mut spans = vec![
        Span::styled("  Search: ", theme.dim_style()),
        Span::styled(params.query.clone(), theme.base()),
    ];
    if typing {
        spans.push(Span::styled("_", theme.accent_style()));
    }
    Line::from(spans)
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
                Line::from(Span::styled("* unsaved changes", theme.warn_style())).right_aligned(),
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
    let name = field.label();
    let (value, color) = field_value(app, params, field);

    if app.settings_field_disabled(field) {
        return disabled_row(theme, name, value, selected);
    }

    let value_text = if selected && field.is_action() && value.is_empty() {
        "<- enter".to_string()
    } else if selected && field.is_action() {
        format!("{value}  <- enter")
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
    line.spans
        .push(Span::styled("###", Style::default().fg(color)));
    line
}

fn auto_width(width: u16) -> String {
    match width {
        0 => "auto".to_string(),
        n => n.to_string(),
    }
}

fn field_value(
    app: &App,
    params: &SettingsParams,
    field: SettingsField,
) -> (String, Option<Color>) {
    let device = &app.config;
    match field {
        SettingsField::Name => (device.name.clone(), None),
        SettingsField::BatchSize => (device.batch.size.to_string(), None),
        SettingsField::BatchAnchor => (device.batch.anchor.label().to_string(), None),
        SettingsField::TimeMode => (app.interpreter.time_mode().label().to_string(), None),
        SettingsField::AddressMode => (app.interpreter.address_mode().label().to_string(), None),
        SettingsField::LabelWidth => (auto_width(app.interpreter.label_width()), None),
        SettingsField::CustomWidth => (auto_width(app.interpreter.custom_width()), None),
        SettingsField::ReadFullCustoms => (on_off(device.batch.read_full_customs), None),
        SettingsField::CustomBatchByRegisters => (on_off(device.batch.custom_by_registers), None),
        SettingsField::FilterPanelsByType => (on_off(device.filter_panels_by_type), None),
        SettingsField::RefreshInterval => (
            match device.refresh_interval_ms {
                0 => "off".to_string(),
                n => n.to_string(),
            },
            None,
        ),
        SettingsField::ReconnectOnTimeout => (on_off(device.reconnect_on_timeout), None),
        SettingsField::GraphHistory => (device.graph.history.to_string(), None),
        SettingsField::MatrixColumns => (auto_width(device.matrix.columns), None),
        SettingsField::SkipUnsavedWarning => (on_off(device.skip_unsaved_warning), None),
        SettingsField::ShowMockDevice => (on_off(device.show_mock_device), None),
        SettingsField::ReadOnly => (on_off(device.read_only), None),
        SettingsField::ApiEnabled => (on_off(device.api.enabled), None),
        SettingsField::ApiPort => (
            match (device.api.enabled, device.api.port) {
                (false, 0) => "any".to_string(),
                (false, n) => n.to_string(),
                (true, 0) if app.api_bind_state() == ApiBindState::Failed => {
                    "any (bind failed)".to_string()
                }
                (true, n) if app.api_bind_state() == ApiBindState::Failed => {
                    format!("{n} (bind failed)")
                }
                (true, 0) => match app.api_bound_port() {
                    Some(bound) => format!("any (:{bound})"),
                    None => "any".to_string(),
                },
                (true, n) => n.to_string(),
            },
            None,
        ),
        SettingsField::ApiUnitIdOverride => (on_off(device.api.unit_id_override), None),
        SettingsField::LogWrites => (on_off(device.log_writes), None),
        SettingsField::StartupPanel => (device.startup.panel.name().to_string(), None),
        SettingsField::StartupType => (device.startup.register_type.name().to_string(), None),
        SettingsField::StartupAddress => (device.startup.address.to_string(), None),
        SettingsField::SavePositionOnExit => (on_off(device.save_position_on_exit), None),
        SettingsField::CycleHoldings => (on_off(device.cycle_register_types.holdings), None),
        SettingsField::CycleInputs => (on_off(device.cycle_register_types.inputs), None),
        SettingsField::CycleCoils => (on_off(device.cycle_register_types.coils), None),
        SettingsField::CycleDiscretes => (on_off(device.cycle_register_types.discretes), None),
        SettingsField::CyclePinned => (on_off(device.cycle_panels.pinned), None),
        SettingsField::CycleLabeled => (on_off(device.cycle_panels.labeled), None),
        SettingsField::CycleCustom => (on_off(device.cycle_panels.custom), None),
        SettingsField::CycleMatrix => (on_off(device.cycle_panels.matrix), None),
        SettingsField::ClearPins => (format!("{} pinned", app.pinned_registers.len()), None),
        SettingsField::ClearLabels => (format!("{} labels", app.label_count()), None),
        SettingsField::ClearCustom => (format!("{} rules", app.custom_count()), None),
        SettingsField::CopyData => (String::new(), None),
        SettingsField::CopyConfig => (String::new(), None),
        SettingsField::ShowRuleContinuation => (on_off(device.show_rule_continuation), None),
        SettingsField::ShowClock => (on_off(device.show_clock), None),
        SettingsField::ShowFrameTime => (on_off(device.show_frame_time), None),
        SettingsField::ShowRam => (on_off(device.show_ram), None),
        SettingsField::ShowConnectionLabel => (on_off(device.show_connection_label), None),
        SettingsField::ShowAsciiStrip => (on_off(device.show_ascii_strip), None),
        SettingsField::ShowInactiveTabs => (on_off(device.show_inactive_tabs), None),
        SettingsField::ShowReadWindow => (on_off(device.show_read_window), None),
        SettingsField::ShowMatrixContext => (on_off(device.matrix.show_context), None),
        SettingsField::GraphTimeAxis => (
            if device.graph.time_axis {
                "time".to_string()
            } else {
                "samples".to_string()
            },
            None,
        ),
        SettingsField::PaddingHorizontal => (device.padding.horizontal.to_string(), None),
        SettingsField::PaddingVertical => (device.padding.vertical.to_string(), None),
        SettingsField::ChangedExpiry => (
            match device.changed_expiry_ms {
                0 => "never".to_string(),
                n => n.to_string(),
            },
            None,
        ),
        SettingsField::ThemePreset => (
            Theme::PRESETS
                .iter()
                .find(|&&(_, t)| t == device.theme)
                .map_or_else(|| "custom".to_string(), |&(name, _)| name.to_string()),
            None,
        ),
        SettingsField::ThemeBorder => color_view(device.theme.border),
        SettingsField::ThemeAccent => color_view(device.theme.accent),
        SettingsField::ThemeText => color_view(device.theme.text),
        SettingsField::ThemeBackground => color_view(device.theme.background),
        SettingsField::ThemeDim => color_view(device.theme.dim),
        SettingsField::ThemeChanged => color_view(device.theme.changed),
        SettingsField::ThemeZebra => color_view(device.theme.zebra),
        SettingsField::ThemeOk => color_view(device.theme.ok),
        SettingsField::ThemeWarning => color_view(device.theme.warning),
        SettingsField::ThemeError => color_view(device.theme.error),
        SettingsField::ThemeSelectedText => color_view(device.theme.selected_text),
        SettingsField::ThemeSelectedBackground => color_view(device.theme.selected_background),
        SettingsField::Save => (app.config_path().display().to_string(), None),
        SettingsField::LoadConfig => (params.load_path.clone(), None),
        SettingsField::NextConfig => (device.next_config.clone(), None),
    }
}

fn draw_keybinds(params: &SettingsParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let kb = &app.config.keybinds;
    let actions = KeybindAction::ALL;
    let count = actions.len() as u16;

    let mut lines: Vec<Line> = vec![Line::default()];

    let (list, footer) = footer_split(area, 1);
    let visible = list.height.saturating_sub(1).max(1);
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

        let value = if capturing {
            "press a key...".to_string()
        } else {
            key.to_string()
        };
        let mut line = field_row(theme, action.label(), 22, value, selected);

        let duplicate = actions.iter().filter(|&&a| kb.get(a) == key).count() > 1;
        if duplicate && !capturing {
            line.spans
                .push(Span::styled(" | duplicate", theme.warn_style()));
        }

        lines.push(line);
    }

    let hint = if params.kb_capturing {
        hints::footer(theme, [Hint::key(KeyCode::Esc, "Cancel")])
    } else {
        hints::footer(
            theme,
            [
                Hint::key(KeyCode::Enter, "Rebind"),
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
