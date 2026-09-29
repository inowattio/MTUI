use crate::app::App;
use crate::input::KeyCode;
use crate::state::{CustomField, CustomParams};
use crate::tui::draw_state::{edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

const LIST_ROWS: usize = 5;
const SIDE_WIDTH: usize = 24;
const SIDE_LEFT: usize = 40;
const PREVIEW_WIDTH: usize = 54;

fn more_label(above: usize, below: usize) -> String {
    let mut parts = Vec::new();
    if above > 0 {
        parts.push(format!("^ {above}"));
    }
    if below > 0 {
        parts.push(format!("v {below}"));
    }
    format!("  {} more", parts.join(" "))
}

fn count_label(n: usize) -> String {
    match n {
        0 => "(none)".to_string(),
        1 => "1 entry".to_string(),
        n => format!("{n} entries"),
    }
}

const fn section(field: CustomField) -> &'static str {
    match field {
        CustomField::Repr | CustomField::WordOrder | CustomField::Next => "DECODE",
        CustomField::Ops | CustomField::Enum | CustomField::Bits => "MAP",
        CustomField::Decimals | CustomField::Prefix | CustomField::Suffix => "FORMAT",
    }
}

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, c: &CustomParams) {
    let sel = c.current_field();

    let in_list = c.list_index.is_some();
    let on_list = matches!(sel, CustomField::Enum | CustomField::Bits);
    let footer = if in_list {
        hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Up, KeyCode::Down, "Entry"),
                Hint::key(KeyCode::Backspace, "Remove"),
                Hint::key(KeyCode::Tab, "Fields"),
            ],
        )
    } else if on_list {
        hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Up, KeyCode::Down, "Field"),
                Hint::pair(KeyCode::Left, KeyCode::Right, "Change"),
                Hint::key(KeyCode::Tab, "Entries"),
            ],
        )
    } else {
        hints::footer(
            theme,
            [
                Hint::pair(KeyCode::Up, KeyCode::Down, "Field"),
                Hint::pair(KeyCode::Left, KeyCode::Right, "Change"),
            ],
        )
    };
    let footer_more = hints::footer(
        theme,
        [
            Hint::key(KeyCode::Enter, "Save"),
            Hint::key(KeyCode::Delete, "Remove"),
            Hint::key(KeyCode::Esc, if in_list { "Back" } else { "Close" }),
        ],
    );
    let inner = PREVIEW_WIDTH;

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
                let sep = if i == 0 { " " } else { " -> " };
                let needed = sep.chars().count() + text.chars().count();
                if i > 0 && used + needed > inner {
                    lines.push(Line::from(std::mem::take(&mut spans)));
                    let indent = "  -> ";
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

    let list_rows =
        |lines: &mut Vec<Line>, label: &str, items: Vec<String>, empty: &str, selected: bool| {
            let hidden = items.len().saturating_sub(LIST_ROWS);
            let mut rows: Vec<(String, bool)> = Vec::new();
            if hidden > 0 {
                rows.push((format!("^ {hidden} more"), true));
            }
            rows.extend(items.into_iter().skip(hidden).map(|item| (item, false)));
            if rows.is_empty() {
                rows.push((empty.to_string(), false));
            }
            let (first, dim) = rows.remove(0);
            let mut head = field_row(theme, label, 12, first, selected);
            if dim {
                head.spans[1].style = theme.dim_style();
            }
            lines.push(head);
            for (row, _) in rows {
                lines.push(Line::from(vec![
                    Span::raw(" ".repeat(15)),
                    Span::styled(row, theme.line_style(selected)),
                ]));
            }
        };

    let entry_hints = |lines: &mut Vec<Line>, buffer: &str, example: &str| {
        lines.push(Line::from(Span::styled(
            format!("    add: {buffer}_   {example}"),
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            "    (enter adds | empty enter saves)",
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            "    (backspace removes)",
            theme.dim_style(),
        )));
    };

    let enum_items: Vec<String> = c
        .enum_map
        .iter()
        .map(|e| format!("{}->{}", e.value, e.text))
        .collect();
    let bit_items: Vec<String> = c
        .bits
        .iter()
        .map(|e| format!("{}->{}", e.bit, e.name))
        .collect();
    let side = match sel {
        CustomField::Enum => Some(("Enum", enum_items)),
        CustomField::Bits => Some(("Bits", bit_items)),
        _ => None,
    };

    let body_start = lines.len();

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
                let items = c.next.iter().map(u16::to_string).collect();
                list_rows(&mut lines, "Next words", items, "(contiguous)", selected);
                if selected {
                    entry_hints(&mut lines, &c.next_buffer, "address of word 2 (then 3, 4)");
                }
            }
            CustomField::Ops => {
                let items = c.ops.iter().map(|o| o.display()).collect();
                list_rows(&mut lines, "Operations", items, "(none)", selected);
                if selected {
                    entry_hints(&mut lines, &c.op_buffer, "e.g. *0.1  +5  /10  ^2");
                }
            }
            CustomField::Enum => {
                let value = count_label(c.enum_map.len());
                lines.push(field_row(theme, "Enum", 12, value, selected));
                if selected {
                    entry_hints(&mut lines, &c.enum_buffer, "e.g. 3=Running");
                }
            }
            CustomField::Bits => {
                let value = count_label(c.bits.len());
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
                        "    auto; 0 for none",
                        theme.dim_style(),
                    )));
                    lines.push(Line::from(Span::styled(
                        "    numerical for amount",
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

    if let Some((label, items)) = side {
        let rows = lines.len().saturating_sub(body_start + 1);
        let capacity = rows.saturating_sub(1);
        let overflow = items.len() > capacity;
        let visible = if overflow {
            capacity.saturating_sub(1)
        } else {
            capacity
        };
        let anchor = c
            .list_index
            .map_or_else(|| items.len().saturating_sub(1), |i| i as usize);
        let top = anchor
            .saturating_sub(visible / 2)
            .min(items.len().saturating_sub(visible));
        let below = items.len().saturating_sub(top + visible);
        let col_w = items
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .clamp(8, SIDE_WIDTH);
        let mut col: Vec<(String, Style)> = vec![(label.to_string(), theme.dim_style())];
        if items.is_empty() {
            col.push(("  (none)".to_string(), theme.dim_style()));
        }
        if overflow {
            col.push((more_label(top, below), theme.dim_style()));
        }
        col.extend(
            items
                .into_iter()
                .enumerate()
                .skip(top)
                .take(visible)
                .map(|(i, item)| {
                    let picked = c.list_index == Some(i as u16);
                    (
                        format!("{}{item}", marker(picked)),
                        theme.line_style(picked),
                    )
                }),
        );
        let left_w = lines
            .iter()
            .skip(body_start + 1)
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(SIDE_LEFT);
        for (i, line) in lines.iter_mut().skip(body_start + 1).enumerate() {
            let pad = left_w.saturating_sub(line.width());
            line.spans.push(Span::raw(" ".repeat(pad)));
            line.spans
                .push(Span::styled(" \u{2502} ", theme.dim_style()));
            if let Some((text, style)) = col.get(i) {
                line.spans
                    .push(Span::styled(super::truncate(text, col_w + 2), *style));
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
    lines.push(footer);
    lines.push(footer_more);

    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 2;
    let title = format!("Custom rule | {:?} @ {}", c.register_type, c.address);
    super::render(frame, area, theme, &title, width, lines);
}
