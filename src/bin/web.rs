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
    use ratzilla::{DomBackend, FontAtlasConfig, WebGl2Backend};
    use std::cell::{Cell, RefCell};
    use std::io;
    use std::rc::Rc;
    use tokio::sync::Mutex;

    /// Both backends mount into this element so a rebuild (context loss) or
    /// a fallback can tear down the old renderer by clearing its children.
    const TERMINAL_CONTAINER_ID: &str = "mtui-term";
    const ERROR_OVERLAY_ID: &str = "mtui-error";

    /// Consecutive `terminal.draw` failures tolerated before the renderer is
    /// torn down and rebuilt. Draws happen roughly every tick (100ms), so
    /// this is ~2s of a persistently broken backend.
    const DRAW_FAILURES_BEFORE_REBUILD: u32 = 20;
    /// Frames between rebuild attempts while the renderer is down (~1s).
    const FRAMES_BETWEEN_REBUILDS: u32 = 60;

    fn convert_key(event: ratzilla::event::KeyEvent) -> Option<input::KeyEvent> {
        use ratzilla::event::KeyCode;
        let code = match event.code {
            KeyCode::Char(c) => input::KeyCode::Char(c),
            KeyCode::Esc => input::KeyCode::Esc,
            KeyCode::Enter => input::KeyCode::Enter,
            KeyCode::Backspace => input::KeyCode::Backspace,
            KeyCode::Delete => input::KeyCode::Delete,
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

    fn console_error(msg: &str) {
        web_sys::console::error_1(&msg.into());
    }

    fn document() -> io::Result<web_sys::Document> {
        web_sys::window()
            .and_then(|w| w.document())
            .ok_or_else(|| io::Error::other("no document"))
    }

    /// A blank page with the error buried in the console is undebuggable
    /// for visitors; render failures into the DOM where they can be seen.
    pub fn show_error_overlay(msg: &str) {
        let Ok(document) = document() else { return };
        let overlay = match document.get_element_by_id(ERROR_OVERLAY_ID) {
            Some(existing) => existing,
            None => {
                let Ok(element) = document.create_element("pre") else {
                    return;
                };
                element.set_id(ERROR_OVERLAY_ID);
                let _ = element.set_attribute(
                    "style",
                    "position:fixed;top:0;left:0;z-index:10;margin:0;padding:8px;\
                     max-width:100vw;white-space:pre-wrap;color:#ff5252;\
                     background:#000c;font:14px monospace",
                );
                if let Some(body) = document.body() {
                    let _ = body.append_child(&element);
                }
                element
            }
        };
        overlay.set_text_content(Some(msg));
        console_error(msg);
    }

    fn hide_error_overlay() {
        if let Ok(document) = document() {
            if let Some(overlay) = document.get_element_by_id(ERROR_OVERLAY_ID) {
                overlay.remove();
            }
        }
    }

    /// The WebGL2 backend (beamterm) renders the grid on the GPU from a
    /// glyph atlas — the cheapest option per frame by far. The DOM backend
    /// turned every cell into a styled `<span>`, and full-area updates (the
    /// graph view, held cursor movement) forced the browser through style
    /// recalc and layout over tens of thousands of nodes. It survives as the
    /// fallback when WebGL2 is unavailable (hardware acceleration disabled,
    /// blocklisted drivers): slow beats a blank page.
    enum WebTerminal {
        Gl(Box<Terminal<WebGl2Backend>>),
        Dom(Box<Terminal<DomBackend>>),
    }

    impl WebTerminal {
        fn draw(&mut self, app: &mut App) -> io::Result<()> {
            match self {
                WebTerminal::Gl(terminal) => terminal.draw(|frame| render(app, frame)),
                WebTerminal::Dom(terminal) => terminal.draw(|frame| render(app, frame)),
            }
            .map(|_| ())
        }
    }

    fn build_gl_terminal() -> io::Result<Terminal<WebGl2Backend>> {
        let options = WebGl2BackendOptions::new()
            .grid_id(TERMINAL_CONTAINER_ID)
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

    fn build_dom_terminal() -> io::Result<Terminal<DomBackend>> {
        let backend = DomBackend::new_by_id(TERMINAL_CONTAINER_ID)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Terminal::new(backend)
    }

    /// Ensures the shared mount point for both backends exists.
    fn ensure_terminal_container() -> io::Result<()> {
        let document = document()?;
        if document.get_element_by_id(TERMINAL_CONTAINER_ID).is_none() {
            let container = document
                .create_element("div")
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
            container.set_id(TERMINAL_CONTAINER_ID);
            let _ = container.set_attribute("style", "width:100%;height:100%");
            document
                .body()
                .ok_or_else(|| io::Error::other("no body"))?
                .append_child(&container)
                .map_err(|e| io::Error::other(format!("{e:?}")))?;
        }
        Ok(())
    }

    fn clear_terminal_container() {
        if let Ok(document) = document() {
            if let Some(container) = document.get_element_by_id(TERMINAL_CONTAINER_ID) {
                container.set_inner_html("");
            }
        }
    }

    /// Browsers drop WebGL contexts under GPU pressure, driver resets and
    /// suspend/resume — and GL calls on a lost context silently no-op, so
    /// beamterm keeps "drawing" a canvas that stays black. The event on the
    /// canvas is the only reliable signal; it flags the render loop to
    /// rebuild the backend from scratch.
    fn watch_context_loss(gl_lost: &Rc<Cell<bool>>) {
        let Ok(document) = document() else { return };
        let Some(container) = document.get_element_by_id(TERMINAL_CONTAINER_ID) else {
            return;
        };
        let Ok(Some(canvas)) = container.query_selector("canvas") else {
            return;
        };
        let gl_lost = gl_lost.clone();
        let on_lost = Closure::<dyn FnMut()>::new(move || {
            console_error("mtui: WebGL context lost; rebuilding the renderer");
            gl_lost.set(true);
        });
        let _ = canvas
            .add_event_listener_with_callback("webglcontextlost", on_lost.as_ref().unchecked_ref());
        on_lost.forget();
    }

    /// Tears down whatever renderer occupied the container and starts a
    /// fresh one: WebGL2 first, DOM as the fallback.
    fn build_web_terminal(gl_lost: &Rc<Cell<bool>>) -> io::Result<WebTerminal> {
        ensure_terminal_container()?;
        clear_terminal_container();
        match build_gl_terminal() {
            Ok(terminal) => {
                watch_context_loss(gl_lost);
                Ok(WebTerminal::Gl(Box::new(terminal)))
            }
            Err(gl_error) => {
                console_error(&format!(
                    "mtui: WebGL2 renderer unavailable ({gl_error}); \
                     falling back to the slower DOM renderer"
                ));
                clear_terminal_container();
                build_dom_terminal().map(|t| WebTerminal::Dom(Box::new(t)))
            }
        }
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
        let gl_lost = Rc::new(Cell::new(false));
        let mut terminal = build_web_terminal(&gl_lost)?;
        let mut last_seen = inner_size(&window);
        let mut last_tick = compat::Instant::now();
        let mut draw_failures: u32 = 0;
        let mut frames_since_rebuild: u32 = 0;

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

                // A dead renderer redraws nothing no matter what the app
                // does; rebuild it (throttled) before minding the app state.
                if gl_lost.get() {
                    frames_since_rebuild += 1;
                    if frames_since_rebuild < FRAMES_BETWEEN_REBUILDS {
                        return;
                    }
                    frames_since_rebuild = 0;
                    match build_web_terminal(&gl_lost) {
                        Ok(rebuilt) => {
                            terminal = rebuilt;
                            gl_lost.set(false);
                            draw_failures = 0;
                            hide_error_overlay();
                            dirty.set(true);
                        }
                        Err(e) => {
                            show_error_overlay(&format!(
                                "mtui: the renderer died and could not be rebuilt: {e}"
                            ));
                            return;
                        }
                    }
                }

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
                    match terminal.draw(&mut app) {
                        Ok(()) => draw_failures = 0,
                        Err(e) => {
                            draw_failures += 1;
                            if draw_failures == 1 {
                                console_error(&format!("mtui: terminal draw failed: {e}"));
                            }
                            // Persistent failure means the backend is beyond
                            // saving; route it through the rebuild path.
                            if draw_failures >= DRAW_FAILURES_BEFORE_REBUILD {
                                gl_lost.set(true);
                                draw_failures = 0;
                            }
                        }
                    }
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
        let dirty = Rc::new(Cell::new(true));

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
fn main() {
    if let Err(e) = web::run() {
        web::show_error_overlay(&format!("mtui: the browser demo failed to start: {e}"));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("mtui-web targets the browser; build it with `trunk build`.");
}
