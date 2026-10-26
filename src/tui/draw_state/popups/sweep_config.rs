use crate::config::AddressMode;
use crate::input::KeyCode;
use crate::state::{SweepConfigParams, SweepField};
use crate::tui::draw_state::{action_line, edit_value, field_row};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    params: SweepConfigParams,
    running: bool,
    address_mode: AddressMode,
) {
    let sel = params.current_field();

    let field =
        |label: &str, value: String, selected: bool| field_row(theme, label, 13, value, selected);

    let shown = |address: u16| address_mode.display(address).to_string();
    let from_val = edit_value(shown(params.from), sel == SweepField::From, false);
    let to_val = edit_value(shown(params.to), sel == SweepField::To, false);
    let mode = if params.continuous { "loop" } else { "once" };
    let mode_val = edit_value(mode.to_string(), sel == SweepField::Mode, true);

    let action_label = if running { "Stop sweep" } else { "Start sweep" };
    let action_style = if running {
        theme.warn_style()
    } else {
        theme.ok_style()
    };
    let action_line = action_line(
        theme,
        action_label,
        sel == SweepField::Action,
        action_style,
        None,
    );

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
                Hint::pair(KeyCode::Up, KeyCode::Down, "Field"),
                Hint::key(KeyCode::Enter, "Action"),
                Hint::key(KeyCode::Esc, "Close"),
            ],
        ),
    ];

    super::render(frame, area, theme, "Sweep", 42, lines);
}
