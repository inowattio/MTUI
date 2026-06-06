use crate::constants::keybind::*;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, area: Rect, theme: &Theme, device: &str) {
    let entries = [
        (format!("{HELP}"), "Help"),
        (format!("{EXIT}"), "Exit / Back"),
        (format!("{MOVE_UP}/{MOVE_DOWN}"), "Move cursor"),
        (format!("{ACTION}"), "Action"),
        (format!("{REFRESH}"), "Refresh data"),
        (format!("{TOGGLE}"), "Switch register type"),
        (format!("{WRITE}"), "Write"),
        (format!("{JUMP}"), "Jump Address"),
        (format!("{SEARCH}"), "Jump/Search Label"),
        (format!("{SAVE}"), "Save config to file"),
        ("".into(), "Read only: "),
        (format!("{PIN}"), "Add / Remove pin"),
        (format!("{LABEL}"), "Label register"),
        (format!("{DUMP}"), "Dump read data to file"),
        (format!("{SWITCH_VIEW}"), "Switch Main / Pinned"),
        (format!("{COLUMNS}"), "Toggle columns"),
    ];

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Device: ", theme.dim_style()),
            Span::styled(device.to_string(), theme.base()),
        ]),
        Line::default(),
    ];
    for (key, desc) in entries {
        lines.push(Line::from(vec![
            Span::styled(format!("  {key:<9}"), theme.accent_style()),
            Span::styled(desc.to_string(), theme.base()),
        ]));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left),
        area,
    );
}
