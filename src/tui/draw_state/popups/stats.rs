use crate::app::App;
use crate::constants::{ELLIPSIS, NO_VALUE};
use crate::input::KeyCode;
use crate::interpretator::format_ago;
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use std::fmt::Write as _;

const ERROR_W: usize = 48;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App) {
    const LABEL_W: usize = 8;
    let s = &app.stats;

    let field = |label: &str, value: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!(" {label:<LABEL_W$}"), theme.dim_style()),
            Span::styled(value, theme.base()),
        ])
    };

    let mut reads = format!("{} ok | {} errors", s.reads_ok, s.read_errors);
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
            format!("{min:.2?} min | {avg:.2?} avg | {max:.2?} max")
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
            format!("{} ok | {} errors", s.writes_ok, s.write_errors),
        ),
        field("Latency", latency),
    ];

    if let Some((message, at)) = s.last_error() {
        let mut short: String = message.chars().take(ERROR_W).collect();
        if short.len() < message.len() {
            short.truncate(ERROR_W.saturating_sub(ELLIPSIS.len()));
            short.push_str(ELLIPSIS);
        }
        let ago = format_ago(Utc::now().signed_duration_since(at));
        lines.push(Line::default());
        lines.push(field("Error", format!("{short} | {ago}")));
    }

    super::push_footer(&mut lines, theme, [Hint::key(KeyCode::Esc, "Close")]);

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    // borders (2) + a column of right padding
    let width = content_w + 3;

    super::render(frame, area, theme, "Session stats", width, lines);
}
