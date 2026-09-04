use crate::config::Keybinds;
use crate::interpretator::ascii_words;
use crate::state::{ScanState, SlaveField, SlaveParams, SlaveScanHit};
use crate::tui::draw_state::{dim_line, edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

const LEFT_W: u16 = 40;
const DIVIDER_W: u16 = 3;
const SIDE_W: u16 = 40;
const LABEL_W: usize = 13;
const LIST_ROWS: usize = 6;
const PREFIX_W: usize = 9;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    params: &SlaveParams,
    active_id: u8,
) {
    let sel = params.current_field();
    let left = form_lines(params, sel, theme);
    let right = params
        .scanned()
        .then(|| hit_lines(params, sel, active_id, theme));

    let primary = match sel {
        SlaveField::Id => "Set",
        SlaveField::Hit(_) => "Use id",
        SlaveField::Repr if params.ascii => "Values",
        SlaveField::Repr => "ASCII",
        SlaveField::Exceptions if params.show_exceptions => "Hide",
        SlaveField::Exceptions => "Include",
        _ if params.active() => "Stop scan",
        _ => "Start scan",
    };

    let (footer_w, footer) = if right.is_some() {
        let items = [
            Hint::pair(kb.move_up, kb.move_down, "Field"),
            Hint::key(kb.pause, "Toggle"),
            Hint::key(kb.action, primary),
            Hint::key(kb.exit, "Close"),
        ];
        (hints::width(&items), vec![hints::footer(theme, items)])
    } else {
        let nav = [
            Hint::pair(kb.move_up, kb.move_down, "Field"),
            Hint::key(kb.pause, "Toggle"),
        ];
        let actions = [Hint::key(kb.action, primary), Hint::key(kb.exit, "Close")];
        (
            hints::width(&nav).max(hints::width(&actions)),
            vec![hints::footer(theme, nav), hints::footer(theme, actions)],
        )
    };
    let footer_w = footer_w as u16;

    let mut tail: Vec<Line> = Vec::new();
    super::push_status(&mut tail, theme, params.status.as_ref());
    tail.push(Line::default());
    tail.extend(footer);

    let body_w = if right.is_some() {
        LEFT_W + DIVIDER_W + SIDE_W
    } else {
        LEFT_W
    };
    let width = (body_w + 2).max(footer_w);
    let body_rows = left.len().max(right.as_ref().map_or(0, Vec::len)) as u16;
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
    match right {
        Some(right) => {
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
        }
        None => frame.render_widget(Paragraph::new(left), body),
    }
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
    let repr = if params.ascii { "ASCII" } else { "values" };
    let repr_val = edit_value(repr.to_string(), sel == SlaveField::Repr, true);
    let exceptions = if params.show_exceptions {
        "included"
    } else {
        "hidden"
    };
    let exceptions_val = edit_value(exceptions.to_string(), sel == SlaveField::Exceptions, true);

    let scan_sel = sel == SlaveField::Scan;
    let scan_label = if params.active() {
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
    } else if params.active() {
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
        field("Show as", repr_val, sel == SlaveField::Repr),
        field("Exceptions", exceptions_val, sel == SlaveField::Exceptions),
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

fn title_line(params: &SlaveParams, theme: &Theme) -> Line<'static> {
    let (phase, style) = match params.scan {
        ScanState::Idle => (String::new(), theme.dim_style()),
        ScanState::Probing => (
            format!("PROBING {}\u{2026}", params.current),
            theme.warn_style(),
        ),
        ScanState::Done => ("DONE".to_string(), theme.ok_style()),
        ScanState::Stopped => ("STOPPED".to_string(), theme.warn_style()),
        ScanState::Failed => ("FAILED".to_string(), theme.err_style()),
    };
    let mut spans = vec![Span::styled(
        format!(" FOUND ({})", params.visible_hits().count()),
        theme.header_style(),
    )];
    if !phase.is_empty() {
        spans.push(Span::styled(" \u{b7} ", theme.dim_style()));
        spans.push(Span::styled(phase, style.add_modifier(Modifier::BOLD)));
    }
    Line::from(spans)
}

fn hit_lines(
    params: &SlaveParams,
    sel: SlaveField,
    active_id: u8,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let visible: Vec<(usize, &SlaveScanHit)> = params.visible_hits().collect();
    let len = visible.len();
    let mut lines = vec![Line::default(), title_line(params, theme)];

    if len == 0 {
        let text = if params.active() {
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
    let cursor = visible.iter().position(|&(i, _)| Some(i) == selected_hit);
    let max_top = len.saturating_sub(LIST_ROWS);
    let top = match cursor {
        Some(row) => row.saturating_sub(LIST_ROWS - 1).min(max_top),
        None => max_top,
    };
    let end = (top + LIST_ROWS).min(len);
    if top > 0 {
        lines.push(hints::more(theme, top, 0));
    }
    let value_width = (SIDE_W as usize).saturating_sub(PREFIX_W);
    for &(i, hit) in visible.iter().take(end).skip(top) {
        let selected = selected_hit == Some(i);
        let active = hit.slave_id == active_id;
        let text = match &hit.result {
            Ok(values) if params.ascii => format!("'{}'", ascii_words(values)),
            Ok(values) => values
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            Err(e) => e.clone(),
        };
        let style = if selected {
            theme.selected_style()
        } else if active {
            theme.accent_style()
        } else if hit.result.is_err() {
            theme.warn_style()
        } else {
            theme.base()
        };
        let radio = if active { "\u{25cf}" } else { "\u{25cb}" };
        let radio_style = if active { style } else { theme.dim_style() };
        let shown: String = text.chars().take(value_width).collect();
        lines.push(Line::from(vec![
            Span::styled(marker(selected), theme.dim_style()),
            Span::styled(format!("{radio} "), radio_style),
            Span::styled(format!("{:>3}  ", hit.slave_id), theme.dim_style()),
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
    use crate::state::StatusMessage;
    use ScanState::{Done, Failed, Probing, Stopped};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render(params: &SlaveParams) -> Vec<String> {
        render_with(params, params.id)
    }

    fn render_with(params: &SlaveParams, active_id: u8) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let (theme, kb) = (Theme::default(), Keybinds::default());
        terminal
            .draw(|frame| draw(frame, frame.area(), &theme, &kb, params, active_id))
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
            scan: Done,
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
            scan: Done,
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

        params.selected = 7;
        let rows = render(&params);
        locate(&rows, "> \u{25cb}   1  1");
        locate(&rows, "4 more");
    }

    fn popup_width(rows: &[String]) -> usize {
        rows.iter()
            .map(|row| row.trim().chars().count())
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn the_list_appears_once_a_scan_has_started() {
        let mut params = SlaveParams::default();
        let before = render(&params);
        assert!(
            !before.iter().any(|row| row.contains("FOUND")),
            "no list before a scan"
        );

        params.scan = Probing;
        params.current = 17;
        let scanning = render(&params);
        locate(&scanning, "FOUND (0) \u{b7} PROBING 17\u{2026}");
        locate(&scanning, "scanning\u{2026}");
        assert_eq!(
            popup_width(&scanning) - popup_width(&before),
            (DIVIDER_W + SIDE_W) as usize,
            "the popup widens by exactly the list column"
        );

        params.scan = Done;
        locate(&render(&params), "(no hits)");
    }

    #[test]
    fn the_hints_share_a_row_only_in_the_wide_popup() {
        let mut params = SlaveParams::default();
        let narrow = render(&params);
        let (_, mode_y) = locate(&narrow, "Toggle");
        let (_, close_y) = locate(&narrow, "Close");
        assert_eq!(close_y, mode_y + 1, "two hint rows in the narrow popup");

        params.scan = Done;
        let wide = render(&params);
        let (_, mode_y) = locate(&wide, "Toggle");
        let (_, close_y) = locate(&wide, "Close");
        assert_eq!(close_y, mode_y, "one hint row once the list is shown");
    }

    #[test]
    fn hit_data_can_be_shown_as_ascii() {
        let mut params = SlaveParams {
            scan: Done,
            hits: vec![hit(3, &[0x4D54, 0x5549])],
            ..SlaveParams::default()
        };
        let rows = render(&params);
        locate(&rows, "  3  19796 21833");
        let (_, y) = locate(&rows, "Show as");
        assert!(rows[y].contains("values"));

        params.ascii = true;
        let rows = render(&params);
        locate(&rows, "  3  'MTUI'");
        let (_, y) = locate(&rows, "Show as");
        assert!(rows[y].contains("ASCII"));
    }

    #[test]
    fn exception_hits_can_be_hidden_from_the_list() {
        let mut params = SlaveParams {
            scan: Done,
            hits: vec![
                hit(1, &[5]),
                SlaveScanHit {
                    slave_id: 2,
                    result: Err("IllegalDataAddress".into()),
                },
            ],
            ..SlaveParams::default()
        };
        let rows = render(&params);
        locate(&rows, "FOUND (2)");
        locate(&rows, "  2  IllegalDataAddress");
        let (_, y) = locate(&rows, "Exceptions");
        assert!(rows[y].contains("included"));

        params.show_exceptions = false;
        let rows = render(&params);
        locate(&rows, "FOUND (1)");
        locate(&rows, "  1  5");
        assert!(
            !rows.iter().any(|row| row.contains("IllegalDataAddress")),
            "hidden exceptions are not drawn"
        );
        let (_, y) = locate(&rows, "Exceptions");
        assert!(rows[y].contains("hidden"));
    }

    #[test]
    fn the_active_slave_id_is_marked_in_the_list() {
        let params = SlaveParams {
            scan: Done,
            id: 17,
            hits: vec![
                hit(1, &[5]),
                hit(17, &[1, 2, 3]),
                SlaveScanHit {
                    slave_id: 9,
                    result: Err("IllegalDataAddress".into()),
                },
            ],
            ..SlaveParams::default()
        };
        let rows = render_with(&params, 17);
        locate(&rows, "\u{25cf}  17  1 2 3");
        locate(&rows, "\u{25cb}   1  5");
        locate(&rows, "\u{25cb}   9  IllegalDataAddress");

        let rows = render_with(&params, 9);
        locate(&rows, "\u{25cb}  17  1 2 3");
        locate(&rows, "\u{25cf}   9  IllegalDataAddress");
    }

    #[test]
    fn the_scan_state_lives_in_the_list_title() {
        let mut params = SlaveParams {
            scan: Probing,
            current: 17,
            hits: vec![hit(3, &[7])],
            ..SlaveParams::default()
        };
        let rows = render(&params);
        let (_, title_y) = locate(&rows, "FOUND (1) \u{b7} PROBING 17\u{2026}");
        let (_, id_y) = locate(&rows, "Slave id");
        assert_eq!(title_y, id_y, "the title heads the list column");

        for (scan, phase) in [(Done, "DONE"), (Stopped, "STOPPED"), (Failed, "FAILED")] {
            params.scan = scan;
            locate(&render(&params), &format!("FOUND (1) \u{b7} {phase}"));
        }
    }

    #[test]
    fn errors_still_use_the_status_row() {
        let params = SlaveParams {
            scan: Done,
            status: Some(StatusMessage::err("No device connected")),
            ..SlaveParams::default()
        };
        let rows = render(&params);
        let (_, status_y) = locate(&rows, "No device connected");
        let (_, button_y) = locate(&rows, "Start scan");
        assert!(status_y > button_y, "below the form, like the narrow popup");
    }
}
