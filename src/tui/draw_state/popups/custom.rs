use crate::app::App;
use crate::input::KeyCode;
use crate::state::{CustomField, CustomParams};
use crate::tui::draw_state::marker;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, c: &CustomParams) {
    let sel = c.current_field();

    let field_line = |label: &str, value: String, selected: bool| -> Line<'static> {
        let marker = marker(selected);
        let style = theme.line_style(selected);
        Line::from(vec![
            Span::styled(format!("{marker}{label:<12} "), theme.dim_style()),
            Span::styled(value, style),
        ])
    };

    let mut lines: Vec<Line> = vec![];

    lines.push(match app.custom_preview(c) {
        Ok((input, output)) => Line::from(vec![
            Span::styled(" Preview  ", theme.dim_style()),
            Span::styled(input.to_string(), theme.base()),
            Span::styled(" \u{2192} ", theme.dim_style()),
            Span::styled(output, theme.accent_style()),
        ]),
        Err(reason) => Line::from(vec![
            Span::styled(" Preview  ", theme.dim_style()),
            Span::styled(reason, theme.dim_style()),
        ]),
    });
    lines.push(Line::default());

    let repr_val = format!("{}  ({} reg)", c.repr.label(), c.repr.register_count());
    let repr_val = if sel == CustomField::Repr {
        format!("\u{2039} {repr_val} \u{203a}")
    } else {
        repr_val
    };
    lines.push(field_line("Type", repr_val, sel == CustomField::Repr));

    let ops_str = if c.ops.is_empty() {
        "(none)".to_string()
    } else {
        c.ops
            .iter()
            .map(|o| o.display())
            .collect::<Vec<_>>()
            .join(" ")
    };
    lines.push(field_line("Operations", ops_str, sel == CustomField::Ops));
    if sel == CustomField::Ops {
        lines.push(Line::from(Span::styled(
            "    (enter adds, backspace removes)",
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            format!("    add: {}_   e.g. *0.1  +5  /10  ^2", c.op_buffer),
            theme.dim_style(),
        )));
    }

    let enum_str = if c.enum_map.is_empty() {
        "(none)".to_string()
    } else {
        c.enum_map
            .iter()
            .map(|e| format!("{}\u{2192}{}", e.value, e.text))
            .collect::<Vec<_>>()
            .join("  ")
    };
    lines.push(field_line("Enum", enum_str, sel == CustomField::Enum));
    if sel == CustomField::Enum {
        lines.push(Line::from(Span::styled(
            "    (enter adds, backspace removes)",
            theme.dim_style(),
        )));
        lines.push(Line::from(Span::styled(
            format!("    add: {}_   e.g. 3=Running", c.enum_buffer),
            theme.dim_style(),
        )));
    }

    let dec = if c.decimals.is_empty() {
        "auto".to_string()
    } else {
        c.decimals.clone()
    };
    lines.push(field_line("Decimals", dec, sel == CustomField::Decimals));
    if sel == CustomField::Decimals {
        lines.push(Line::from(Span::styled(
            "    auto; 0 for none; numerical for amount",
            theme.dim_style(),
        )));
    }

    let pfx = if sel == CustomField::Prefix {
        format!("{} ", c.prefix)
    } else {
        c.prefix.to_string()
    };
    lines.push(field_line("Prefix", pfx, sel == CustomField::Prefix));

    let sfx = if sel == CustomField::Suffix {
        format!("{} ", c.suffix)
    } else {
        c.suffix.to_string()
    };
    lines.push(field_line("Suffix", sfx, sel == CustomField::Suffix));

    lines.push(Line::default());
    let save_hint = if sel == CustomField::Save {
        "\u{2190} enter".to_string()
    } else {
        String::new()
    };
    lines.push(field_line("Save rule", save_hint, sel == CustomField::Save));
    let remove_hint = if sel == CustomField::Remove {
        "\u{2190} enter".to_string()
    } else {
        String::new()
    };
    lines.push(field_line(
        "Remove rule",
        remove_hint,
        sel == CustomField::Remove,
    ));

    if let Some(err) = &c.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {err}"),
            theme.err_style(),
        )));
    }

    lines.push(Line::default());
    let kb = &app.config.keybinds;
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(kb.move_up, kb.move_down, "Field"),
            Hint::pair(KeyCode::Left, KeyCode::Right, "Change"),
            Hint::key(kb.exit, "Close"),
        ],
    ));

    super::render(frame, area, theme, "Custom rule", 48, lines);
}
