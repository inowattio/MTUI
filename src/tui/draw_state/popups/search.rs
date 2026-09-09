use crate::app::App;
use crate::constants::{SEARCH_POPUP_MAX_HEIGHT_PERCENT, SEARCH_POPUP_MAX_WIDTH_PERCENT};
use crate::input::KeyCode;
use crate::state::{SearchMatch, SearchParams};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const PREFIX_W: usize = 18;
const MIN_WIDTH: u16 = 44;
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

    let footer = [
        Hint::pair(kb.move_up, kb.move_down, "Select"),
        Hint::key(kb.action, "Go"),
        Hint::key(KeyCode::Esc, "Close"),
    ];
    let longest = search.matches[top..end]
        .iter()
        .map(|m| m.text.chars().count())
        .max()
        .unwrap_or(0);
    let (width, label_w) = fit(hints::min_width(MIN_WIDTH, &footer), area.width, longest);

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
                label_w,
                i as u16 == search.selected,
            ));
        }
    }

    lines.push(hints::more(theme, top, len.saturating_sub(end)));
    super::push_footer(&mut lines, theme, footer);

    super::render(frame, area, theme, "Go to", width, lines);
}

fn fit(min_width: u16, area_width: u16, longest: usize) -> (u16, usize) {
    let needed = (PREFIX_W + longest + 2).min(u16::MAX as usize) as u16;
    let cap = (u32::from(area_width) * u32::from(SEARCH_POPUP_MAX_WIDTH_PERCENT) / 100) as u16;
    let width = min_width.max(needed.min(cap)).min(area_width);
    (width, (width as usize).saturating_sub(PREFIX_W + 2))
}

fn clipped(text: &str, width: usize) -> (Vec<char>, bool) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return (chars, false);
    }
    (chars[..width.saturating_sub(1)].to_vec(), true)
}

fn row(
    theme: &Theme,
    m: &SearchMatch,
    query: &str,
    label_w: usize,
    selected: bool,
) -> Line<'static> {
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
        spans.extend(label_spans(theme, &m.text, query, label_w, selected));
    } else {
        let (shown, truncated) = clipped(&m.text, label_w);
        let mut text: String = shown.into_iter().collect();
        if truncated {
            text.push('\u{2026}');
        }
        spans.push(Span::styled(
            text,
            style(theme.dim_style()).add_modifier(Modifier::ITALIC),
        ));
    }

    Line::from(spans)
}

fn label_spans(
    theme: &Theme,
    text: &str,
    query: &str,
    label_w: usize,
    selected: bool,
) -> Vec<Span<'static>> {
    let style = |s: Style| if selected { theme.selected_style() } else { s };
    let mut spans = Vec::new();

    let (shown, truncated) = clipped(text, label_w);

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

#[cfg(test)]
mod tests {
    use super::{MIN_WIDTH, PREFIX_W, clipped, fit};

    #[test]
    fn short_labels_keep_the_minimum_width() {
        let (width, label_w) = fit(MIN_WIDTH, 120, 10);
        assert_eq!(width, MIN_WIDTH);
        assert_eq!(label_w, MIN_WIDTH as usize - PREFIX_W - 2);
    }

    #[test]
    fn long_labels_widen_the_popup_up_to_half_the_screen() {
        let (width, label_w) = fit(MIN_WIDTH, 200, 60);
        assert_eq!(width as usize, PREFIX_W + 60 + 2);
        assert_eq!(label_w, 60);

        let (width, label_w) = fit(MIN_WIDTH, 120, 60);
        assert_eq!(width, 60);
        assert_eq!(label_w, 60 - PREFIX_W - 2);

        let (width, label_w) = fit(MIN_WIDTH, 200, 5000);
        assert_eq!(width, 100);
        assert_eq!(label_w, 100 - PREFIX_W - 2);
    }

    #[test]
    fn the_minimum_width_wins_over_the_cap_on_narrow_screens() {
        let (width, label_w) = fit(MIN_WIDTH, 50, 60);
        assert_eq!(width, MIN_WIDTH);
        assert_eq!(label_w, MIN_WIDTH as usize - PREFIX_W - 2);
    }

    #[test]
    fn a_tiny_screen_leaves_no_room_for_labels_without_panicking() {
        let (width, label_w) = fit(MIN_WIDTH, 10, 60);
        assert_eq!(width, 10);
        assert_eq!(label_w, 0);
        assert_eq!(clipped("abc", 0), (Vec::new(), true));
    }

    #[test]
    fn clipping_leaves_room_for_the_ellipsis() {
        assert_eq!(clipped("abc", 3), ("abc".chars().collect(), false));
        assert_eq!(clipped("abcd", 3), ("ab".chars().collect(), true));
        assert_eq!(clipped("ș-ț-â", 4), ("ș-ț".chars().collect(), true));
    }
}
