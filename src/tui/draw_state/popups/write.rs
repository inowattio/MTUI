use crate::app::WriteType;
use crate::config::Keybinds;
use crate::state::WriteParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    write: &WriteParams,
) {
    let value = write
        .value
        .map_or_else(|| "(none)".to_string(), |n| n.to_string());

    let bits: u16 = match write.write_type {
        WriteType::Word => 16,
        WriteType::DWord => 32,
    };
    let raw = write.value.unwrap_or(0) as u32;

    let mut bit_spans = vec![Span::styled("Bits:  ", theme.dim_style())];
    for i in (0..bits).rev() {
        let set = (raw >> i) & 1 == 1;
        let style = if i == write.bit_cursor {
            theme.selected_style()
        } else if set {
            theme.accent_style()
        } else {
            theme.dim_style()
        };
        bit_spans.push(Span::styled(if set { "1" } else { "0" }, style));
        if i % 4 == 0 && i != 0 {
            bit_spans.push(Span::raw(" "));
        }
    }

    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("[{:?}] ", write.write_type), theme.dim_style()),
            Span::styled("to ", theme.dim_style()),
            Span::styled(write.position.to_string(), theme.accent_style()),
        ]),
        Line::from(vec![
            Span::styled("Value: ", theme.dim_style()),
            Span::styled(value, theme.base()),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(bit_spans),
        Line::from(Span::styled(
            format!("       bit {}", write.bit_cursor),
            theme.dim_style(),
        )),
    ];

    if let Some(result) = &write.result {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            result.text.clone(),
            theme.message_style(result.kind),
        )));
    }

    lines.push(Line::from(Span::styled(
        format!(
            " {} write \u{b7} {} exit \u{b7} {} word/dword \u{b7} {} negate",
            kb.action, kb.exit, kb.write, kb.negator
        ),
        theme.dim_style(),
    )));
    lines.push(Line::from(Span::styled(
        format!(" \u{2190}/\u{2192} bit \u{b7} {} toggle", kb.pause),
        theme.dim_style(),
    )));

    super::render(frame, area, theme, "Write", 58, lines);
}
