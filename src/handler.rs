use crate::app::{App, AppResult};
use crossterm::event::{KeyCode, KeyEvent};

/// Handles the key events and updates the state of [`App`].
pub fn handle_key_events(key_event: KeyEvent, app: &mut App) -> AppResult<()> {
    match key_event.code {
        // Exit application on `ESC` or `q`
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Char('r') => {
            app.refresh();
        }
        KeyCode::Char('t') => {
            app.toggle_type();
        }
        KeyCode::Up => {
            app.up();
        }
        KeyCode::Down => {
            app.down();
        }
        // Other handlers you could add here.
        _ => {}
    }
    Ok(())
}
