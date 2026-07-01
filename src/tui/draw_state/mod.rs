pub mod logs;
pub mod popups;
pub mod read;
pub mod settings;

pub(crate) fn marker(selected: bool) -> &'static str {
    if selected {
        "> "
    } else {
        "  "
    }
}

pub(crate) fn cyclable(value: &str) -> String {
    format!("\u{2039} {value} \u{203a}")
}

pub(crate) fn edit_value(value: String, selected: bool, cyclable_field: bool) -> String {
    match (selected, cyclable_field) {
        (true, true) => cyclable(&value),
        (true, false) => format!("{value}_"),
        (false, _) => value,
    }
}

pub(crate) fn field_row(
    theme: &crate::tui::theme::Theme,
    label: &str,
    width: usize,
    value: String,
    selected: bool,
) -> ratatui::text::Line<'static> {
    use ratatui::text::{Line, Span};
    Line::from(vec![
        Span::styled(
            format!("{}{label:<width$} ", marker(selected)),
            theme.dim_style(),
        ),
        Span::styled(value, theme.line_style(selected)),
    ])
}
