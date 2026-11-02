use crate::input::KeyCode;
use crate::state::{ImportParams, ImportSource};
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, params: &ImportParams) {
    let Some(device) = params.device() else {
        return;
    };
    let found = match params.source {
        ImportSource::Mtui => " Found importable data:",
        ImportSource::Mbmap => " Found an .mbmap device map:",
        ImportSource::HomeAssistant => " Found a Home Assistant config:",
    };
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(found, theme.base())),
        Line::default(),
    ];

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
    let cancel = Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel");
    if picking {
        let device = Hint::pair(KeyCode::Up, KeyCode::Down, "Device");
        super::push_footer(&mut lines, theme, [device, import, cancel]);
    } else {
        super::push_footer(&mut lines, theme, [import, cancel]);
    }

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Paste import", width, lines);
}
