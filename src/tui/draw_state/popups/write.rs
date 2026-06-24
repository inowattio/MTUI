use crate::app::WriteType;
use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::WriteParams;
use crate::tui::hints::{self, Hint};
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
    if write.write_type == WriteType::Coil {
        draw_coil(frame, area, theme, kb, write);
        return;
    }

    let value = write
        .value
        .map_or_else(|| "(none)".to_string(), |n| n.to_string());

    let bits: u16 = match write.write_type {
        WriteType::Word => 16,
        WriteType::DWord => 32,
        WriteType::Coil => 1, // handled by draw_coil above
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

    let dword = write.write_type == WriteType::DWord;
    let func = if write.force_multiple || dword {
        "Multiple registers (0x10)"
    } else {
        "Single register (0x06)"
    };
    let func_style = if dword {
        theme.dim_style()
    } else {
        theme.base()
    };
    lines.push(Line::from(vec![
        Span::styled("Func:  ", theme.dim_style()),
        Span::styled(func, func_style),
    ]));

    if let Some(result) = &write.result {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            result.text.clone(),
            theme.message_style(result.kind),
        )));
    }

    let footer1 = [
        Hint::key(kb.action, "Write"),
        Hint::key(kb.exit, "Exit"),
        Hint::key(kb.write, "Cycle mode"),
    ];
    let footer2 = [
        Hint::key(KeyCode::Char('-'), "Negate"),
        Hint::pair(KeyCode::Left, KeyCode::Right, "Bit"),
        Hint::key(kb.pause, "Toggle"),
    ];
    let width = 50.max(hints::width(&footer1) as u16);
    lines.push(hints::footer(theme, footer1));
    lines.push(hints::footer(theme, footer2));

    super::render(frame, area, theme, "Write", width, lines);
}

fn draw_coil(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, write: &WriteParams) {
    let on = write.value.unwrap_or(0) != 0;
    let (state, state_style) = if on {
        ("ON", theme.ok_style())
    } else {
        ("OFF", theme.dim_style())
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("[Coil] ", theme.dim_style()),
            Span::styled("to ", theme.dim_style()),
            Span::styled(write.position.to_string(), theme.accent_style()),
        ]),
        Line::from(vec![
            Span::styled("State: ", theme.dim_style()),
            Span::styled(state, state_style),
        ]),
    ];

    if let Some(result) = &write.result {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            result.text.clone(),
            theme.message_style(result.kind),
        )));
    }

    let footer = [
        Hint::key(kb.action, "Write"),
        Hint::key(kb.pause, "Toggle on/off"),
        Hint::key(kb.exit, "Exit"),
    ];
    let width = 44.max(hints::width(&footer) as u16);
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Write coil", width, lines);
}
