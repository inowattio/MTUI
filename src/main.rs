#[cfg(not(target_arch = "wasm32"))]
mod native {
    use clap::Parser;
    use mtui::app::{App, AppResult};
    use mtui::constants::EVENT_HANDLER_TICKRATE;
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

        /// Run as an API server only, with no TUI; logs are printed to stderr.
        #[arg(long)]
        headless: bool,
    }

    #[tokio::main]
    pub async fn run() -> AppResult<()> {
        let args = Args::parse();
        logger::init();

        if args.headless {
            return run_headless(args.config).await;
        }

        let mut app = App::new(args.config).await;

        let writer = io::BufWriter::with_capacity(256 * 1024, io::stderr());
        let backend = CrosstermBackend::new(writer);
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

    async fn run_headless(config: Option<String>) -> AppResult<()> {
        logger::enable_echo();
        let mut app = App::new(config).await;
        app.headless = true;

        app.config
            .port
            .inspect(|port| log::info!("Headless mode - API server on port {port}"))
            .ok_or_else(|| {
                anyhow::anyhow!("Headless mode requires an API port; set `port` in the config")
            })?;

        let mut ticker = tokio::time::interval(EVENT_HANDLER_TICKRATE);
        while app.running {
            tokio::select! {
                _ = ticker.tick() => app.tick().await,
                _ = tokio::signal::ctrl_c() => {
                    log::info!("Shutdown requested, stopping");
                    break;
                }
            }
        }

        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> mtui::app::AppResult<()> {
    native::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
