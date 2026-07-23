use crate::app::App;
use crate::config::Keybinds;
use crate::constants::NO_VALUE;
use crate::interpretator::format_ago;
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use chrono::Utc;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;
use std::fmt::Write as _;

const ERROR_W: usize = 48;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, kb: &Keybinds, app: &App) {
    const LABEL_W: usize = 8;
    let s = &app.stats;

    let field = |label: &str, value: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!(" {label:<LABEL_W$}"), theme.dim_style()),
            Span::styled(value, theme.base()),
        ])
    };

    let mut reads = format!("{} ok \u{b7} {} errors", s.reads_ok, s.read_errors);
    if s.read_errors > 0 {
        let total = s.reads_ok + s.read_errors;
        let _ = write!(
            reads,
            " ({:.1}%)",
            s.read_errors as f64 * 100.0 / total as f64
        );
    }

    let latency = match s.latency() {
        Some((min, avg, max)) => {
            format!("{min:.2?} min \u{b7} {avg:.2?} avg \u{b7} {max:.2?} max")
        }
        None => NO_VALUE.to_string(),
    };

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            " Counted since connect or clear",
            theme.dim_style(),
        )),
        Line::default(),
        field("Reads", reads),
        field(
            "Writes",
            format!("{} ok \u{b7} {} errors", s.writes_ok, s.write_errors),
        ),
        field("Latency", latency),
    ];

    if let Some((message, at)) = s.last_error() {
        let mut short: String = message.chars().take(ERROR_W).collect();
        if short.len() < message.len() {
            short.push('\u{2026}');
        }
        let ago = format_ago(Utc::now().signed_duration_since(at));
        lines.push(Line::default());
        lines.push(field("Error", format!("{short} \u{b7} {ago}")));
    }

    super::push_footer(&mut lines, theme, [Hint::key(kb.exit, "Close")]);

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    // borders (2) + a column of right padding
    let width = content_w + 3;

    super::render(frame, area, theme, "Session stats", width, lines);
}
