mod draw_state;
mod make_bottom_title;
mod make_top_title;
pub mod theme;

use crate::app::{App, AppResult};
use crate::event::{Event, EventHandler};
use crate::state::State;
use crate::tui::make_bottom_title::make_bottom_title;
use crate::tui::make_top_title::make_top_title;
use crate::tui::theme::{status_span, Theme};
use chrono::Local;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::Backend;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Terminal;
use std::io;
use std::panic;

#[derive(Debug)]
pub struct Tui<B: Backend> {
    terminal: Terminal<B>,
    events: EventHandler,
}

impl<B: Backend> Tui<B>
where
    <B as Backend>::Error: 'static,
{
    pub fn new(mut terminal: Terminal<B>, events: EventHandler) -> AppResult<Self> {
        terminal::enable_raw_mode()?;
        crossterm::execute!(
            io::stderr(),
            EnterAlternateScreen,
            EnableBracketedPaste
        )?;

        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            Self::reset().expect("failed to reset the terminal");
            panic_hook(panic);
        }));

        terminal.hide_cursor()?;
        terminal.clear()?;

        Ok(Self { terminal, events })
    }

    pub fn draw(&mut self, app: &mut App) -> AppResult<()> {
        let device = app.config.display_device();
        let theme = Theme::default();

        self.terminal.draw(|frame| {
            let mode = make_top_title(&app.state);
            let key_hints = make_bottom_title(&app.state);
            let clock = Local::now().format("%H:%M:%S:%3f").to_string();

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
                State::Jump(p) => draw_state::jump::draw(p, frame, inner, &theme, &device),
                State::Write(p) => draw_state::write::draw(p, frame, inner, &theme, &device),
                State::Label(p) => draw_state::label::draw(p, frame, inner, &theme, &device),
                State::Save(p) => draw_state::save::draw(p, frame, inner, &theme, &device),
                State::Dump(p) => draw_state::dump::draw(p, app, frame, inner, &theme, &device),
                State::Search(p) => draw_state::search::draw(p, app, frame, inner, &theme, &device),
                State::Help => draw_state::help::draw(frame, inner, &theme, &device),
            }
        })?;
        Ok(())
    }

    fn reset() -> AppResult<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(
            io::stderr(),
            LeaveAlternateScreen,
            DisableBracketedPaste
        )?;
        Ok(())
    }

    pub fn exit(&mut self) -> AppResult<()> {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }

    pub async fn next_event(&mut self) -> AppResult<Event> {
        self.events.next().await
    }
}
