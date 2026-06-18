use crate::input::KeyCode;
use crate::tui::theme::Theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub struct Hint {
    key: String,
    label: String,
}

impl Hint {
    pub fn key(kc: KeyCode, label: &str) -> Self {
        Self {
            key: glyph(kc),
            label: label.to_string(),
        }
    }

    pub fn keys(key: impl Into<String>, label: &str) -> Self {
        Self {
            key: key.into(),
            label: label.to_string(),
        }
    }

    pub fn note(label: &str) -> Self {
        Self {
            key: String::new(),
            label: label.to_string(),
        }
    }

    fn width(&self) -> usize {
        if self.key.is_empty() {
            self.label.chars().count()
        } else {
            1 + self.key.chars().count() + 2 + self.label.chars().count()
        }
    }
}

pub fn glyph(kc: KeyCode) -> String {
    match kc {
        KeyCode::Up => "\u{2191}".to_string(),
        KeyCode::Down => "\u{2193}".to_string(),
        KeyCode::Left => "\u{2190}".to_string(),
        KeyCode::Right => "\u{2192}".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        other => other.to_string(),
    }
}

pub fn pair(a: KeyCode, b: KeyCode) -> String {
    let arrow = |k| {
        matches!(
            k,
            KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
        )
    };
    if arrow(a) && arrow(b) {
        format!("{}{}", glyph(a), glyph(b))
    } else {
        format!("{}/{}", glyph(a), glyph(b))
    }
}

const SEP: &str = " ";

pub fn width(items: &[Hint]) -> usize {
    let tokens: usize = items.iter().map(Hint::width).sum::<usize>()
        + SEP.chars().count() * items.len().saturating_sub(1);
    tokens + 2 + 2
}

pub fn footer(theme: &Theme, items: &[Hint]) -> Line<'static> {
    let key_style = Style::default().fg(theme.accent);
    let dim = theme.dim_style();
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(items.len() * 5 + 2);
    spans.push(Span::styled(" ", dim));
    for (i, h) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEP, dim));
        }
        if h.key.is_empty() {
            spans.push(Span::styled(h.label.clone(), dim));
        } else {
            spans.push(Span::styled("[", dim));
            spans.push(Span::styled(h.key.clone(), key_style));
            spans.push(Span::styled("] ", dim));
            spans.push(Span::styled(h.label.clone(), dim));
        }
    }
    spans.push(Span::styled(" ", dim));
    Line::from(spans)
}

pub fn more(theme: &Theme, above: usize, below: usize) -> Line<'static> {
    let dim = theme.dim_style();
    let mut spans: Vec<Span<'static>> = Vec::new();
    if above > 0 {
        spans.push(Span::styled(format!(" \u{2191} {above} more"), dim));
    }
    if above > 0 && below > 0 {
        spans.push(Span::styled(SEP, dim));
    }
    if below > 0 {
        spans.push(Span::styled(format!("\u{2193} {below} more"), dim));
    }
    Line::from(spans)
}
