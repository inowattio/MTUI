use crate::app::App;
use crate::input::KeyCode;
use crate::modbus::DeviceIdAccess;
use crate::state::DeviceIdParams;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::Frame;

// " 0x00  " before each value.
const PREFIX_W: usize = 7;

pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    app: &App,
    params: &DeviceIdParams,
) {
    let kb = &app.config.keybinds;
    let footer = [
        Hint::key(kb.switch_view, "Access"),
        Hint::pair(KeyCode::Left, KeyCode::Right, "Scroll"),
        Hint::key(kb.refresh, "Reread"),
        Hint::key(kb.exit, "Close"),
    ];

    let value_max = params
        .objects
        .iter()
        .map(|(_, value)| value.chars().count())
        .max()
        .unwrap_or(0);
    let width = hints::min_width(46, &footer);
    let value_width = (width.saturating_sub(2) as usize).saturating_sub(PREFIX_W);

    let max_offset = value_max.saturating_sub(value_width) as u16;
    app.h_max_offset.set(max_offset);
    let offset = params.h_offset.min(max_offset) as usize;

    let access_index = DeviceIdAccess::ALL
        .iter()
        .position(|&a| a == params.access)
        .unwrap_or(0);
    let mut tabs = vec![Span::raw(" ")];
    tabs.extend(theme.tab_spans(DeviceIdAccess::ALL.map(DeviceIdAccess::label), access_index));
    if offset > 0 {
        tabs.push(Span::styled(
            format!("   \u{25c2} +{offset}"),
            theme.dim_style(),
        ));
    }

    let mut lines: Vec<Line> = vec![Line::from(tabs), Line::default()];

    if params.objects.is_empty() {
        let text = if params.loading {
            " reading\u{2026}"
        } else {
            " no objects"
        };
        lines.push(Line::from(Span::styled(text, theme.dim_style())));
    } else {
        for &(id, ref value) in &params.objects {
            let shown: String = value.chars().skip(offset).collect();
            lines.push(Line::from(vec![
                Span::styled(format!(" {id:#04X}  "), theme.dim_style()),
                Span::styled(shown, theme.base()),
            ]));
        }
    }

    super::push_status(&mut lines, theme, params.status.as_ref());

    super::push_footer(&mut lines, theme, footer);

    super::render(frame, area, theme, "Device identification", width, lines);
}
