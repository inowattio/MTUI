use crate::input::KeyCode;
use crate::tui::theme::Theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

pub struct Hint {
    key: String,
    label: &'static str,
}

impl Hint {
    pub fn key(kc: KeyCode, label: &'static str) -> Self {
        Self {
            key: glyph(kc),
            label,
        }
    }

    pub fn pair(a: KeyCode, b: KeyCode, label: &'static str) -> Self {
        Self {
            key: format!("{}/{}", glyph(a), glyph(b)),
            label,
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

fn glyph(kc: KeyCode) -> String {
    match kc {
        KeyCode::Up => "\u{2191}".to_string(),
        KeyCode::Down => "\u{2193}".to_string(),
        KeyCode::Left => "\u{2190}".to_string(),
        KeyCode::Right => "\u{2192}".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Delete => "Del".to_string(),
        other => other.to_string(),
    }
}

const SEP: &str = " ";

pub fn width(items: &[Hint]) -> usize {
    let tokens: usize = items.iter().map(Hint::width).sum::<usize>()
        + SEP.chars().count() * items.len().saturating_sub(1);
    tokens + 1 + 2
}

pub fn min_width(min: u16, items: &[Hint]) -> u16 {
    min.max(width(items) as u16)
}

pub fn footer<const N: usize>(theme: &Theme, items: [Hint; N]) -> Line<'static> {
    let key_style = Style::default().fg(theme.accent);
    let dim = theme.dim_style();
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(items.len() * 5 + 2);
    spans.push(Span::styled(" ", dim));
    for (i, h) in items.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEP, dim));
        }

        spans.push(Span::styled("[", dim));
        spans.push(Span::styled(h.key, key_style));
        spans.push(Span::styled("] ", dim));
        spans.push(Span::styled(h.label, dim));
    }
    Line::from(spans)
}

pub fn hscroll(theme: &Theme, offset: u16, max_offset: u16) -> Option<Line<'static>> {
    if max_offset == 0 {
        return None;
    }
    let left = if offset > 0 { '\u{25c2}' } else { ' ' };
    let right = if offset < max_offset { '\u{25b8}' } else { ' ' };
    Some(Line::styled(
        format!(" {left} cols {right} "),
        theme.dim_style(),
    ))
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
