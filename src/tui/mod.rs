mod make_bottom_title;
mod make_top_title;
mod draw_state;

use crate::app::{App, AppResult};
use crate::event::EventHandler;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::Backend;
use ratatui::Terminal;
use std::io;
use std::panic;
use ratatui::prelude::{Color, Style};
use ratatui::widgets::{Block, Borders, BorderType};
use crate::state::State;
use crate::tui::make_bottom_title::make_bottom_title;
use crate::tui::make_top_title::make_top_title;

#[derive(Debug)]
pub struct Tui<B: Backend> {
    terminal: Terminal<B>,
    pub events: EventHandler,
}

impl<B: Backend> Tui<B> where <B as Backend>::Error: 'static {
    pub fn new(terminal: Terminal<B>, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    pub fn init(&mut self) -> AppResult<()> {
        terminal::enable_raw_mode()?;
        crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;

        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            Self::reset().expect("failed to reset the terminal");
            panic_hook(panic);
        }));

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    pub fn draw(&mut self, app: &mut App) -> AppResult<()> {
        let device = app.config.display_device();
        let base_style = Style::default().fg(Color::White);

        self.terminal.draw(|frame| {
            let top_title = make_top_title(&app.state);
            let bottom_title = make_bottom_title(&app.state);

            let outer = Block::default()
                .title_top(top_title)
                .title_bottom(bottom_title)
                .style(Style::default().fg(Color::LightGreen))
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded);
            
            match &app.state {
                State::Read(p) => draw_state::read::draw(p, app, frame, outer, base_style, device),
                State::Dump(p) => draw_state::dump::draw(p, app, frame, outer, base_style, device),
                State::Jump(p) => draw_state::jump::draw(p, frame, outer, base_style, device),
                State::Write(p) => draw_state::write::draw(p, frame, outer, base_style, device),
                State::Help => draw_state::help::draw(frame, outer, base_style, device),
            }
        })?;
        Ok(())
    }

    fn reset() -> AppResult<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture)?;
        Ok(())
    }

    pub fn exit(&mut self) -> AppResult<()> {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}
