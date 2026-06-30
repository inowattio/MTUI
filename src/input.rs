use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
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

impl KeyCode {
    pub fn from_name(s: &str) -> Option<Self> {
        let named = match s.to_ascii_lowercase().as_str() {
            "esc" | "escape" => Some(KeyCode::Esc),
            "enter" | "return" => Some(KeyCode::Enter),
            "backspace" => Some(KeyCode::Backspace),
            "tab" => Some(KeyCode::Tab),
            "up" => Some(KeyCode::Up),
            "down" => Some(KeyCode::Down),
            "left" => Some(KeyCode::Left),
            "right" => Some(KeyCode::Right),
            "pageup" => Some(KeyCode::PageUp),
            "pagedown" => Some(KeyCode::PageDown),
            "space" => Some(KeyCode::Char(' ')),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        let mut chars = s.chars();
        let c = chars.next()?;
        chars.next().is_none().then_some(KeyCode::Char(c))
    }
}

impl Serialize for KeyCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for KeyCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        KeyCode::from_name(&s).ok_or_else(|| de::Error::custom(format!("invalid key: {s:?}")))
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
