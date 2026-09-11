use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::StatusMessage;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    read_count: usize,
    result: &Option<StatusMessage>,
) {
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(
            format!(" Export {read_count} read register(s) to a file?"),
            theme.base(),
        )),
        Line::from(Span::styled(
            " Written as dump_<date>_<time>.txt",
            theme.dim_style(),
        )),
        Line::default(),
    ];

    if let Some(result) = result {
        lines.push(Line::from(Span::styled(
            format!(" {}", result.text),
            theme.message_style(result.kind),
        )));
        lines.push(Line::default());
    }

    lines.push(hints::footer(
        theme,
        [
            Hint::key(kb.action, "Confirm"),
            Hint::pair(KeyCode::Backspace, KeyCode::Esc, "Cancel"),
        ],
    ));

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w.saturating_add(3).min(area.width);
    super::render(frame, area, theme, "Dump", width, lines);
}
