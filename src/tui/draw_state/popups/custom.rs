use crate::app::App;
use crate::input::KeyCode;
use crate::state::{CustomField, CustomParams};
use crate::tui::draw_state::{edit_value, field_row};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

fn section(field: CustomField) -> &'static str {
    match field {
        CustomField::Repr | CustomField::WordOrder | CustomField::Next => "DECODE",
        CustomField::Ops | CustomField::Enum | CustomField::Bits => "MAP",
        CustomField::Decimals | CustomField::Prefix | CustomField::Suffix => "FORMAT",
    }
}

fn joined<T>(items: &[T], display: impl Fn(&T) -> String) -> String {
    if items.is_empty() {
        "(none)".to_string()
    } else {
        items.iter().map(display).collect::<Vec<_>>().join("  ")
    }
}

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, c: &CustomParams) {
    let sel = c.current_field();

    let kb = &app.config.keybinds;
    let footer = [
        Hint::pair(kb.move_up, kb.move_down, "Field"),
        Hint::pair(KeyCode::Left, KeyCode::Right, "Change"),
        Hint::key(kb.action, "Save"),
        Hint::key(KeyCode::Delete, "Remove"),
        Hint::key(kb.exit, "Close"),
    ];
    let width = hints::min_width(56, &footer);
    let inner = width.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = vec![];

    match app.custom_preview(c) {
        Ok(preview) => {
            let mut segments = vec![(preview.words, false)];
            if let Some(base) = preview.base {
                segments.push((base, false));
            }
            segments.push((preview.output, true));

            let mut spans: Vec<Span> = Vec::new();
            let mut used = 0usize;
            for (i, (text, is_output)) in segments.into_iter().enumerate() {
                let style = if is_output {
                    theme.accent_style()
                } else {
                    theme.base()
                };
                let sep = if i == 0 { " " } else { " \u{2192} " };
                let needed = sep.chars().count() + text.chars().count();
                if i > 0 && used + needed > inner {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    let indent = "  \u{2192} ";
                    used = indent.chars().count() + text.chars().count();
                    spans.push(Span::styled(indent, theme.dim_style()));
                } else {
                    used += needed;
                    spans.push(Span::styled(sep, theme.dim_style()));
                }
                spans.push(Span::styled(text, style));
            }
            lines.push(Line::from(spans));
        }
        Err(reason) => {
            lines.push(Line::from(Span::styled(
                format!(" {reason}"),
                theme.dim_style(),
            )));
        }
    }

    let entry_hints = |lines: &mut Vec<Line>, buffer: &str, example: &str| {
        lines.push(Line::from(Span::styled(
            format!("    add: {buffer}_   {example}"),
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            "    (enter adds \u{b7} empty enter saves \u{b7} backspace removes)",
            theme.dim_style(),
        )));
    };

    let mut current_section = "";
    for field in c.fields() {
        if section(field) != current_section {
            current_section = section(field);
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                format!(" {current_section}"),
                theme.dim_style(),
            )));
        }
        let selected = field == sel;

        match field {
            CustomField::Repr => {
                let value = format!("{}  ({} reg)", c.repr.label(), c.repr.register_count());
                lines.push(field_row(
                    theme,
                    "Type",
                    12,
                    edit_value(value, selected, true),
                    selected,
                ));
            }
            CustomField::WordOrder => {
                let value = match c.word_order {
                    Some(order) => format!("{order:?}"),
                    None => format!("device ({:?})", app.config.device.word_order),
                };
                lines.push(field_row(
                    theme,
                    "Word order",
                    12,
                    edit_value(value, selected, true),
                    selected,
                ));
            }
            CustomField::Next => {
                let value = if c.next.is_empty() {
                    "(contiguous)".to_string()
                } else {
                    joined(&c.next, u16::to_string)
                };
                lines.push(field_row(theme, "Next words", 12, value, selected));
                if selected {
                    entry_hints(&mut lines, &c.next_buffer, "address of word 2 (then 3, 4)");
                }
            }
            CustomField::Ops => {
                let value = joined(&c.ops, |o| o.display());
                lines.push(field_row(theme, "Operations", 12, value, selected));
                if selected {
                    entry_hints(&mut lines, &c.op_buffer, "e.g. *0.1  +5  /10  ^2");
                }
            }
            CustomField::Enum => {
                let value = joined(&c.enum_map, |e| format!("{}\u{2192}{}", e.value, e.text));
                lines.push(field_row(theme, "Enum", 12, value, selected));
                if selected {
                    entry_hints(&mut lines, &c.enum_buffer, "e.g. 3=Running");
                }
            }
            CustomField::Bits => {
                let value = joined(&c.bits, |e| format!("{}\u{2192}{}", e.bit, e.name));
                lines.push(field_row(theme, "Bits", 12, value, selected));
                if selected {
                    entry_hints(&mut lines, &c.bit_buffer, "e.g. 0=run");
                }
            }
            CustomField::Decimals => {
                let value = if selected {
                    format!("{}_", c.decimals)
                } else if c.decimals.is_empty() {
                    "auto".to_string()
                } else {
                    c.decimals.clone()
                };
                lines.push(field_row(theme, "Decimals", 12, value, selected));
                if selected {
                    lines.push(Line::from(Span::styled(
                        "    auto; 0 for none; numerical for amount",
                        theme.dim_style(),
                    )));
                }
            }
            CustomField::Prefix => {
                let value = edit_value(c.prefix.clone(), selected, false);
                lines.push(field_row(theme, "Prefix", 12, value, selected));
            }
            CustomField::Suffix => {
                let value = edit_value(c.suffix.clone(), selected, false);
                lines.push(field_row(theme, "Suffix", 12, value, selected));
            }
        }
    }

    if let Some(err) = &c.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {err}"),
            theme.err_style(),
        )));
    }

    lines.push(Line::default());
    lines.push(hints::footer(theme, footer));

    let title = format!("Custom rule \u{b7} {:?} @ {}", c.register_type, c.address);
    super::render(frame, area, theme, &title, width, lines);
}
