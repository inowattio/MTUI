use crate::config::Keybinds;
use crate::state::{SweepConfigParams, SweepField};
use crate::tui::draw_state::{cyclable, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    kb: &Keybinds,
    params: &SweepConfigParams,
    running: bool,
) {
    let sel = params.current_field();

    let field = |label: &str, value: String, selected: bool| -> Line<'static> {
        let marker = marker(selected);
        let style = theme.line_style(selected);
        Line::from(vec![
            Span::styled(format!("{marker}{label:<14}"), theme.dim_style()),
            Span::styled(value, style),
        ])
    };

    let from_val = if sel == SweepField::From {
        format!("{}_", params.from)
    } else {
        params.from.to_string()
    };
    let to_val = if sel == SweepField::To {
        format!("{}_", params.to)
    } else {
        params.to.to_string()
    };
    let mode = if params.continuous { "loop" } else { "once" };
    let mode_val = if sel == SweepField::Mode {
        cyclable(mode)
    } else {
        mode.to_string()
    };

    let action_sel = sel == SweepField::Action;
    let action_label = if running { "Stop sweep" } else { "Start sweep" };
    let action_text = if action_sel {
        format!("{action_label}  \u{2190} enter")
    } else {
        action_label.to_string()
    };
    let action_style = if action_sel {
        theme.selected_style()
    } else if running {
        theme.warn_style()
    } else {
        theme.ok_style()
    };
    let action_line = Line::from(Span::styled(
        format!("{}{action_text}", marker(action_sel)),
        action_style,
    ));

    let lines = vec![
        Line::default(),
        field("From address", from_val, sel == SweepField::From),
        field("To address", to_val, sel == SweepField::To),
        field("Mode", mode_val, sel == SweepField::Mode),
        Line::default(),
        action_line,
        Line::default(),
        hints::footer(
            theme,
            [
                Hint::pair(kb.move_up, kb.move_down, "Field"),
                Hint::key(kb.pause, "Toggle mode"),
            ],
        ),
        hints::footer(
            theme,
            [
                Hint::key(kb.action, "Start/Stop"),
                Hint::key(kb.exit, "Close"),
            ],
        ),
    ];

    super::render(frame, area, theme, "Sweep", 46, lines);
}
