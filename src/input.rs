use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Tab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Backspace => f.write_str("Backspace"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::Up => f.write_str("Up"),
            KeyCode::Down => f.write_str("Down"),
            KeyCode::Left => f.write_str("Left"),
            KeyCode::Right => f.write_str("Right"),
            KeyCode::PageUp => f.write_str("PageUp"),
            KeyCode::PageDown => f.write_str("PageDown"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
}

impl KeyEvent {
    pub fn new(code: KeyCode) -> Self {
        Self { code }
    }
}
