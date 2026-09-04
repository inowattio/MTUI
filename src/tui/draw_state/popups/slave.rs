use crate::config::Keybinds;
use crate::state::{SlaveField, SlaveParams};
use crate::tui::draw_state::{dim_line, edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const LEFT_W: u16 = 40;
const DIVIDER_W: u16 = 3;
const SIDE_W: u16 = 38;
const LABEL_W: usize = 13;
const LIST_ROWS: usize = 6;

const PREFIX_W: usize = 7;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    params: &SlaveParams,
) {
    let sel = params.current_field();
    let left = form_lines(params, sel, theme);
    let right = hit_lines(params, sel, theme);

    let primary = match sel {
        SlaveField::Id => "Set",
        SlaveField::Hit(_) => "Use id",
        _ if params.active => "Stop scan",
        _ => "Start scan",
    };
    let nav = [
        Hint::pair(kb.move_up, kb.move_down, "Field"),
        Hint::key(kb.pause, "Toggle mode"),
    ];
    let actions = [Hint::key(kb.action, primary), Hint::key(kb.exit, "Close")];
    let footer_w = hints::width(&nav).max(hints::width(&actions)) as u16;

    let mut tail: Vec<Line> = Vec::new();
    super::push_status(&mut tail, theme, params.status.as_ref());
    tail.push(Line::default());
    tail.push(hints::footer(theme, nav));
    tail.push(hints::footer(theme, actions));

    let body_w = LEFT_W + DIVIDER_W + SIDE_W;
    let width = (body_w + 2).max(footer_w);
    let body_rows = left.len().max(right.len()) as u16;
    let height = body_rows + tail.len() as u16 + 2;
    let rect = super::centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    let block = theme
        .panel(" Slave")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme.bg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let [body, tail_area] =
        Layout::vertical([Constraint::Length(body_rows), Constraint::Min(0)]).areas(inner);
    let [l, divider, r] = Layout::horizontal([
        Constraint::Length(LEFT_W),
        Constraint::Length(DIVIDER_W),
        Constraint::Min(0),
    ])
    .areas(body);
    frame.render_widget(Paragraph::new(left), l);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(theme.dim_style()),
        divider,
    );
    frame.render_widget(Paragraph::new(right), r);
    frame.render_widget(Paragraph::new(tail), tail_area);
}

fn form_lines(params: &SlaveParams, sel: SlaveField, theme: &Theme) -> Vec<Line<'static>> {
    let field = |label: &str, value: String, selected: bool| {
        field_row(theme, label, LABEL_W, value, selected)
    };

    let id_val = edit_value(params.id.to_string(), sel == SlaveField::Id, false);
    let from_val = edit_value(params.from.to_string(), sel == SlaveField::From, false);
    let to_val = edit_value(params.to.to_string(), sel == SlaveField::To, false);
    let mode = if params.stop_at_first {
        "stop at first hit"
    } else {
        "full range"
    };
    let mode_val = edit_value(mode.to_string(), sel == SlaveField::Mode, true);

    let scan_sel = sel == SlaveField::Scan;
    let scan_label = if params.active {
        "Stop scan"
    } else {
        "Start scan"
    };
    let scan_text = if scan_sel {
        format!("{scan_label}  \u{2190} enter")
    } else {
        scan_label.to_string()
    };
    let scan_style = if scan_sel {
        theme.selected_style()
    } else if params.active {
        theme.warn_style()
    } else {
        theme.ok_style()
    };
    let scan_line = Line::from(Span::styled(
        format!("{}{scan_text}", marker(scan_sel)),
        scan_style,
    ));

    vec![
        Line::default(),
        field("Slave id", id_val, sel == SlaveField::Id),
        Line::default(),
        field("Scan from", from_val, sel == SlaveField::From),
        field("Scan to", to_val, sel == SlaveField::To),
        field("Mode", mode_val, sel == SlaveField::Mode),
        dim_line(
            theme,
            format!(
                "   Request      {} @ {} \u{d7}{}",
                params.register_type.name(),
                params.address,
                params.amount
            ),
        ),
        scan_line,
    ]
}

fn hit_lines(params: &SlaveParams, sel: SlaveField, theme: &Theme) -> Vec<Line<'static>> {
    let len = params.hits.len();
    let title = if len == 0 {
        " FOUND".to_string()
    } else {
        format!(" FOUND ({len})")
    };
    let mut lines = vec![
        Line::default(),
        Line::from(Span::styled(title, theme.header_style())),
    ];

    if len == 0 {
        let text = if params.active {
            "   scanning\u{2026}"
        } else {
            "   (no hits)"
        };
        lines.push(dim_line(theme, text));
        return lines;
    }

    // keep the selected hit visible otherwise follow the newest ones.
    let selected_hit = match sel {
        SlaveField::Hit(i) => Some(i),
        _ => None,
    };
    let max_top = len.saturating_sub(LIST_ROWS);
    let top = match selected_hit {
        Some(i) => i.saturating_sub(LIST_ROWS - 1).min(max_top),
        None => max_top,
    };
    let end = (top + LIST_ROWS).min(len);
    if top > 0 {
        lines.push(hints::more(theme, top, 0));
    }
    let value_width = (SIDE_W as usize).saturating_sub(PREFIX_W);
    for (i, hit) in params.hits.iter().enumerate().take(end).skip(top) {
        let selected = selected_hit == Some(i);
        let (text, style) = match &hit.result {
            Ok(values) => {
                let joined = values
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(" ");
                (joined, theme.line_style(selected))
            }
            Err(e) => (
                e.clone(),
                if selected {
                    theme.selected_style()
                } else {
                    theme.warn_style()
                },
            ),
        };
        let shown: String = text.chars().take(value_width).collect();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{:>3}  ", marker(selected), hit.slave_id),
                theme.dim_style(),
            ),
            Span::styled(shown, style),
        ]));
    }
    if end < len {
        lines.push(hints::more(theme, 0, len - end));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SlaveScanHit;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(params: &SlaveParams) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let (theme, kb) = (Theme::default(), Keybinds::default());
        terminal
            .draw(|frame| draw(frame, frame.area(), &theme, &kb, params))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).map_or(" ", |c| c.symbol()))
                    .collect()
            })
            .collect()
    }

    fn locate(rows: &[String], needle: &str) -> (usize, usize) {
        rows.iter()
            .enumerate()
            .find_map(|(y, row)| row.find(needle).map(|x| (x, y)))
            .unwrap_or_else(|| panic!("{needle:?} not rendered"))
    }

    fn hit(slave_id: u8, values: &[u16]) -> SlaveScanHit {
        SlaveScanHit {
            slave_id,
            result: Ok(values.to_vec()),
        }
    }

    #[test]
    fn hits_are_listed_beside_the_form() {
        let params = SlaveParams {
            hits: vec![hit(17, &[1, 2, 3])],
            ..SlaveParams::default()
        };
        let rows = render(&params);

        let (form_x, form_y) = locate(&rows, "Slave id");
        let (title_x, _) = locate(&rows, "FOUND (1)");
        let (hit_x, hit_y) = locate(&rows, " 17  1 2 3");
        assert!(
            title_x > form_x + LEFT_W as usize - LABEL_W,
            "the list starts right of the form"
        );
        assert!(hit_x > form_x, "hits sit in the right column");
        assert!(
            hit_y >= form_y && hit_y < form_y + 8,
            "hits share rows with the form instead of trailing it"
        );
    }

    #[test]
    fn the_list_follows_the_newest_hits_and_then_the_cursor() {
        let mut params = SlaveParams {
            hits: (1..=10).map(|id| hit(id, &[id as u16])).collect(),
            ..SlaveParams::default()
        };
        let rows = render(&params);
        locate(&rows, "  10  10");
        locate(&rows, "4 more");
        assert!(
            !rows.iter().any(|r| r.contains("   1  1")),
            "the oldest hit scrolled out"
        );

        params.selected = 5;
        let rows = render(&params);
        locate(&rows, ">   1  1");
        locate(&rows, "4 more");
    }

    #[test]
    fn an_empty_list_says_so() {
        let mut params = SlaveParams::default();
        locate(&render(&params), "(no hits)");
        params.active = true;
        locate(&render(&params), "scanning\u{2026}");
    }
}
