use crate::config::Keybinds;
use crate::state::{SlaveField, SlaveParams};
use crate::tui::draw_state::{dim_line, edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

const WIDTH: u16 = 46;
const VISIBLE_HITS: usize = 4;
const PREFIX_W: usize = 7;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    params: &SlaveParams,
) {
    let sel = params.current_field();

    let field =
        |label: &str, value: String, selected: bool| field_row(theme, label, 13, value, selected);

    let id = params.id.to_string();
    let id_val = edit_value(id, sel == SlaveField::Id, false);
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

    let mut lines = vec![
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
    ];

    if !params.hits.is_empty() {
        lines.push(Line::default());
        let value_width = (WIDTH.saturating_sub(2) as usize).saturating_sub(PREFIX_W);
        // keep the selected hit visible, scroll the window to include it.
        let selected_hit = match sel {
            SlaveField::Hit(i) => Some(i),
            _ => None,
        };
        let max_top = params.hits.len().saturating_sub(VISIBLE_HITS);
        let top = match selected_hit {
            Some(i) => i.saturating_sub(VISIBLE_HITS - 1).min(max_top),
            None => max_top,
        };
        let end = (top + VISIBLE_HITS).min(params.hits.len());
        if top > 0 {
            lines.push(hints::more(theme, top, 0));
        }
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
        if end < params.hits.len() {
            lines.push(hints::more(theme, 0, params.hits.len() - end));
        }
    }

    super::push_status(&mut lines, theme, params.status.as_ref());

    let primary = match sel {
        SlaveField::Id => "Set",
        SlaveField::Hit(_) => "Use id",
        _ if params.active => "Stop scan",
        _ => "Start scan",
    };
    lines.push(Line::default());
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(kb.move_up, kb.move_down, "Field"),
            Hint::key(kb.pause, "Toggle mode"),
        ],
    ));
    lines.push(hints::footer(
        theme,
        [Hint::key(kb.action, primary), Hint::key(kb.exit, "Close")],
    ));

    super::render(frame, area, theme, "Slave", WIDTH, lines);
}
