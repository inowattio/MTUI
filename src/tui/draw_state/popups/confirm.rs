use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::StatusMessage;
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
    title: &str,
    prompt: &str,
    result: &Option<StatusMessage>,
) {
    let mut lines = vec![
        Line::from(Span::styled(prompt.to_string(), theme.base())),
        hints::footer(
            theme,
            [
                Hint::key(kb.action, "Confirm"),
                Hint::pair(KeyCode::Backspace, kb.exit, "Cancel"),
            ],
        ),
    ];

    if let Some(result) = result {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            result.text.clone(),
            theme.message_style(result.kind),
        )));
    }

    super::render(frame, area, theme, title, 60, lines);
}
