use crate::config::Keybinds;
use crate::input::KeyCode;
use crate::state::DeviceIdParams;
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
    params: &DeviceIdParams,
) {
    const NAME: usize = 18;
    const VALUE: usize = 40;

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(" Access ", theme.dim_style()),
            Span::styled(params.access.label(), theme.accent_style()),
        ]),
        Line::default(),
    ];

    if params.objects.is_empty() {
        let text = if params.loading {
            " reading\u{2026}"
        } else {
            " no objects"
        };
        lines.push(Line::from(Span::styled(text, theme.dim_style())));
    } else {
        for (id, value) in &params.objects {
            let value: String = value.chars().take(VALUE).collect();
            lines.push(Line::from(vec![
                Span::styled(format!(" {id:#04X}  "), theme.dim_style()),
                Span::styled(format!("{value:<VALUE$}"), theme.base()),
            ]));
        }
    }

    super::push_status(&mut lines, theme, params.status.as_ref());

    lines.push(Line::default());
    lines.push(hints::footer(
        theme,
        [
            Hint::pair(KeyCode::Left, KeyCode::Right, "Access"),
            Hint::key(kb.refresh, "Reread"),
            Hint::key(kb.exit, "Close"),
        ],
    ));

    let width = (7 + NAME + 1 + VALUE + 2) as u16;
    super::render(frame, area, theme, "Device identification", width, lines);
}
