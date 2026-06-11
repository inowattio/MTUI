use crate::app::App;
use crate::state::State;
use crate::tui::draw_state;
use crate::tui::make_bottom_title::make_bottom_title;
use crate::tui::make_top_title::make_top_title;
use crate::tui::theme::{status_span, Theme};
use chrono::Local;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

pub fn render(app: &mut App, frame: &mut Frame) {
    let device = app.config.display_device();
    let theme = Theme::default();

    let mode = make_top_title(&app.state);
    let key_hints = make_bottom_title(&app.state);
    let clock = Local::now().format("%H:%M:%S.%3f").to_string();

    let outer = Block::default()
        .title_top(Line::styled(format!(" {mode} "), theme.accent_style()))
        .title_top(Line::from(status_span(&app.connection, &theme)).right_aligned())
        .title_bottom(Line::styled(format!(" {clock} "), theme.accent_style()))
        .title_bottom(Line::styled(key_hints, theme.dim_style()).right_aligned())
        .style(Style::default().fg(theme.border))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded);

    let area = frame.area();
    let inner = outer.inner(area);
    frame.render_widget(outer, area);

    match &app.state {
        State::Read(p) => draw_state::read::draw(p, app, frame, inner, &theme, &device),
        State::Discovery(d) => draw_state::discovery::draw(d, frame, inner, &theme),
        State::Settings(s) => draw_state::settings::draw(s, app, frame, inner, &theme),
        State::Logs(l) => draw_state::logs::draw(l, app, frame, inner, &theme),
    }
}
