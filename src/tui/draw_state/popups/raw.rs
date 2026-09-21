use crate::input::KeyCode;
use crate::state::{RawField, RawParams};
use crate::tui::draw_state::{edit_value, field_row};
use crate::tui::hints::Hint;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

const LABEL_W: usize = 13;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, params: &RawParams) {
    let sel = params.field;
    let field = |label: &str, value: String, selected: bool| {
        field_row(theme, label, LABEL_W, value, selected)
    };

    let code = match params.code.trim().parse::<u16>() {
        Ok(value) if value <= u8::MAX as u16 => format!("{value} ({value:#04X})"),
        _ => params.code.clone(),
    };
    let code_val = edit_value(code, sel == RawField::Code, false);
    let data_val = edit_value(params.data.clone(), sel == RawField::Data, false);

    let mut lines = vec![
        Line::default(),
        field("Function code", code_val, sel == RawField::Code),
        field("Data (hex)", data_val, sel == RawField::Data),
    ];

    if let Some(response) = &params.response {
        lines.push(Line::default());
        lines.push(field("Response", response.clone(), false));
    }
    super::push_status(&mut lines, theme, params.status.as_ref());

    super::push_footer(
        &mut lines,
        theme,
        [
            Hint::pair(KeyCode::Up, KeyCode::Down, "Field"),
            Hint::key(KeyCode::Enter, "Send"),
            Hint::key(KeyCode::Esc, "Close"),
        ],
    );

    super::render(frame, area, theme, "Raw request", 60, lines);
}
