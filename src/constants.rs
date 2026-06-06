use std::time::Duration;

pub const CONFIG_PATH: &str = "config.json";

// Drives both auto-refresh checks (gated by elapsed whole seconds, so the
// cadence is unaffected) and the loading-spinner / countdown animation, which
// needs sub-second redraws to look smooth.
pub const EVENT_HANDLER_TICKRATE: Duration = Duration::from_millis(150);

pub mod keybind {
    use crossterm::event::KeyCode;

    pub const EXIT: KeyCode = KeyCode::Char('q');
    pub const PIN: KeyCode = KeyCode::Char('p');
    pub const DUMP: KeyCode = KeyCode::Char('d');
    pub const HELP: KeyCode = KeyCode::Char('h');
    pub const REFRESH: KeyCode = KeyCode::Char('r');
    pub const TOGGLE: KeyCode = KeyCode::Char('t');
    pub const WRITE: KeyCode = KeyCode::Char('w');
    pub const JUMP: KeyCode = KeyCode::Char('j');
    pub const LABEL: KeyCode = KeyCode::Char('l');
    pub const NEGATOR: KeyCode = KeyCode::Char('-');
    pub const ACTION: KeyCode = KeyCode::Enter;
    pub const MOVE_UP: KeyCode = KeyCode::Up;
    pub const MOVE_DOWN: KeyCode = KeyCode::Down;
}
