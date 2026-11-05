use crate::app::App;
use crate::functions::DiagnosticSub;
use crate::input::KeyCode;
use crate::modbus::DeviceIdAccess;
use crate::state::{FunctionField, FunctionParams, FunctionsParams, OtherFunction, ScreenLayout};
use crate::tui::draw_state::{dim_line, edit_value, field_row};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

const CODE_W: usize = 9;
const MIN_DETAIL_W: usize = 40;
const ROW_GAP: usize = 2;
const SEPARATOR: &str = " │ ";
const DATA_FIELD_W: usize = 32;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    app: &App,
    params: &FunctionsParams,
    layout: &mut ScreenLayout,
) {
    let kb = &app.config.keybinds;
    let form = params.current();
    let detail = params.detail;
    let fields = form.fields();

    let list_w = 1
        + CODE_W
        + 2
        + OtherFunction::ALL
            .iter()
            .map(|f| f.label().chars().count())
            .max()
            .unwrap_or(0)
        + 1;
    let list = list_lines(theme, params, list_w);

    let footer_lines = 2;
    let max_body = (area.height.saturating_sub(2) as usize).saturating_sub(footer_lines);
    let detail_content_w = params
        .forms
        .iter()
        .map(detail_width)
        .max()
        .unwrap_or(0)
        .max(MIN_DETAIL_W)
        + 1;
    let content_w = list_w + SEPARATOR.chars().count() + detail_content_w + 2;
    let widest_footer = footer(kb, form, detail, true);
    let width = hints::min_width(u16::try_from(content_w).unwrap_or(u16::MAX), &widest_footer)
        .min(area.width);
    let detail_w = (width as usize).saturating_sub(2 + list_w + SEPARATOR.chars().count());

    let field_lines = field_lines(theme, form, detail, detail_w);
    let row_label_w = form
        .rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let label_w = row_label_w.min(detail_w / 2);
    let value_w = detail_w.saturating_sub(1 + label_w + ROW_GAP).max(1);
    let visual: Vec<(&str, String)> = form
        .rows
        .iter()
        .flat_map(|(label, value)| {
            wrap_value(value, value_w)
                .into_iter()
                .enumerate()
                .map(move |(i, chunk)| (if i == 0 { label.as_str() } else { "" }, chunk))
        })
        .collect();
    let len = visual.len();
    let fixed = detail_height(form) - form.rows.len();
    let body_h = params
        .forms
        .iter()
        .map(detail_height)
        .max()
        .unwrap_or(0)
        .max(fixed + len)
        .max(list.len())
        .min(max_body);
    let available = body_h.saturating_sub(fixed);
    let (top, end, max_scroll) = rows_window(form.scroll, available, len);
    layout.v_max_offset = max_scroll;
    let overflow = max_scroll > 0;
    let footer = footer(kb, form, detail, overflow);

    let mut right: Vec<Line<'static>> = vec![Line::from(Span::styled(
        format!(" {} {}", form.function.code(), form.function.label()),
        theme.accent_style(),
    ))];
    right.extend(field_lines);
    if len > 0 {
        right.push(Line::default());
        for (label, chunk) in &visual[top..end] {
            let label = super::truncate(label, label_w);
            right.push(Line::from(vec![
                Span::styled(
                    format!(" {label:<label_w$}{:ROW_GAP$}", ""),
                    theme.dim_style(),
                ),
                Span::styled(chunk.clone(), theme.base()),
            ]));
        }
        if overflow {
            right.push(hints::more(theme, top, len - end));
        }
    } else if form.status.is_none() {
        right.push(Line::default());
        let hint = match (fields.is_empty(), detail) {
            (true, _) => " Press Enter to read",
            (false, false) => " Press Enter to edit",
            (false, true) => " Press Enter to send",
        };
        right.push(dim_line(theme, hint));
    }
    super::push_status(&mut right, theme, form.status.as_ref());

    let list_top = params
        .selected
        .saturating_sub(body_h.saturating_sub(1))
        .min(list.len().saturating_sub(body_h));
    let mut list = list.into_iter().skip(list_top);
    let mut right = right.into_iter();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(body_h + footer_lines);
    for _ in 0..body_h {
        let left = list.next().unwrap_or_default();
        let pad = list_w.saturating_sub(left.width());
        let mut spans = left.spans;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(SEPARATOR, theme.dim_style()));
        spans.extend(right.next().unwrap_or_default().spans);
        lines.push(Line::from(spans));
    }
    lines.push(Line::default());
    lines.push(hints::footer(theme, footer));

    super::render(frame, area, theme, "Other functions", width, lines);
}

fn wrap_value(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in value.split(' ') {
        let mut word: Vec<char> = word.chars().collect();
        let current_len = current.chars().count();
        let joined = current_len + usize::from(current_len > 0) + word.len();
        if current_len > 0 && joined <= width {
            current.push(' ');
            current.extend(word);
            continue;
        }
        if current_len > 0 {
            lines.push(std::mem::take(&mut current));
        }
        while word.len() > width {
            lines.push(word.drain(..width).collect());
        }
        current = word.into_iter().collect();
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

fn tail(text: &str, width: usize) -> String {
    let len = text.chars().count();
    if len <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(crate::constants::ELLIPSIS.chars().count());
    let mut out = crate::constants::ELLIPSIS.to_string();
    out.extend(text.chars().skip(len - keep));
    out
}

const fn detail_height(form: &FunctionParams) -> usize {
    let fields = form.fields().len();
    let fields_block = if fields == 0 { 0 } else { 1 + fields };
    let results_block = match (form.rows.len(), form.status.is_some()) {
        (0, true) => 0,
        (0, false) => 2,
        (rows, _) => 1 + rows,
    };
    let status_block = if form.status.is_some() { 2 } else { 0 };
    1 + fields_block + results_block + status_block
}

fn field_value_width(form: &FunctionParams, field: FunctionField) -> usize {
    match field {
        FunctionField::SubFunction => {
            5 + DiagnosticSub::ALL
                .iter()
                .map(|sub| sub.label().chars().count())
                .max()
                .unwrap_or(0)
        }
        FunctionField::Access => DeviceIdAccess::ALL
            .iter()
            .map(|access| access.label().chars().count())
            .max()
            .unwrap_or(0),
        FunctionField::Data => form.data.chars().count().min(DATA_FIELD_W),
        FunctionField::File => form.file.chars().count(),
        FunctionField::Record => form.record.chars().count(),
        FunctionField::Length => form.length.chars().count(),
        FunctionField::Address => form.address.chars().count(),
    }
}

fn detail_width(form: &FunctionParams) -> usize {
    let fields = form.fields();
    let field_label_w = fields
        .iter()
        .map(|f| f.label().chars().count())
        .max()
        .unwrap_or(0);
    let fields_w = fields
        .iter()
        .map(|&field| 2 + field_label_w + 1 + field_value_width(form, field) + 4)
        .max()
        .unwrap_or(0);
    let row_label_w = form
        .rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let rows_w = form
        .rows
        .iter()
        .map(|(_, value)| 1 + row_label_w + ROW_GAP + value.chars().count())
        .max()
        .unwrap_or(0);
    let header_w = 2 + form.function.code().len() + form.function.label().chars().count();
    let status_w = form
        .status
        .as_ref()
        .map_or(0, |status| 1 + status.text.chars().count());
    fields_w.max(rows_w).max(header_w).max(status_w)
}

fn list_lines(theme: &Theme, params: &FunctionsParams, list_w: usize) -> Vec<Line<'static>> {
    let current = params.current().function;
    OtherFunction::ALL
        .iter()
        .map(|&function| {
            let text = format!(" {:<CODE_W$}  {}", function.code(), function.label());
            if function == current {
                let style = if params.detail {
                    theme.accent_style()
                } else {
                    theme.selected_style()
                };
                Line::from(Span::styled(format!("{text:<list_w$}"), style))
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<CODE_W$}  ", function.code()),
                        theme.dim_style(),
                    ),
                    Span::styled(function.label(), theme.base()),
                ])
            }
        })
        .collect()
}

fn field_lines(
    theme: &Theme,
    form: &FunctionParams,
    detail: bool,
    detail_w: usize,
) -> Vec<Line<'static>> {
    let fields = form.fields();
    if fields.is_empty() {
        return Vec::new();
    }
    let selected = form.current_field().filter(|_| detail);
    let label_w = fields
        .iter()
        .map(|f| f.label().chars().count())
        .max()
        .unwrap_or(0);
    let value_w = detail_w.saturating_sub(2 + label_w + 1 + 1);
    let text = |value: &str| tail(value, value_w);
    let mut lines = vec![Line::default()];
    for &field in fields {
        let is_selected = selected == Some(field);
        let value = match field {
            FunctionField::SubFunction => edit_value(
                format!("0x{:02X} {}", form.sub.code(), form.sub.label()),
                is_selected,
                true,
            ),
            FunctionField::Access => edit_value(form.access.label().to_string(), is_selected, true),
            FunctionField::Data => edit_value(text(&form.data), is_selected, false),
            FunctionField::File => edit_value(text(&form.file), is_selected, false),
            FunctionField::Record => edit_value(text(&form.record), is_selected, false),
            FunctionField::Length => edit_value(text(&form.length), is_selected, false),
            FunctionField::Address => edit_value(text(&form.address), is_selected, false),
        };
        lines.push(field_row(theme, field.label(), label_w, value, is_selected));
    }
    lines
}

fn footer(
    kb: &crate::config::Keybinds,
    form: &FunctionParams,
    detail: bool,
    overflow: bool,
) -> Vec<Hint> {
    let fields = form.fields();
    let mut footer = Vec::new();
    if detail {
        match form.current_field() {
            Some(FunctionField::SubFunction) => {
                footer.push(Hint::pair(KeyCode::Left, KeyCode::Right, "Sub-function"));
            }
            Some(FunctionField::Access) => {
                footer.push(Hint::pair(KeyCode::Left, KeyCode::Right, "Access"));
            }
            _ => {}
        }
        if fields.len() > 1 {
            footer.push(Hint::pair(KeyCode::Up, KeyCode::Down, "Field"));
        }
        footer.push(Hint::key(KeyCode::Enter, "Send"));
    } else {
        footer.push(Hint::pair(KeyCode::Up, KeyCode::Down, "Function"));
        if fields.is_empty() {
            footer.push(Hint::key(KeyCode::Enter, "Read"));
        } else {
            footer.push(Hint::key(KeyCode::Enter, "Edit"));
            footer.push(Hint::key(kb.refresh, "Send"));
        }
    }
    if overflow {
        footer.push(Hint::pair(kb.page_up, kb.page_down, "Scroll"));
    }
    footer.push(Hint::key(
        KeyCode::Esc,
        if detail { "Back" } else { "Close" },
    ));
    footer
}

fn rows_window(scroll: u16, available: usize, len: usize) -> (usize, usize, u16) {
    if len <= available {
        return (0, len, 0);
    }
    let visible = available.saturating_sub(1).max(1);
    let max_top = len - visible;
    let top = (scroll as usize).min(max_top);
    (
        top,
        top + visible,
        u16::try_from(max_top).unwrap_or(u16::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::{rows_window, tail, wrap_value};

    #[test]
    fn wrap_value_breaks_on_spaces() {
        assert_eq!(wrap_value("4D 54 55 49", 5), vec!["4D 54", "55 49"]);
        assert_eq!(wrap_value("short", 20), vec!["short"]);
        assert_eq!(wrap_value("", 5), vec![""]);
    }

    #[test]
    fn wrap_value_splits_long_words() {
        assert_eq!(
            wrap_value("http://example.com/x", 8),
            vec!["http://e", "xample.c", "om/x"]
        );
        assert_eq!(wrap_value("ab cdefgh", 3), vec!["ab", "cde", "fgh"]);
        assert_eq!(wrap_value("abc", 0), vec!["a", "b", "c"]);
    }

    #[test]
    fn tail_keeps_the_end_visible() {
        assert_eq!(tail("1234", 10), "1234");
        let shown = tail("0123456789", 6);
        assert!(shown.ends_with("789"));
        assert_eq!(shown.chars().count(), 6);
    }

    #[test]
    fn rows_window_fits_without_scroll() {
        assert_eq!(rows_window(0, 10, 4), (0, 4, 0));
        assert_eq!(rows_window(7, 10, 10), (0, 10, 0));
        assert_eq!(rows_window(3, 0, 0), (0, 0, 0));
    }

    #[test]
    fn rows_window_reserves_indicator_line() {
        assert_eq!(rows_window(0, 5, 20), (0, 4, 16));
        assert_eq!(rows_window(2, 5, 20), (2, 6, 16));
    }

    #[test]
    fn rows_window_clamps_scroll() {
        assert_eq!(rows_window(16, 5, 20), (16, 20, 16));
        assert_eq!(rows_window(u16::MAX, 5, 20), (16, 20, 16));
    }

    #[test]
    fn rows_window_tiny_area_shows_one_row() {
        assert_eq!(rows_window(0, 0, 3), (0, 1, 2));
        assert_eq!(rows_window(9, 1, 3), (2, 3, 2));
    }

    #[test]
    fn rows_window_saturates_huge_max_scroll() {
        let len = usize::from(u16::MAX) + 10;
        let (top, end, max) = rows_window(u16::MAX, 3, len);
        assert_eq!(max, u16::MAX);
        assert_eq!(
            (top, end),
            (usize::from(u16::MAX), usize::from(u16::MAX) + 2)
        );
    }
}
