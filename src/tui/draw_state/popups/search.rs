use crate::config::AddressMode;
use crate::constants::{ELLIPSIS, SEARCH_POPUP_MAX_HEIGHT_PERCENT, SEARCH_POPUP_MAX_WIDTH_PERCENT};
use crate::input::KeyCode;
use crate::state::{ScreenLayout, SearchMatch, SearchParams};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

const MIN_WIDTH: u16 = 38;
const CHROME_ROWS: u16 = 7;

const fn address_w(mode: AddressMode) -> usize {
    match mode {
        AddressMode::Dec => 5,
        AddressMode::Hex => 4,
    }
}

const fn prefix_w(mode: AddressMode, kind_w: usize) -> usize {
    1 + address_w(mode) + 1 + kind_w + 1
}

fn kind_w(matches: &[SearchMatch]) -> usize {
    matches
        .iter()
        .map(|m| m.cell.0.name().len())
        .max()
        .unwrap_or(0)
}

fn max_rows(area: Rect) -> u16 {
    let budget = u32::from(area.height) * u32::from(SEARCH_POPUP_MAX_HEIGHT_PERCENT) / 100;
    (budget as u16).saturating_sub(CHROME_ROWS).max(1)
}

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    search: &SearchParams,
    layout: &mut ScreenLayout,
    address_mode: AddressMode,
) {
    let rows = max_rows(area);
    layout.search_rows = rows;

    let len = search.matches.len();
    let (top, end) = super::window(search.top as usize, rows as usize, len);

    let mut lines = vec![
        super::query_line(theme, &search.query, len),
        Line::default(),
    ];

    let footer = [
        Hint::pair(KeyCode::Up, KeyCode::Down, "Select"),
        Hint::key(KeyCode::Enter, "Go"),
        Hint::key(KeyCode::Esc, "Close"),
    ];
    let longest = search.matches[top..end]
        .iter()
        .map(|m| m.text.chars().count())
        .max()
        .unwrap_or(0);
    let kind_w = kind_w(&search.matches);
    let (width, label_w) = fit(
        hints::min_width(MIN_WIDTH, &footer),
        area.width,
        longest,
        prefix_w(address_mode, kind_w),
    );

    if search.matches.is_empty() {
        for hint in [
            " Type an address or a label.",
            "",
            " Prefix for quick actions:",
            " h/i/c/d register type",
            " +/-     relative to the cursor",
            " n/x     explicit decimal / hex",
        ] {
            lines.push(Line::from(Span::styled(hint, theme.dim_style())));
        }
    } else {
        for i in top..end {
            lines.push(row(
                theme,
                &search.matches[i],
                &search.query,
                kind_w,
                label_w,
                i as u16 == search.selected,
                address_mode,
            ));
        }
        if top > 0 || end < len {
            lines.push(hints::more(theme, top, len - end));
        }
    }

    super::push_footer(&mut lines, theme, footer);

    super::render(frame, area, theme, "Go to", width, lines);
}

fn fit(min_width: u16, area_width: u16, longest: usize, prefix_w: usize) -> (u16, usize) {
    let needed = (prefix_w + longest + 2).min(u16::MAX as usize) as u16;
    let cap = (u32::from(area_width) * u32::from(SEARCH_POPUP_MAX_WIDTH_PERCENT) / 100) as u16;
    let width = min_width.max(needed.min(cap)).min(area_width);
    (width, (width as usize).saturating_sub(prefix_w + 2))
}

fn clipped(text: &str, width: usize) -> (Vec<char>, bool) {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return (chars, false);
    }
    (chars[..width.saturating_sub(ELLIPSIS.len())].to_vec(), true)
}

fn row(
    theme: &Theme,
    m: &SearchMatch,
    query: &str,
    kind_w: usize,
    label_w: usize,
    selected: bool,
    address_mode: AddressMode,
) -> Line<'static> {
    let style = |s: Style| if selected { theme.selected_style() } else { s };
    let (kind, address) = m.cell;

    let mut spans = vec![
        Span::styled(
            format!(
                " {:>w$} ",
                address_mode.display(address),
                w = address_w(address_mode)
            ),
            style(theme.accent_style()),
        ),
        Span::styled(
            format!("{:<kind_w$} ", kind.name()),
            style(theme.dim_style()),
        ),
    ];

    if m.labeled {
        spans.extend(label_spans(theme, &m.text, query, label_w, selected));
    } else {
        spans.push(Span::styled(
            super::truncate(&m.text, label_w),
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
        spans.push(Span::styled(ELLIPSIS, style(theme.dim_style())));
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
    use super::{MIN_WIDTH, clipped, fit, kind_w, prefix_w};
    use crate::config::AddressMode;
    use crate::register::RegisterType;
    use crate::state::SearchMatch;

    const PREFIX_W: usize = prefix_w(AddressMode::Dec, 8);

    fn hit(kind: RegisterType) -> SearchMatch {
        SearchMatch {
            cell: (kind, 0),
            text: String::new(),
            labeled: false,
        }
    }

    #[test]
    fn the_kind_column_fits_the_longest_type_listed() {
        assert_eq!(kind_w(&[]), 0);
        assert_eq!(
            kind_w(&[hit(RegisterType::Coil), hit(RegisterType::Input)]),
            "Input".len()
        );
        assert_eq!(
            kind_w(&[hit(RegisterType::Coil), hit(RegisterType::Discrete)]),
            "Discrete".len()
        );
    }

    #[test]
    fn the_prefix_holds_the_address_and_kind_with_one_space_around() {
        assert_eq!(prefix_w(AddressMode::Dec, 8), " 65535 Discrete ".len());
        assert_eq!(prefix_w(AddressMode::Hex, 5), " FFFF Input ".len());
    }

    #[test]
    fn short_labels_keep_the_minimum_width() {
        let (width, label_w) = fit(MIN_WIDTH, 120, 10, PREFIX_W);
        assert_eq!(width, MIN_WIDTH);
        assert_eq!(label_w, MIN_WIDTH as usize - PREFIX_W - 2);
    }

    #[test]
    fn long_labels_widen_the_popup_up_to_half_the_screen() {
        let (width, label_w) = fit(MIN_WIDTH, 200, 60, PREFIX_W);
        assert_eq!(width as usize, PREFIX_W + 60 + 2);
        assert_eq!(label_w, 60);

        let (width, label_w) = fit(MIN_WIDTH, 120, 60, PREFIX_W);
        assert_eq!(width, 60);
        assert_eq!(label_w, 60 - PREFIX_W - 2);

        let (width, label_w) = fit(MIN_WIDTH, 200, 5000, PREFIX_W);
        assert_eq!(width, 100);
        assert_eq!(label_w, 100 - PREFIX_W - 2);
    }

    #[test]
    fn the_minimum_width_wins_over_the_cap_on_narrow_screens() {
        let (width, label_w) = fit(MIN_WIDTH, 50, 60, PREFIX_W);
        assert_eq!(width, MIN_WIDTH);
        assert_eq!(label_w, MIN_WIDTH as usize - PREFIX_W - 2);
    }

    #[test]
    fn a_tiny_screen_leaves_no_room_for_labels_without_panicking() {
        let (width, label_w) = fit(MIN_WIDTH, 10, 60, PREFIX_W);
        assert_eq!(width, 10);
        assert_eq!(label_w, 0);
        assert_eq!(clipped("abc", 0), (Vec::new(), true));
    }

    #[test]
    fn clipping_leaves_room_for_the_ellipsis() {
        assert_eq!(clipped("abc", 3), ("abc".chars().collect(), false));
        assert_eq!(clipped("abcdef", 5), ("ab".chars().collect(), true));
        assert_eq!(clipped("ș-ț-â-x", 6), ("ș-ț".chars().collect(), true));
    }
}
