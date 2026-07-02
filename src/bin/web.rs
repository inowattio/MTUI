#[cfg(target_arch = "wasm32")]
mod web {
    use mtui::app::App;
    use mtui::config::Config;
    use mtui::constants::EVENT_HANDLER_TICKRATE;
    use mtui::handler::handle_key_events;
    use mtui::input;
    use mtui::tui::render;
    use mtui::{compat, logger};
    use ratatui::Terminal;
    use ratzilla::backend::webgl2::WebGl2BackendOptions;
    use ratzilla::web_sys;
    use ratzilla::web_sys::wasm_bindgen::prelude::Closure;
    use ratzilla::web_sys::wasm_bindgen::JsCast;
    use ratzilla::{FontAtlasConfig, WebGl2Backend};
    use std::cell::{Cell, RefCell};
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

    /// The WebGL2 backend (beamterm) renders the grid on the GPU from a
    /// glyph atlas — the cheapest option per frame by far. The DOM backend
    /// turned every cell into a styled `<span>`, and full-area updates (the
    /// graph view, held cursor movement) forced the browser through style
    /// recalc and layout over tens of thousands of nodes.
    fn build_terminal() -> io::Result<Terminal<WebGl2Backend>> {
        let options = WebGl2BackendOptions::new()
            // Rasterize glyphs on demand: the default static atlas blanks
            // out anything it doesn't carry, and the UI needs braille (the
            // graph), box drawing and assorted symbols.
            .font_atlas_config(FontAtlasConfig::dynamic(&["monospace"], 16.0))
            // index.html's CSS sizes the canvas to fill the body.
            .disable_auto_css_resize();
        let backend = WebGl2Backend::new_with_options(options)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Terminal::new(backend)
    }

    fn inner_size(window: &web_sys::Window) -> (i32, i32) {
        let px = |v: Result<web_sys::wasm_bindgen::JsValue, _>| {
            v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as i32
        };
        (px(window.inner_width()), px(window.inner_height()))
    }

    /// Drives rendering from `requestAnimationFrame`, like ratzilla's own
    /// `draw_web`, but drawing only when something changed.
    ///
    /// Resizes need no special handling beyond a prompt redraw: the backend
    /// re-fits its grid to the canvas CSS size during `flush`, and ratatui's
    /// autoresize follows on the draw after that.
    fn start_render_loop(
        window: web_sys::Window,
        app: Rc<Mutex<App>>,
        dirty: Rc<Cell<bool>>,
    ) -> io::Result<()> {
        let mut terminal = build_terminal()?;
        let mut last_seen = inner_size(&window);
        let mut last_tick = compat::Instant::now();

        type FrameCallback = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;
        let callback: FrameCallback = Rc::new(RefCell::new(None));
        let schedule: Rc<dyn Fn()> = Rc::new({
            let callback = callback.clone();
            let window = window.clone();
            move || {
                if let Some(callback) = callback.borrow().as_ref() {
                    let _ = window.request_animation_frame(callback.as_ref().unchecked_ref());
                }
            }
        });

        *callback.borrow_mut() = Some(Closure::new({
            let schedule = schedule.clone();
            move || {
                schedule();

                // Acquire the app before consuming any redraw flags so a
                // skipped frame (key handler holding the lock) loses nothing.
                let Ok(mut app) = app.try_lock() else {
                    return;
                };

                // Render only when something changed: rAF runs at the
                // monitor's refresh rate, and drawing the full frame every
                // vsync burns a core for no visible benefit.
                let mut must_draw = dirty.replace(false);

                let size = inner_size(&window);
                if size != last_seen {
                    last_seen = size;
                    // Draw next frame too: this draw's flush re-fits the
                    // grid, the next one renders at the final dimensions.
                    dirty.set(true);
                    must_draw = true;
                }

                if last_tick.elapsed() >= EVENT_HANDLER_TICKRATE {
                    last_tick = compat::Instant::now();
                    futures::executor::block_on(app.tick());
                    must_draw = true;
                }

                if must_draw {
                    let started = compat::Instant::now();
                    let _ = terminal.draw(|frame| render(&mut app, frame));
                    app.last_frame = started.elapsed();
                }
            }
        }));
        schedule();
        Ok(())
    }

    pub fn run() -> io::Result<()> {
        console_error_panic_hook::set_once();
        logger::init();

        // The Mock device connects without awaiting anything, so blocking
        // here is safe.
        let app = Rc::new(Mutex::new(futures::executor::block_on(App::boot(
            Config::demo(),
            "browser demo (not persisted)".to_string(),
        ))));

        let window = web_sys::window().ok_or_else(|| io::Error::other("no window"))?;

        // Listen on the window rather than `terminal.on_key_event`, which
        // binds to the grid element: the grid only receives keys while
        // focused, and Tab's default action moves that focus to the browser
        // UI. Preventing the default keeps handled keys (Tab included) in
        // the app; browser shortcuts (Ctrl/Alt/Meta) are left alone.
        let dirty = Rc::new(Cell::new(false));

        let key_app = app.clone();
        let key_dirty = dirty.clone();
        let on_key = Closure::<dyn FnMut(_)>::new(move |event: web_sys::KeyboardEvent| {
            if event.ctrl_key() || event.alt_key() || event.meta_key() {
                return;
            }
            let Some(key) = convert_key(event.clone().into()) else {
                return;
            };
            event.prevent_default();
            let app = key_app.clone();
            let dirty = key_dirty.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut app = app.lock().await;
                let _ = handle_key_events(key, &mut app).await;
                dirty.set(true);
            });
        });
        window
            .add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref())
            .map_err(|e| io::Error::other(format!("{e:?}")))?;
        on_key.forget();

        start_render_loop(window, app, dirty)
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
