use crate::app::{App, AppResult};
use crate::event::{Event, EventHandler};
use crate::handler::{handle_key_events, handle_paste};
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
    <B as Backend>::Error: std::error::Error + Send + Sync + 'static,
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
        let started = crate::compat::Instant::now();
        self.terminal.draw(|frame| render(app, frame))?;
        app.last_frame = started.elapsed();
        Ok(())
    }

    pub async fn process_events(&mut self, app: &mut App) -> AppResult<()> {
        for event in self.events.nexts().await? {
            match event {
                Event::Tick => app.tick().await,
                Event::Key(key_event) => handle_key_events(key_event, app).await?,
                Event::Resize(_, _) => {}
                Event::Paste(data) => handle_paste(data, app),
            }
        }

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
}
