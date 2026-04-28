pub mod app;
mod config;
mod constants;
pub mod event;
pub mod handler;
mod interpretator;
mod mock;
mod modbus;
mod num_ops;
mod register;
mod state;
pub mod tui;

use crate::app::{App, AppResult};
use crate::event::{Event, EventHandler};
use crate::handler::handle_key_events;
use crate::tui::Tui;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io;

#[tokio::main]
async fn main() -> AppResult<()> {
    let mut app = App::new().await;

    let backend = CrosstermBackend::new(io::stderr());
    let terminal = Terminal::new(backend)?;
    let events = EventHandler::new();
    let mut tui = Tui::new(terminal, events)?;

    while app.running {
        app.complete_background_task().await;
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
