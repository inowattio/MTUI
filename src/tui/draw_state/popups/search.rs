use crate::app::App;
use crate::constants::SEARCH_POPUP_MAX_HEIGHT_PERCENT;
use crate::state::{SearchMatch, SearchParams};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const LABEL_W: usize = 24;

const CHROME_ROWS: u16 = 7;

fn max_rows(area: Rect) -> u16 {
    let budget = u32::from(area.height) * u32::from(SEARCH_POPUP_MAX_HEIGHT_PERCENT) / 100;
    (budget as u16).saturating_sub(CHROME_ROWS).max(1)
}

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, search: &SearchParams) {
    let kb = &app.config.keybinds;
    let rows = max_rows(area);
    app.search_rows.set(rows);

    let len = search.matches.len();
    let (top, end) = super::window(search.top as usize, rows as usize, len);

    let mut lines = vec![
        super::query_line(theme, &search.query, len),
        Line::default(),
    ];

    if search.matches.is_empty() {
        for hint in [
            " Type an address (x6F for hex) or a label.",
            " +N / -N moves relative to the cursor.",
            " h/i/c/d prefix picks the register type.",
        ] {
            lines.push(Line::from(Span::styled(hint, theme.dim_style())));
        }
    } else {
        for i in top..end {
            lines.push(row(
                theme,
                &search.matches[i],
                &search.query,
                i as u16 == search.selected,
            ));
        }
    }

    let footer = [
        Hint::pair(kb.move_up, kb.move_down, "Select"),
        Hint::key(kb.action, "Go"),
        Hint::key(kb.exit, "Close"),
    ];
    lines.push(hints::more(theme, top, len.saturating_sub(end)));
    let width = hints::min_width(44, &footer);
    super::push_footer(&mut lines, theme, footer);

    super::render(frame, area, theme, "Go to", width, lines);
}

fn row(theme: &Theme, m: &SearchMatch, query: &str, selected: bool) -> Line<'static> {
    let style = |s: Style| if selected { theme.selected_style() } else { s };
    let (kind, address) = m.cell;

    let mut spans = vec![
        Span::styled(format!(" {address:>5}  "), style(theme.accent_style())),
        Span::styled(
            format!("{:<10}", format!("{kind:?}")),
            style(theme.dim_style()),
        ),
    ];

    if m.labeled {
        spans.extend(label_spans(theme, &m.text, query, selected));
    } else {
        spans.push(Span::styled(
            m.text.clone(),
            style(theme.dim_style()).add_modifier(Modifier::ITALIC),
        ));
    }

    Line::from(spans)
}

fn label_spans(theme: &Theme, text: &str, query: &str, selected: bool) -> Vec<Span<'static>> {
    let style = |s: Style| if selected { theme.selected_style() } else { s };
    let mut spans = Vec::new();

    let chars: Vec<char> = text.chars().collect();
    let truncated = chars.len() > LABEL_W;
    let shown = if truncated {
        &chars[..LABEL_W - 1]
    } else {
        &chars[..]
    };

    // Split the label into runs of matched/unmatched characters so the part
    // that matched the query lights up.
    let hits = match_positions(query, text);
    let mut run = String::new();
    let mut run_hit = false;
    for (i, &ch) in shown.iter().enumerate() {
        let hit = hits.contains(&i);
        if hit != run_hit && !run.is_empty() {
            spans.push(label_span(
                theme,
                std::mem::take(&mut run),
                run_hit,
                selected,
            ));
        }
        run_hit = hit;
        run.push(ch);
    }
    if !run.is_empty() {
        spans.push(label_span(theme, run, run_hit, selected));
    }
    if truncated {
        spans.push(Span::styled("\u{2026}", style(theme.dim_style())));
    }

    spans
}

fn label_span(theme: &Theme, text: String, hit: bool, selected: bool) -> Span<'static> {
    let style = if selected {
        theme.selected_style()
    } else if hit {
        theme.accent_style()
    } else {
        theme.base()
    };
    Span::styled(text, style)
}

fn match_positions(query: &str, text: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_ascii_lowercase();
    let text = text.to_ascii_lowercase();

    if let Some(byte_pos) = text.find(&query) {
        let start = text[..byte_pos].chars().count();
        return (start..start + query.chars().count()).collect();
    }

    let mut positions = Vec::new();
    let mut want = query.chars().peekable();
    for (i, ch) in text.chars().enumerate() {
        if want.peek() == Some(&ch) {
            positions.push(i);
            want.next();
        }
    }
    if want.peek().is_some() {
        Vec::new()
    } else {
        positions
    }
}
