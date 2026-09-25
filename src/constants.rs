use std::time::Duration;

pub const CONFIG_PATH: &str = "config.json";

pub const NO_VALUE: &str = "-";

pub const UNINTERPRETABLE: &str = "?";

pub const ELLIPSIS: &str = "...";

pub const EVENT_HANDLER_TICKRATE: Duration = Duration::from_millis(100);

pub const SEARCH_POPUP_MAX_HEIGHT_PERCENT: u16 = 80;
pub const SEARCH_POPUP_MAX_WIDTH_PERCENT: u16 = 50;

pub mod message {
    pub const DEVICE_BUSY: &str = "Device is busy.";
    pub const DEVICE_READING: &str = "Device is currently reading, try again.";
    pub const NO_DEVICE: &str = "No device connected";
    pub const CLIPBOARD_UNAVAILABLE: &str = "Clipboard unavailable";
    pub const CONNECTING: &str = "Connecting...";
    pub const CONNECT_TASK_STOPPED: &str = "Connection failed: task stopped unexpectedly";
    pub const SCAN_NEEDS_IPV4: &str = "Enter an IPv4 address to pick the subnet to scan";
    pub const SCAN_UNAVAILABLE_WEB: &str = "Network scan isn't available in the web demo";
    pub const SCAN_FOUND_NOTHING: &str = "No devices found on this subnet";
    pub const RAW_READ_ONLY: &str =
        "Read-only mode is on - custom calls may write and are disabled";
    pub const RAW_BAD_FUNCTION_CODE: &str = "Function code must be 0-255";
    pub const SENDING: &str = "Sending...";
    pub const TASK_STOPPED: &str = "Failed: task stopped unexpectedly";
    pub const PASTE_NOT_REGISTERS: &str = "Pasted text isn't pinned/labels/custom data";
    pub const CONFIG_COPIED: &str = "Copied the configuration to clipboard";
    pub const LOADING: &str = "Loading...";
    pub const NO_NEXT_CONFIG: &str = "No next configuration set";
    pub const READ_ONLY_ON: &str = "Read-only mode is on (toggle in settings)";
    pub const READ_ONLY: &str = "Read-only mode.";
    pub const ENTER_VALUE: &str = "Enter a value first.";
    pub const WRITING: &str = "Writing...";
    pub const READING: &str = "Reading...";
    pub const READ_TASK_STOPPED: &str = "Read failed: task stopped unexpectedly";
    pub const NO_ID_OBJECTS: &str = "No identification objects returned";
    pub const NOTHING_READ_HERE: &str = "Nothing read here yet";
    pub const GRAPH_CLEARED: &str = "Cleared graph history";
    pub const NOTHING_TO_DUMP: &str = "Nothing read yet to dump.";
    pub const SESSION_CLEARED: &str = "Cleared session read data";
    pub const LOAD_NEEDS_FILE_NAME: &str = "Load failed: enter a file name";
    pub const LOAD_TASK_STOPPED: &str = "Load failed: task stopped unexpectedly";
}

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
