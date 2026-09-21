use std::time::Duration;

pub const CONFIG_PATH: &str = "config.json";

pub const NO_VALUE: &str = "-";

pub const UNINTERPRETABLE: &str = "?";

pub const ELLIPSIS: &str = "...";

pub const EVENT_HANDLER_TICKRATE: Duration = Duration::from_millis(100);

pub const SEARCH_POPUP_MAX_HEIGHT_PERCENT: u16 = 80;
pub const SEARCH_POPUP_MAX_WIDTH_PERCENT: u16 = 50;

pub mod keybind {
    use crate::input::KeyCode;

    pub const PIN: KeyCode = KeyCode::Char('p');
    pub const DUMP: KeyCode = KeyCode::Char('d');
    pub const HELP: KeyCode = KeyCode::Char('h');
    pub const ABOUT: KeyCode = KeyCode::Char('a');
    pub const REFRESH: KeyCode = KeyCode::Char('r');
    pub const REGISTER_TYPE: KeyCode = KeyCode::Char('t');
    pub const WRITE: KeyCode = KeyCode::Char('w');
    pub const GO_TO: KeyCode = KeyCode::Char('j');
    pub const LABEL: KeyCode = KeyCode::Char('l');
    pub const CUSTOM_RULE: KeyCode = KeyCode::Char('m');
    pub const COLUMNS: KeyCode = KeyCode::Char('c');
    pub const PAUSE: KeyCode = KeyCode::Char(' ');
    pub const WORD_ORDER: KeyCode = KeyCode::Char('o');
    pub const UNIT_ID: KeyCode = KeyCode::Char('i');
    pub const INSPECT: KeyCode = KeyCode::Char('v');
    pub const DEVICE_ID: KeyCode = KeyCode::Char('D');
    pub const RAW_REQUEST: KeyCode = KeyCode::Char('f');
    pub const GRAPH: KeyCode = KeyCode::Char('g');
    pub const DEVICE: KeyCode = KeyCode::Char('n');
    pub const SETTINGS: KeyCode = KeyCode::Char('s');
    pub const COPY_COLUMN: KeyCode = KeyCode::Char('y');
    pub const WRITE_LOGS: KeyCode = KeyCode::Char('L');
    pub const APP_LOGS: KeyCode = KeyCode::Char('k');
    pub const STATS: KeyCode = KeyCode::Char('S');
    pub const SWEEP: KeyCode = KeyCode::Char('u');
    pub const CLEAR_SESSION: KeyCode = KeyCode::Char('x');
    pub const CYCLE_CONFIG: KeyCode = KeyCode::Char('C');
    pub const PANEL: KeyCode = KeyCode::Tab;
    pub const PAGE_UP: KeyCode = KeyCode::Char(',');
    pub const PAGE_DOWN: KeyCode = KeyCode::Char('.');
    pub const BATCH_DECREASE: KeyCode = KeyCode::Char('[');
    pub const BATCH_INCREASE: KeyCode = KeyCode::Char(']');
}
