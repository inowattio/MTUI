use crate::config::Keybinds;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds) {
    const LABEL_W: usize = 8;

    let field = |label: &str, value: &str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!(" {label:<LABEL_W$}"), theme.dim_style()),
            Span::styled(value.to_string(), theme.base()),
        ])
    };

    let lines: Vec<Line> = vec![
        field("", "MTUI"),
        Line::from(Span::styled(
            format!(" {}", env!("CARGO_PKG_DESCRIPTION")),
            theme.dim_style(),
        )),
        Line::default(),
        field("Version", env!("CARGO_PKG_VERSION")),
        field("Commit", env!("MTUI_GIT_HASH")),
        field("Repo", env!("CARGO_PKG_REPOSITORY")),
        Line::default(),
        hints::footer(theme, [Hint::key(kb.exit, "Close")]),
    ];

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    // borders (2) + a column of right padding
    let width = content_w + 3;

    super::render(frame, area, theme, "About", width, lines);
}
