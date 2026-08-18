use serde::de::{self, Deserialize, Deserializer};
use serde::{Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Esc,
    Enter,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
}

macro_rules! named_keys {
    ( $( $variant:ident $( ( $ch:literal ) )? => $canonical:literal $( | $alias:literal )* ),+ $(,)? ) => {
        const NAMED_KEYS: &[(KeyCode, &str, &[&str])] = &[
            $( (KeyCode::$variant $( ( $ch ) )?, $canonical, &[$( $alias ),*]) ),+
        ];

        impl fmt::Display for KeyCode {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $( KeyCode::$variant $( ( $ch ) )? => f.write_str($canonical), )+
                    KeyCode::Char(c) => write!(f, "{c}"),
                }
            }
        }
    };
}

named_keys! {
    Char(' ') => "Space",
    Esc => "Esc" | "escape",
    Enter => "Enter" | "return",
    Backspace => "Backspace",
    Delete => "Delete" | "del",
    Tab => "Tab",
    BackTab => "Shift+Tab" | "backtab",
    Up => "Up",
    Down => "Down",
    Left => "Left",
    Right => "Right",
    PageUp => "PageUp",
    PageDown => "PageDown",
}

impl KeyCode {
    fn from_name(s: &str) -> Option<Self> {
        let named = NAMED_KEYS.iter().find(|(_, canonical, aliases)| {
            canonical.eq_ignore_ascii_case(s) || aliases.iter().any(|a| a.eq_ignore_ascii_case(s))
        });
        if let Some(&(key, _, _)) = named {
            return Some(key);
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
