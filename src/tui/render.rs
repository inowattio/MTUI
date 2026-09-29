use crate::app::App;
use crate::state::{ScreenLayout, State};
use crate::tui::draw_state;
use crate::tui::hints;
use crate::tui::make_bottom_title::make_bottom_title;
use crate::tui::make_top_title::make_top_title;
use crate::tui::theme::status_span;
use chrono::Local;
use ratatui::Frame;
use ratatui::layout::Margin;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

pub fn render(app: &mut App, frame: &mut Frame) {
    let device = app.config.display_device();
    let theme = app.config.theme;

    let mode = make_top_title(&app.state);
    let key_hints = make_bottom_title(&theme, app);
    let mut clock_spans = Vec::new();
    if app.config.display.clock {
        let clock = Local::now().format("%H:%M:%S.%3f").to_string();
        clock_spans.push(Span::styled(format!("{clock} "), theme.accent_style()));
    }
    if app.config.display.ram
        && let Some(bytes) = app.ram_bytes
    {
        clock_spans.push(Span::styled(
            format!("{:.1}MiB ", bytes as f64 / (1024. * 1024.)),
            theme.dim_style(),
        ));
    }
    if app.config.display.frame_time {
        clock_spans.push(Span::styled(
            format!("{:.2?}ms ", app.last_frame.as_micros() as f64 / 1000.),
            theme.dim_style(),
        ));
    }
    let clock_line = Line::from(clock_spans);

    let left_top = match &app.state {
        State::Read(p) => draw_state::read::live_status(app, p, &theme),
        _ => vec![status_span(&app.connection, &theme)],
    };

    let mut mode_spans = Vec::new();
    let counter = match &app.state {
        State::Logs(l) => Some(draw_state::logs::counter(l, app)),
        State::Diff(d) => Some(draw_state::diff::summary(d)),
        _ => None,
    };
    if let Some(counter) = counter {
        mode_spans.push(Span::styled(format!(" {counter} |"), theme.dim_style()));
    }
    mode_spans.push(Span::styled(format!(" {mode}"), theme.base()));
    if !app.config.name.is_empty() {
        mode_spans.push(Span::styled(" - ", theme.dim_style()));
        mode_spans.push(Span::styled(app.config.name.clone(), theme.accent_style()));
    }

    let mut outer = Block::default()
        .title_top(Line::from(left_top))
        .title_top(Line::from(mode_spans).right_aligned())
        .title_bottom(clock_line)
        .title_bottom(key_hints.right_aligned())
        .border_style(Style::default().fg(theme.border))
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_type(BorderType::Rounded);
    if theme.background != Color::Reset {
        outer = outer.style(Style::default().bg(theme.background));
    }

    // The layout is published by the draw below, so this reads the previous
    // frame's value; it settles on the next redraw.
    let h_offset = match &app.state {
        State::Read(p) => Some(p.col_offset),
        State::Diff(d) => Some(d.h_offset),
        _ => None,
    };
    if let Some(offset) = h_offset {
        let max = app.layout.h_max_offset;
        if let Some(hint) = hints::hscroll(&theme, offset.min(max), max) {
            outer = outer.title_top(hint.centered());
        }
    }

    const PADDED_MIN_WIDTH: u16 = 91;
    const PADDED_MIN_HEIGHT: u16 = 19;
    let full = frame.area();
    let pad_h = app
        .config
        .display
        .padding
        .horizontal
        .min(full.width.saturating_sub(PADDED_MIN_WIDTH) / 2);
    let pad_v = app
        .config
        .display
        .padding
        .vertical
        .min(full.height.saturating_sub(PADDED_MIN_HEIGHT) / 2);
    let area = full.inner(Margin::new(pad_h, pad_v));
    if area != full {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.background)),
            full,
        );
    }
    let inner = outer.inner(area);
    app.viewport_width = inner.width;
    frame.render_widget(outer, area);

    let mut layout: ScreenLayout = app.layout;
    match &app.state {
        State::Read(p) => {
            draw_state::read::draw(p, app, frame, inner, &theme, &device, &mut layout);
        }
        State::Settings(s) => draw_state::settings::draw(s, app, frame, inner, &theme),
        State::Logs(l) => draw_state::logs::draw(l, frame, inner, &theme, &mut layout),
        State::Diff(d) => draw_state::diff::draw(d, app, frame, inner, &theme, &mut layout),
    }
    app.layout = layout;
}
