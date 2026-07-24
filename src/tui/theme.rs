use crate::state::{ConnectionStatus, MessageKind, StatusMessage};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use serde::{Deserialize, Serialize};

const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub const PALETTE: &[Color] = &[
    Color::Reset,
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Theme {
    pub bg: Color,
    pub border: Color,
    pub accent: Color,
    pub text: Color,
    pub dim: Color,
    pub changed: Color,
    pub zebra: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub selected_fg: Color,
    pub selected_bg: Color,
}

const DEFAULT: Theme = Theme {
    bg: Color::Reset,
    border: Color::LightGreen,
    accent: Color::LightGreen,
    text: Color::White,
    dim: Color::DarkGray,
    changed: Color::Yellow,
    zebra: Color::Indexed(235),
    ok: Color::LightGreen,
    warn: Color::Yellow,
    err: Color::LightRed,
    selected_fg: Color::Black,
    selected_bg: Color::LightGreen,
};

const LIGHT: Theme = Theme {
    bg: Color::Indexed(255),
    border: Color::Blue,
    accent: Color::Blue,
    text: Color::Black,
    dim: Color::DarkGray,
    changed: Color::Indexed(166),
    zebra: Color::Indexed(253),
    ok: Color::Green,
    warn: Color::Indexed(130),
    err: Color::Red,
    selected_fg: Color::White,
    selected_bg: Color::Blue,
};

const AMBER: Theme = Theme {
    bg: Color::Indexed(233),
    border: Color::Indexed(130),
    accent: Color::Indexed(214),
    text: Color::Indexed(223),
    dim: Color::Indexed(94),
    changed: Color::Indexed(229),
    zebra: Color::Indexed(236),
    ok: Color::Indexed(142),
    warn: Color::Indexed(208),
    err: Color::Indexed(196),
    selected_fg: Color::Black,
    selected_bg: Color::Indexed(214),
};

impl Default for Theme {
    fn default() -> Self {
        DEFAULT
    }
}

impl Theme {
    pub const PRESETS: &'static [(&'static str, Theme)] =
        &[("Default", DEFAULT), ("Light", LIGHT), ("Amber", AMBER)];

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

    pub fn changed_style(&self) -> Style {
        Style::default().fg(self.changed)
    }

    pub fn selected_style(&self) -> Style {
        Style::default()
            .bg(self.selected_bg)
            .fg(self.selected_fg)
            .add_modifier(Modifier::BOLD)
    }

    pub fn row_style(&self, zebra: bool, changed: bool) -> Style {
        let base = if zebra {
            Style::default().fg(self.text).bg(self.zebra)
        } else {
            self.base()
        };

        if changed {
            base.patch(self.changed_style())
        } else {
            base
        }
    }

    pub fn header_style(&self) -> Style {
        Style::default().fg(self.dim).add_modifier(Modifier::BOLD)
    }

    pub fn ok_style(&self) -> Style {
        Style::default().fg(self.ok)
    }

    pub fn err_style(&self) -> Style {
        Style::default().fg(self.err)
    }

    pub fn warn_style(&self) -> Style {
        Style::default().fg(self.warn)
    }

    pub fn message_style(&self, kind: MessageKind) -> Style {
        match kind {
            MessageKind::Ok => self.ok_style(),
            MessageKind::Warn => self.warn_style(),
            MessageKind::Err => self.err_style(),
            MessageKind::Info => self.dim_style(),
        }
    }

    pub fn line_style(&self, selected: bool) -> Style {
        if selected {
            self.selected_style()
        } else {
            self.base()
        }
    }

    pub fn status_line(&self, status: &StatusMessage) -> Line<'static> {
        Line::from(Span::styled(
            status.text.clone(),
            self.message_style(status.kind),
        ))
    }

    pub fn join_dotted(
        &self,
        groups: impl IntoIterator<Item = Vec<Span<'static>>>,
    ) -> Vec<Span<'static>> {
        let separator = Span::styled(" \u{b7} ", self.dim_style());
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, group) in groups.into_iter().enumerate() {
            if i > 0 {
                spans.push(separator.clone());
            }
            spans.extend(group);
        }
        spans
    }

    pub fn panel(&self, title: &str) -> Block<'static> {
        Block::default()
            .title_top(Line::styled(format!("{title} "), self.accent_style()))
            .borders(Borders::TOP)
            .border_type(BorderType::Rounded)
            .border_style(self.dim_style())
    }

    pub fn tab_spans(
        &self,
        names: impl IntoIterator<Item = impl Into<String>>,
        active: usize,
    ) -> Vec<Span<'static>> {
        let mut spans = Vec::new();
        for (i, name) in names.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" \u{2502} ", self.dim_style()));
            }
            let style = if i == active {
                self.accent_style()
            } else {
                self.dim_style()
            };
            spans.push(Span::styled(name.into(), style));
        }
        spans
    }

    pub fn tabbed_panel(&self, names: &[&'static str], active: usize) -> Block<'static> {
        let mut spans = self.tab_spans(names.iter().copied(), active);
        spans.push(Span::raw(" "));
        Block::default()
            .title_top(Line::from(spans))
            .borders(Borders::TOP)
            .border_type(BorderType::Rounded)
            .border_style(self.dim_style())
    }
}

pub fn spinner_frame(frame: u64) -> &'static str {
    SPINNER_FRAMES[(frame as usize) % SPINNER_FRAMES.len()]
}

pub fn status_span(status: &ConnectionStatus, theme: &Theme) -> Span<'static> {
    let (symbol, label, color) = match status {
        ConnectionStatus::Unknown => ("○", "no data", theme.dim),
        ConnectionStatus::Reading => ("◍", "reading", theme.warn),
        ConnectionStatus::Connected => ("●", "connected", theme.ok),
        ConnectionStatus::Reconnecting => ("↻", "reconnecting", theme.warn),
        ConnectionStatus::Error(_) => ("●", "error", theme.err),
    };
    Span::styled(
        format!("{symbol} {label:<9} "),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}
