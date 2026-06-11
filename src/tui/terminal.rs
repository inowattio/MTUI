use crate::app::{App, AppResult};
use crate::event::{Event, EventHandler};
use crate::tui::render;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::Backend;
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
        crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableBracketedPaste)?;

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
        self.terminal.draw(|frame| render(app, frame))?;
        Ok(())
    }

    fn reset() -> AppResult<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableBracketedPaste)?;
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
