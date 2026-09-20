use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::LogsParams;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use crate::writes_log::WriteEntry;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

const TIME_W: usize = 23;
const SLAVE_W: usize = 5;
const ADDR_W: usize = 5;
const TYPE_W: usize = 8;
const PREV_W: usize = 10;
const VALUE_W: usize = 24;
const GAP: &str = "  ";

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    logs: &LogsParams,
    logging: bool,
) {
    let visible = LogsParams::VISIBLE as usize;
    let len = logs.entries.len();
    let (top, end) = super::window(logs.top as usize, visible, len);

    let mut lines = vec![
        summary_line(theme, logs),
        Line::default(),
        header_line(theme),
    ];

    if len == 0 {
        let text = if logging {
            "   no writes logged yet"
        } else {
            "   logging is off, enable \"Log writes\" in settings"
        };
        lines.push(Line::from(Span::styled(text, theme.dim_style())));
        for _ in 1..visible {
            lines.push(Line::default());
        }
    } else {
        for (i, entry) in logs.entries[top..end].iter().enumerate() {
            lines.push(entry_line(theme, entry, (top + i) % 2 == 1));
        }
        for _ in end..top + visible {
            lines.push(Line::default());
        }
    }

    lines.push(hints::more(theme, top, len.saturating_sub(end)));
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(KeyCode::Up, KeyCode::Down, "Scroll"),
            Hint::pair(kb.page_up, kb.page_down, "Page"),
            Hint::key(KeyCode::Esc, "Close"),
        ],
    ));

    super::render(frame, area, theme, "Write log", width(), lines);
}

fn width() -> u16 {
    (1 + TIME_W + SLAVE_W + ADDR_W + TYPE_W + PREV_W + VALUE_W + GAP.len() * 5 + 2) as u16
}

fn summary_line(theme: &Theme, logs: &LogsParams) -> Line<'static> {
    let count = match logs.entries.len() {
        1 => "1 write".to_string(),
        n => format!("{n} writes"),
    };
    Line::from(vec![
        Span::styled(format!(" {count}"), theme.accent_style()),
        Span::styled(format!("  {}", logs.path), theme.dim_style()),
    ])
}

fn header_line(theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!(
            " {:<TIME_W$}{GAP}{:>SLAVE_W$}{GAP}{:>ADDR_W$}{GAP}{:<TYPE_W$}{GAP}{:>PREV_W$}{GAP}{:<VALUE_W$}",
            "TIME", "SLAVE", "ADDR", "TYPE", "PREVIOUS", "VALUE"
        ),
        theme.header_style(),
    ))
}

fn entry_line(theme: &Theme, entry: &WriteEntry, zebra: bool) -> Line<'static> {
    let row = theme.row_style(zebra, false);
    let time = entry.timestamp.replacen('T', " ", 1);
    Line::from(vec![
        Span::styled(
            format!(" {:<TIME_W$}{GAP}", super::truncate(&time, TIME_W)),
            row.patch(theme.dim_style()),
        ),
        Span::styled(format!("{:>SLAVE_W$}{GAP}", entry.slave), row),
        Span::styled(format!("{:>ADDR_W$}{GAP}", entry.address), row),
        Span::styled(
            format!("{:<TYPE_W$}{GAP}", super::truncate(&entry.kind, TYPE_W)),
            row.patch(theme.accent_style()),
        ),
        Span::styled(
            format!(
                "{:>PREV_W$}{GAP}",
                super::truncate(&entry.display_previous(), PREV_W)
            ),
            row.patch(theme.dim_style()),
        ),
        Span::styled(
            format!(
                "{:<VALUE_W$}",
                super::truncate(&entry.display_value(), VALUE_W)
            ),
            row,
        ),
    ])
}
