#[cfg(not(target_arch = "wasm32"))]
mod native {
    use clap::Parser;
    use mtui::app::{App, AppResult};
    use mtui::event::{Event, EventHandler};
    use mtui::handler::{handle_key_events, handle_paste};
    use mtui::logger;
    use mtui::tui::Tui;
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use std::io;

    /// A TUI for Modbus reads and writes (RTU and TCP).
    #[derive(Parser)]
    #[command(version)]
    struct Args {
        /// Path to the configuration file [default: config.json]
        #[arg(long)]
        config: Option<String>,
    }

    #[tokio::main]
    pub async fn run() -> AppResult<()> {
        let args = Args::parse();
        logger::init();
        let mut app = App::new(args.config).await;

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
                Event::Resize(_, _) => {}
                Event::Paste(data) => handle_paste(data, &mut app),
            }
        }

        tui.exit()?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> mtui::app::AppResult<()> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
