pub mod logs;
pub mod popups;
pub mod read;
pub mod settings;

pub const fn marker(selected: bool) -> &'static str {
    if selected { "> " } else { "  " }
}

pub fn action_line(
    theme: &crate::tui::theme::Theme,
    label: &str,
    selected: bool,
    style: ratatui::style::Style,
    suffix: Option<ratatui::text::Span<'static>>,
) -> ratatui::text::Line<'static> {
    use ratatui::text::{Line, Span};
    let style = if selected {
        theme.selected_style()
    } else {
        style
    };
    let mut spans = vec![Span::styled(format!("{}{label}", marker(selected)), style)];
    spans.extend(suffix);
    Line::from(spans)
}

pub fn cyclable(value: &str) -> String {
    format!("< {value} >")
}

pub fn edit_value(value: String, selected: bool, cyclable_field: bool) -> String {
    match (selected, cyclable_field) {
        (true, true) => cyclable(&value),
        (true, false) => format!("{value}_"),
        (false, _) => value,
    }
}

pub fn field_row(
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

pub fn dim_line(
    theme: &crate::tui::theme::Theme,
    text: impl Into<std::borrow::Cow<'static, str>>,
) -> ratatui::text::Line<'static> {
    use ratatui::text::{Line, Span};
    Line::from(Span::styled(text, theme.dim_style()))
}
