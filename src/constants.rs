use std::time::Duration;

pub const CONFIG_PATH: &str = "config.json";

pub const EVENT_HANDLER_TICKRATE: Duration = Duration::from_millis(100);

pub mod keybind {
    use crate::input::KeyCode;

    pub const EXIT: KeyCode = KeyCode::Esc;
    pub const PIN: KeyCode = KeyCode::Char('p');
    pub const DUMP: KeyCode = KeyCode::Char('d');
    pub const HELP: KeyCode = KeyCode::Char('h');
    pub const REFRESH: KeyCode = KeyCode::Char('r');
    pub const TOGGLE: KeyCode = KeyCode::Char('t');
    pub const WRITE: KeyCode = KeyCode::Char('w');
    pub const JUMP: KeyCode = KeyCode::Char('j');
    pub const LABEL: KeyCode = KeyCode::Char('l');
    pub const CUSTOM: KeyCode = KeyCode::Char('m');
    pub const COLUMNS: KeyCode = KeyCode::Char('c');
    pub const PAUSE: KeyCode = KeyCode::Char(' ');
    pub const WORD_ORDER: KeyCode = KeyCode::Char('o');
    pub const SLAVE: KeyCode = KeyCode::Char('i');
    pub const CYCLE_POSITION: KeyCode = KeyCode::Char('b');
    pub const INSPECT: KeyCode = KeyCode::Char('v');
    pub const GRAPH: KeyCode = KeyCode::Char('g');
    pub const DISCOVERY: KeyCode = KeyCode::Char('n');
    pub const SETTINGS: KeyCode = KeyCode::Char('s');
    pub const COPY_ADDRESS: KeyCode = KeyCode::Char('y');
    pub const LOGS: KeyCode = KeyCode::Char('L');
    pub const APP_LOGS: KeyCode = KeyCode::Char('k');
    pub const NEGATOR: KeyCode = KeyCode::Char('-');
    pub const SWEEP: KeyCode = KeyCode::Char('u');
    pub const SWEEP_CONFIG: KeyCode = KeyCode::Char('U');
    pub const SWITCH_VIEW: KeyCode = KeyCode::Tab;
    pub const ACTION: KeyCode = KeyCode::Enter;
    pub const MOVE_UP: KeyCode = KeyCode::Up;
    pub const MOVE_DOWN: KeyCode = KeyCode::Down;
    pub const PAGE_UP: KeyCode = KeyCode::PageUp;
    pub const PAGE_DOWN: KeyCode = KeyCode::PageDown;
}
