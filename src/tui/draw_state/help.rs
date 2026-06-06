use crate::constants::keybind::*;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(frame: &mut Frame, area: Rect, theme: &Theme, device: &str) {
    let entries = [
        (format!("{EXIT}"), "Exit / Back"),
        (format!("{MOVE_UP}/{MOVE_DOWN}"), "Move cursor"),
        (format!("{REFRESH}"), "Refresh data"),
        (format!("{TOGGLE}"), "Switch register type"),
        (format!("{SWITCH_VIEW}"), "Switch Main / Pinned (Read only)"),
        (format!("{WRITE}"), "Write"),
        (format!("{JUMP}"), "Jump"),
        (format!("{DUMP}"), "Dump read data to file (Read only)"),
        (format!("{HELP}"), "Help"),
        (format!("{PIN}"), "Add / Remove pin (Read only)"),
        (format!("{LABEL}"), "Label register (Read only)"),
        (format!("{SEARCH}"), "Search labels / jump"),
        (format!("{SAVE}"), "Save config to file"),
        (format!("{ACTION}"), "Action"),
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
