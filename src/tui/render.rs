use crate::app::App;
use crate::state::State;
use crate::tui::draw_state;
use crate::tui::make_bottom_title::make_bottom_title;
use crate::tui::make_top_title::make_top_title;
use crate::tui::theme::status_span;
use chrono::Local;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

pub fn render(app: &mut App, frame: &mut Frame) {
    let device = app.config.display_device();
    let theme = app.config.theme;

    let mode = make_top_title(&app.state);
    let key_hints = make_bottom_title(&theme, &app.state, &app.config.keybinds);
    let mut clock_spans = Vec::new();
    if app.config.show_clock {
        let clock = Local::now().format("%H:%M:%S.%3f").to_string();
        clock_spans.push(Span::styled(format!(" {clock} "), theme.accent_style()));
    }
    if app.config.show_frame_time {
        if !app.config.show_clock {
            clock_spans.push(Span::styled(" ", theme.base()));
        }

        clock_spans.push(Span::styled(
            format!(">{:.2?}ms ", app.last_frame.as_micros() as f64 / 1000.),
            theme.dim_style(),
        ));
    }
    let clock_line = Line::from(clock_spans);

    let mut top_right = match &app.state {
        State::Read(p) => draw_state::read::live_status(app, p, &theme),
        _ => Vec::new(),
    };
    top_right.push(status_span(&app.connection, &theme));

    let outer = Block::default()
        .title_top(Line::styled(format!(" {mode} "), theme.base()))
        .title_top(Line::from(top_right).right_aligned())
        .title_bottom(clock_line)
        .title_bottom(key_hints.right_aligned())
        .style(Style::default().fg(theme.border))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let area = frame.area();
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    match &app.state {
        State::Read(p) => draw_state::read::draw(p, app, frame, inner, &theme, &device),
        State::Settings(s) => draw_state::settings::draw(s, app, frame, inner, &theme),
        State::Logs(l) => draw_state::logs::draw(l, app, frame, inner, &theme),
    }
}
