#[cfg(target_arch = "wasm32")]
mod web {
    use mtui::app::App;
    use mtui::config::Config;
    use mtui::constants::EVENT_HANDLER_TICKRATE;
    use mtui::handler::handle_key_events;
    use mtui::input;
    use mtui::tui::render;
    use mtui::{compat, logger};
    use ratatui::backend::Backend;
    use ratatui::layout::Rect;
    use ratatui::{TerminalOptions, Viewport};
    use ratzilla::{DomBackend, WebRenderer};
    use std::io;
    use std::rc::Rc;
    use tokio::sync::Mutex;

    fn convert_key(event: ratzilla::event::KeyEvent) -> Option<input::KeyEvent> {
        use ratzilla::event::KeyCode;
        let code = match event.code {
            KeyCode::Char(c) => input::KeyCode::Char(c),
            KeyCode::Esc => input::KeyCode::Esc,
            KeyCode::Enter => input::KeyCode::Enter,
            KeyCode::Backspace => input::KeyCode::Backspace,
            KeyCode::Tab => input::KeyCode::Tab,
            KeyCode::Up => input::KeyCode::Up,
            KeyCode::Down => input::KeyCode::Down,
            KeyCode::Left => input::KeyCode::Left,
            KeyCode::Right => input::KeyCode::Right,
            KeyCode::PageUp => input::KeyCode::PageUp,
            KeyCode::PageDown => input::KeyCode::PageDown,
            _ => return None,
        };
        Some(input::KeyEvent::new(code))
    }

    pub fn run() -> io::Result<()> {
        console_error_panic_hook::set_once();
        logger::init();

        // The Mock device connects without awaiting anything, so blocking
        // here is safe.
        let app = Rc::new(Mutex::new(futures::executor::block_on(App::boot(
            Config::default(),
            "browser demo (not persisted)".to_string(),
        ))));

        // The backend's `size()` assumes 10x20px cells, which rarely matches
        // the measured cell size and leaves part of the page blank; fix the
        // viewport to the measured DOM grid instead. Window resizes reload
        // the page (see index.html) so the grid is re-measured.
        let mut backend = DomBackend::new()?;
        let grid = backend.window_size()?.columns_rows;
        let mut terminal = ratatui::Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, grid.width, grid.height)),
            },
        )?;

        let key_app = app.clone();
        terminal
            .on_key_event(move |event| {
                let Some(key) = convert_key(event) else {
                    return;
                };
                let app = key_app.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let mut app = app.lock().await;
                    let _ = handle_key_events(key, &mut app).await;
                });
            })
            .map_err(|e| io::Error::other(e.to_string()))?;

        let mut last_tick = compat::Instant::now();
        terminal.draw_web(move |frame| {
            let Ok(mut app) = app.try_lock() else {
                return;
            };
            if last_tick.elapsed() >= EVENT_HANDLER_TICKRATE {
                last_tick = compat::Instant::now();
                futures::executor::block_on(app.tick());
            }
            render(&mut app, frame);
        });

        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
fn main() -> std::io::Result<()> {
    web::run()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("mtui-web targets the browser; build it with `trunk build`.");
}
