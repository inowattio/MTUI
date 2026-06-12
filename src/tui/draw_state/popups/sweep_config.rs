use crate::config::Keybinds;
use crate::state::{SweepConfigParams, SweepField};
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
) {
    let sel = params.current_field();

    let field = |label: &str, value: String, selected: bool| -> Line<'static> {
        let marker = if selected { "> " } else { "  " };
        let style = if selected {
            theme.selected_style()
        } else {
            theme.base()
        };
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
        format!("\u{2039} {mode} \u{203a}")
    } else {
        mode.to_string()
    };

    let lines = vec![
        Line::default(),
        field("From address", from_val, sel == SweepField::From),
        field("To address", to_val, sel == SweepField::To),
        field("Mode", mode_val, sel == SweepField::Mode),
        Line::default(),
        Line::from(Span::styled(
            format!(
                " {}/{} field \u{b7} {} toggle mode",
                kb.move_up, kb.move_down, kb.pause
            ),
            theme.dim_style(),
        )),
        Line::from(Span::styled(
            format!(" {} apply \u{b7} {} cancel", kb.action, kb.exit),
            theme.dim_style(),
        )),
    ];

    super::render(frame, area, theme, "Sweep", 46, lines);
}
