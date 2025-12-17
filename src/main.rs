pub mod app;
pub mod event;
pub mod tui;
pub mod handler;
mod modbus;
mod mock;
mod interpretator;
mod num_ops;
mod register;
mod config;
mod state;
mod constants;

use std::io;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use crate::app::{App, AppResult};
use crate::event::{Event, EventHandler};
use crate::handler::handle_key_events;
use crate::tui::Tui;

#[tokio::main]
async fn main() -> AppResult<()> {
    let mut app = App::new().await;

    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new();
    let mut tui = Tui::new(terminal, events)?;

    while app.running {
        tui.draw(&mut app)?;
        match tui.next_event().await? {
            Event::Tick => app.tick().await,
            Event::Key(key_event) => handle_key_events(key_event, &mut app).await?,
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
        }
    }

    tui.exit()?;
    Ok(())
}
