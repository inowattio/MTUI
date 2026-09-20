use crate::input::KeyCode;
use crate::state::LabelParams;
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, label: &LabelParams) {
    let (text, text_style) = if label.text.is_empty() {
        ("(empty - will remove)".to_string(), theme.dim_style())
    } else {
        (label.text.clone(), theme.base())
    };

    let mut lines = vec![
        Line::default(),
        Line::from(vec![
            Span::styled(" Text: ", theme.dim_style()),
            Span::styled(text, text_style),
            super::cursor_span(theme),
        ]),
    ];
    super::push_footer(
        &mut lines,
        theme,
        [
            Hint::key(KeyCode::Enter, "Set"),
            Hint::key(KeyCode::Esc, "Cancel"),
        ],
    );

    super::render(frame, area, theme, "Label", 48, lines);
}
