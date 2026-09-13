use crate::input::KeyCode;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

const WORDMARK: [&str; 4] = [
    "▗▖  ▗▖▗▄▄▄▖▗▖ ▗▖▗▄▄▄▖",
    "▐▛▚▞▜▌  █  ▐▌ ▐▌  █  ",
    "▐▌  ▐▌  █  ▐▌ ▐▌  █  ",
    "▐▌  ▐▌  █  ▝▚▄▞▘▗▄█▄▖",
];

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme) {
    const LABEL_W: usize = 10;

    let field = |label: &str, value: Vec<Span<'static>>| -> Line<'static> {
        let mut spans = vec![Span::styled(
            format!(" {label:<LABEL_W$}"),
            theme.dim_style(),
        )];
        spans.extend(value);
        Line::from(spans)
    };
    let plain = |value: &str| vec![Span::styled(value.to_string(), theme.base())];

    let mut lines: Vec<Line> = vec![Line::default()];
    lines.extend(
        WORDMARK
            .iter()
            .map(|row| Line::from(Span::styled(*row, theme.accent_style())).centered()),
    );
    lines.push(Line::default());
    lines.push(
        Line::from(Span::styled(
            env!("CARGO_PKG_DESCRIPTION"),
            theme.dim_style(),
        ))
        .centered(),
    );
    lines.push(Line::default());
    lines.push(field(
        "Version",
        vec![
            Span::styled(env!("CARGO_PKG_VERSION"), theme.accent_style()),
            Span::styled(format!("  {}", env!("MTUI_GIT_HASH")), theme.dim_style()),
        ],
    ));
    lines.push(field("License", plain(env!("CARGO_PKG_LICENSE"))));
    lines.push(field("Homepage", plain(env!("CARGO_PKG_HOMEPAGE"))));
    lines.push(field("Source", plain(env!("CARGO_PKG_REPOSITORY"))));
    lines.push(Line::default());
    lines.push(
        Line::from(Span::styled(
            "Built with ratatui and tokio-modbus",
            theme.dim_style(),
        ))
        .centered(),
    );
    lines.push(Line::default());
    lines.push(hints::footer(theme, [Hint::key(KeyCode::Esc, "Close")]));

    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    let width = content_w + 4;

    super::render(frame, area, theme, "About", width, lines);
}
