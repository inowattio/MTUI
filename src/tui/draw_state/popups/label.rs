use crate::config::Keybinds;
use crate::state::LabelParams;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, label: &LabelParams) {
    let (text, text_style) = if label.text.is_empty() {
        ("(empty - will remove)".to_string(), theme.dim_style())
    } else {
        (label.text.clone(), theme.base())
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Label ", theme.dim_style()),
            Span::styled(label.position.to_string(), theme.accent_style()),
            Span::styled(format!("  ({:?})", label.register_type), theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("Text: ", theme.dim_style()),
            Span::styled(text, text_style),
            Span::styled("_", theme.accent_style()),
        ]),
        Line::from(Span::styled(
            format!(
                " {} \u{b7} set (empty removes)   {} \u{b7} cancel",
                kb.action, kb.exit
            ),
            theme.dim_style(),
        )),
    ];

    if let Some(result) = &label.result {
        lines.push(Line::from(Span::styled(result.clone(), theme.err_style())));
    }

    super::render(frame, area, theme, "Label", 48, lines);
}
