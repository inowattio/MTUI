use crate::app::WriteType;
use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::WriteParams;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

const LABEL_WIDTH: usize = 8;

fn label(text: &'static str, theme: &Theme) -> Span<'static> {
    Span::styled(
        format!(" {text:<width$}", width = LABEL_WIDTH - 1),
        theme.dim_style(),
    )
}

fn continuation() -> Span<'static> {
    Span::raw(" ".repeat(LABEL_WIDTH))
}

fn push_result(lines: &mut Vec<Line<'static>>, theme: &Theme, write: &WriteParams) {
    if let Some(result) = &write.result {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(result.text.clone(), theme.message_style(result.kind)),
        ]));
    }
}

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

    let bits: u16 = match write.write_type {
        WriteType::Word => 16,
        WriteType::DWord => 32,
        WriteType::Coil => 1, // handled by draw_coil above
    };
    let raw = write.value.unwrap_or(0) as u32;

    let mut bit_spans = vec![label("Bits", theme)];
    let mut caret_col = 0usize;
    let mut col = 0usize;
    for i in (0..bits).rev() {
        let set = (raw >> i) & 1 == 1;
        let style = if i == write.bit_cursor {
            theme.selected_style()
        } else if set {
            theme.accent_style()
        } else {
            theme.dim_style()
        };
        if i == write.bit_cursor {
            caret_col = col;
        }
        bit_spans.push(Span::styled(if set { "1" } else { "0" }, style));
        col += 1;
        if i % 4 == 0 && i != 0 {
            bit_spans.push(Span::raw(" "));
            col += 1;
        }
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
    let width = hints::min_width(50, &footer1);

    // Caret marking the bit cursor; its label flips to the left side when
    // the caret is too close to the popup's right edge.
    let caret = LABEL_WIDTH + caret_col;
    let bit_label = format!("bit {}", write.bit_cursor);
    let caret_line = if caret + 2 + bit_label.len() <= width as usize - 2 {
        Line::from(vec![
            Span::raw(" ".repeat(caret)),
            Span::styled("\u{25b4}", theme.accent_style()),
            Span::styled(format!(" {bit_label}"), theme.dim_style()),
        ])
    } else {
        Line::from(vec![
            Span::raw(" ".repeat(caret - bit_label.len() - 1)),
            Span::styled(bit_label, theme.dim_style()),
            Span::raw(" "),
            Span::styled("\u{25b4}", theme.accent_style()),
        ])
    };

    let value_line = Line::from(vec![
        label("Value", theme),
        Span::styled(
            write.value.map_or_else(String::new, |n| n.to_string()),
            theme.base(),
        ),
        Span::styled("_", theme.accent_style()),
    ]);
    let detail = match write.value {
        None => Span::styled("(empty)", theme.dim_style()),
        Some(n) => {
            let (hex, alt) = match write.write_type {
                WriteType::Word => {
                    let r = n as u16;
                    (
                        format!("0x{r:04X}"),
                        alt_view(n, i64::from(r), i64::from(r as i16), 16),
                    )
                }
                _ => {
                    let r = n as u32;
                    (
                        format!("0x{r:08X}"),
                        alt_view(n, i64::from(r), i64::from(r as i32), 32),
                    )
                }
            };
            let mut text = hex;
            if let Some(alt) = alt {
                text.push_str(&format!(" \u{b7} {alt}"));
            }
            Span::styled(text, theme.dim_style())
        }
    };

    let mut lines = vec![
        Line::default(),
        value_line,
        Line::from(vec![continuation(), detail]),
        Line::from(bit_spans),
        caret_line,
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
        label("Func", theme),
        Span::styled(func, func_style),
    ]));

    push_result(&mut lines, theme, write);

    lines.push(Line::default());
    lines.push(hints::footer(theme, footer1));
    lines.push(hints::footer(theme, footer2));

    let title = format!("Write [{:?}] @ {}", write.write_type, write.position);
    super::render(frame, area, theme, &title, width, lines);
}

fn alt_view(typed: i64, unsigned: i64, signed: i64, bits: u16) -> Option<String> {
    if typed < 0 {
        Some(format!("u{bits} {unsigned}"))
    } else if signed < 0 {
        Some(format!("i{bits} {signed}"))
    } else {
        None
    }
}

fn draw_coil(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, write: &WriteParams) {
    let on = write.value.unwrap_or(0) != 0;
    let (state, state_style) = if on {
        ("\u{25c9} ON", theme.ok_style())
    } else {
        ("\u{25cb} OFF", theme.dim_style())
    };

    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            label("State", theme),
            Span::styled(state, state_style),
        ]),
    ];

    push_result(&mut lines, theme, write);

    let footer = [
        Hint::key(kb.action, "Write"),
        Hint::key(kb.pause, "Toggle on/off"),
        Hint::key(kb.exit, "Exit"),
    ];
    let width = hints::min_width(44, &footer);
    lines.push(Line::default());
    lines.push(hints::footer(theme, footer));

    let title = format!("Write coil @ {}", write.position);
    super::render(frame, area, theme, &title, width, lines);
}
