use crate::state::ConnectionStatus;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};

/// Frames for the loading spinner, advanced once per tick via `App::frame`.
const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

/// Central green-forward palette for the whole UI. Built once per frame in
/// `Tui::draw` and threaded into every screen so styling stays consistent.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub border: Color,
    pub accent: Color,
    pub text: Color,
    pub dim: Color,
    pub changed: Color,
    pub zebra: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            border: Color::LightGreen,
            accent: Color::LightGreen,
            text: Color::White,
            dim: Color::DarkGray,
            changed: Color::Yellow,
            zebra: Color::Indexed(235),
            ok: Color::LightGreen,
            warn: Color::Yellow,
            err: Color::LightRed,
        }
    }
}

impl Theme {
    pub fn base(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.dim)
    }

    pub fn accent_style(&self) -> Style {
        Style::default()
            .fg(self.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Yellow-bold row whose register value changed since the previous read.
    pub fn changed_style(&self) -> Style {
        Style::default()
            .fg(self.changed)
            .add_modifier(Modifier::BOLD)
    }

    /// The cursor bar marking the active register (the one actions target).
    pub fn selected_style(&self) -> Style {
        Style::default()
            .bg(self.accent)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD)
    }

    /// Subtle background for alternating table rows.
    pub fn zebra_style(&self) -> Style {
        Style::default().fg(self.text).bg(self.zebra)
    }

    pub fn header_style(&self) -> Style {
        Style::default()
            .fg(self.dim)
            .add_modifier(Modifier::BOLD)
    }

    pub fn ok_style(&self) -> Style {
        Style::default().fg(self.ok)
    }

    /// A rounded, dim-bordered inner panel with an accented title. Shared by the
    /// register tables and the form-style secondary screens for a consistent look.
    pub fn panel(&self, title: &str) -> Block<'static> {
        Block::default()
            .title_top(Line::styled(format!(" {title} "), self.accent_style()))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.dim_style())
    }
}

pub fn spinner_frame(frame: u64) -> &'static str {
    SPINNER_FRAMES[(frame as usize) % SPINNER_FRAMES.len()]
}

/// A colored ●/◍ indicator + label reflecting real device reachability.
pub fn status_span(status: &ConnectionStatus, theme: &Theme) -> Span<'static> {
    let (symbol, label, color) = match status {
        ConnectionStatus::Unknown => ("○", "no data", theme.dim),
        ConnectionStatus::Reading => ("◍", "reading", theme.warn),
        ConnectionStatus::Connected => ("●", "connected", theme.ok),
        ConnectionStatus::Error(_) => ("●", "error", theme.err),
    };
    Span::styled(
        format!("{symbol} {label} "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}
