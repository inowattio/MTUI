use crate::config::Keybinds;
use crate::state::{RawField, RawParams};
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
    params: &RawParams,
) {
    let field = params.current_field();
    let cursor = |f: RawField| if field == f { "_" } else { "" };

    let code_display = match params.code.trim().parse::<u16>() {
        Ok(value) if value <= u8::MAX as u16 => format!("{value} ({value:#04X})"),
        _ => params.code.clone(),
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(" Function code ", theme.dim_style()),
            Span::styled(code_display, theme.base()),
            Span::styled(cursor(RawField::Code), theme.accent_style()),
        ]),
        Line::from(vec![
            Span::styled(" Data (hex)    ", theme.dim_style()),
            Span::styled(params.data.clone(), theme.base()),
            Span::styled(cursor(RawField::Data), theme.accent_style()),
        ]),
        Line::default(),
    ];

    if let Some(response) = &params.response {
        lines.push(Line::from(vec![
            Span::styled(" Response      ", theme.dim_style()),
            Span::styled(response.clone(), theme.base()),
        ]));
    }

    if let Some(status) = &params.status {
        lines.push(Line::from(Span::styled(
            format!(" {}", status.text),
            theme.message_style(status.kind),
        )));
    }

    lines.push(Line::default());
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(kb.move_up, kb.move_down, "Field"),
            Hint::key(kb.action, "Send"),
            Hint::key(kb.exit, "Close"),
        ],
    ));

    super::render(frame, area, theme, "Raw function", 60, lines);
}
