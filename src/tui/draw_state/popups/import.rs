use crate::import::mbmap::{Revision, Status};
use crate::input::KeyCode;
use crate::state::{
    ConfigImport, ConfigImportAction, FirmwareInput, ImportDevice, ImportParams, ImportSource,
};
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, params: &ImportParams) {
    let Some(device) = params.device() else {
        return;
    };
    if let Some(config) = &params.config {
        draw_config(frame, area, theme, device, config);
        return;
    }
    let found = match params.source {
        ImportSource::Mtui => " Found importable data:",
        ImportSource::Mbmap => " Found an .mbmap device map:",
        ImportSource::HomeAssistant => " Found a Home Assistant config:",
        ImportSource::SunSpec => " Found a SunSpec model:",
        ImportSource::Solarman => " Found a Solarman inverter definition:",
    };
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(found, theme.base())),
    ];
    if params.revision.is_some() || params.status.is_some() {
        lines.push(map_line(theme, params.revision.as_ref(), params.status));
    }
    lines.push(Line::default());

    if let Some(firmware) = &params.firmware {
        push_firmware(&mut lines, theme, firmware);
    }

    let picking = params.devices.len() > 1;
    if picking {
        let width = params
            .devices
            .iter()
            .map(|d| d.name.chars().count())
            .max()
            .unwrap_or(0);
        for (i, d) in params.devices.iter().enumerate() {
            let selected = i == usize::from(params.selected);
            let marker = if selected { '>' } else { ' ' };
            let plural = if d.labels == 1 { "" } else { "s" };
            lines.push(Line::from(Span::styled(
                format!(
                    "   {marker} {:<width$}   {} label{plural}",
                    d.name, d.labels
                ),
                theme.line_style(selected),
            )));
        }
        lines.push(Line::default());
    }

    for (count, noun) in [
        (device.pins, "pinned register"),
        (device.labels, "label"),
        (device.rules, "custom rule"),
    ] {
        if count > 0 {
            let plural = if count == 1 { "" } else { "s" };
            lines.push(Line::from(vec![
                Span::styled(format!("   {count} "), theme.accent_style()),
                Span::styled(format!("{noun}{plural}"), theme.base()),
            ]));
        }
    }

    if device.replaced > 0 {
        let (count, verb) = match device.replaced {
            1 => ("1 entry".to_string(), "was"),
            n => (format!("{n} entries"), "were"),
        };
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {count} {verb} replaced by a later one at the same address."),
            theme.warn_style(),
        )));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        " Entries at the same address are overwritten.",
        theme.dim_style(),
    )));
    let import = Hint::key(KeyCode::Enter, "Import");
    let cancel = if params.firmware.is_some() {
        Hint::key(KeyCode::Esc, "Cancel")
    } else {
        Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel")
    };
    if picking {
        let device = Hint::pair(KeyCode::Up, KeyCode::Down, "Device");
        super::push_footer(&mut lines, theme, [device, import, cancel]);
    } else {
        super::push_footer(&mut lines, theme, [import, cancel]);
    }

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Import", width, lines);
}

fn draw_config(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    device: &ImportDevice,
    config: &ConfigImport,
) {
    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            Span::styled(" Found a full config ", theme.base()),
            Span::styled(format!("\"{}\"", config.name), theme.accent_style()),
            Span::styled(":", theme.base()),
        ]),
        Line::default(),
    ];

    for action in ConfigImportAction::ALL {
        let selected = action == config.action;
        let marker = if selected { '>' } else { ' ' };
        let mut title = vec![Span::styled(
            format!("   {marker} {}", action.label()),
            theme.line_style(selected),
        )];
        if action == ConfigImportAction::Save {
            title.push(Span::styled(" ", theme.base()));
            title.push(Span::styled(config.file_name.clone(), theme.base()));
            if selected {
                title.push(super::cursor_span(theme));
            }
            title.push(Span::styled(".json", theme.dim_style()));
        }
        lines.push(Line::from(title));
        let (detail, style) = match action {
            ConfigImportAction::Registers => (registers_detail(device), theme.dim_style()),
            ConfigImportAction::Load if config.unsaved => (
                "Replaces the current config, discarding unsaved changes".to_string(),
                theme.warn_style(),
            ),
            ConfigImportAction::Load => (
                "Replaces the current config until you save".to_string(),
                theme.dim_style(),
            ),
            ConfigImportAction::Save => (format!("in {}", config.folder), theme.dim_style()),
        };
        lines.push(Line::from(Span::styled(format!("       {detail}"), style)));
    }

    if let Some(error) = &config.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {error}"),
            theme.err_style(),
        )));
    }

    let action = Hint::pair(KeyCode::Up, KeyCode::Down, "Action");
    let confirm = Hint::key(KeyCode::Enter, config.action.confirm());
    let cancel = if config.action == ConfigImportAction::Save {
        Hint::key(KeyCode::Esc, "Cancel")
    } else {
        Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel")
    };
    super::push_footer(&mut lines, theme, [action, confirm, cancel]);

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Import", width, lines);
}

fn registers_detail(device: &ImportDevice) -> String {
    let counts: Vec<String> = [
        (device.pins, "pin"),
        (device.labels, "label"),
        (device.rules, "custom rule"),
    ]
    .into_iter()
    .filter(|&(count, _)| count > 0)
    .map(|(count, noun)| {
        let plural = if count == 1 { "" } else { "s" };
        format!("{count} {noun}{plural}")
    })
    .collect();
    if counts.is_empty() {
        "No pins, labels or custom rules".to_string()
    } else {
        counts.join(", ")
    }
}

const RANGES_SHOWN: usize = 8;

fn push_firmware(lines: &mut Vec<Line<'static>>, theme: &Theme, firmware: &FirmwareInput) {
    let FirmwareInput { text, known } = firmware;
    let (shown, style) = if text.is_empty() {
        ("any".to_string(), theme.dim_style())
    } else {
        (text.clone(), theme.base())
    };
    lines.push(Line::from(vec![
        Span::styled("   Firmware: ", theme.dim_style()),
        Span::styled(shown, style),
        super::cursor_span(theme),
    ]));
    if let Some(map) = &known.map {
        let (note, style) = if map.applies(text) {
            (
                format!("   This map is for firmware {map}"),
                theme.dim_style(),
            )
        } else {
            (
                format!("   {text} is outside this map's firmware ({map})"),
                theme.warn_style(),
            )
        };
        lines.push(Line::from(Span::styled(note, style)));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "   Registers by firmware (dimmed are skipped):",
        theme.dim_style(),
    )));
    let width = known
        .ranges
        .iter()
        .take(RANGES_SHOWN)
        .map(|(range, _)| range.to_string().chars().count())
        .max()
        .unwrap_or(0);
    for (range, count) in known.ranges.iter().take(RANGES_SHOWN) {
        let plural = if *count == 1 { "" } else { "s" };
        let style = if range.applies(text) {
            theme.base()
        } else {
            theme.dim_style()
        };
        lines.push(Line::from(Span::styled(
            format!(
                "     {:<width$}   {count} register{plural}",
                range.to_string()
            ),
            style,
        )));
    }
    let hidden = known.ranges.len().saturating_sub(RANGES_SHOWN);
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("     and {hidden} more"),
            theme.dim_style(),
        )));
    }
    lines.push(Line::default());
}

fn map_line(theme: &Theme, revision: Option<&Revision>, status: Option<Status>) -> Line<'static> {
    let mut spans = vec![Span::styled("   Map ", theme.dim_style())];
    if let Some(revision) = revision {
        spans.push(Span::styled(revision.to_string(), theme.dim_style()));
    }
    if let Some(status) = status {
        if revision.is_some() {
            spans.push(Span::styled(" · ", theme.dim_style()));
        }
        let style = match status {
            Status::Verified => theme.ok_style(),
            Status::Official => theme.dim_style(),
            Status::Unofficial => theme.warn_style(),
        };
        spans.push(Span::styled(status.label(), style));
    }
    Line::from(spans)
}
