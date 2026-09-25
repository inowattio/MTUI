use crate::state::{ConnectionStatus, MessageKind, StatusMessage};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders};
use serde::{Deserialize, Serialize};

const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Theme {
    pub background: Color,
    pub border: Color,
    pub accent: Color,
    pub text: Color,
    pub dim: Color,
    pub changed: Color,
    pub zebra: Color,
    pub ok: Color,
    pub warning: Color,
    pub error: Color,
    pub selected_text: Color,
    pub selected_background: Color,
    pub series_1: Color,
    pub series_2: Color,
    pub series_3: Color,
}

const DEFAULT: Theme = Theme {
    background: Color::Reset,
    border: Color::LightGreen,
    accent: Color::LightGreen,
    text: Color::White,
    dim: Color::DarkGray,
    changed: Color::Yellow,
    zebra: Color::Indexed(235),
    ok: Color::LightGreen,
    warning: Color::Yellow,
    error: Color::LightRed,
    selected_text: Color::Black,
    selected_background: Color::LightGreen,
    series_1: Color::LightBlue,
    series_2: Color::LightMagenta,
    series_3: Color::LightYellow,
};

const LIGHT: Theme = Theme {
    background: Color::Indexed(255),
    border: Color::Blue,
    accent: Color::Blue,
    text: Color::Black,
    dim: Color::DarkGray,
    changed: Color::Indexed(166),
    zebra: Color::Indexed(253),
    ok: Color::Green,
    warning: Color::Indexed(130),
    error: Color::Red,
    selected_text: Color::White,
    selected_background: Color::Blue,
    series_1: Color::Indexed(125),
    series_2: Color::Indexed(30),
    series_3: Color::Indexed(94),
};

const AMBER: Theme = Theme {
    background: Color::Indexed(233),
    border: Color::Indexed(130),
    accent: Color::Indexed(214),
    text: Color::Indexed(223),
    dim: Color::Indexed(94),
    changed: Color::Indexed(229),
    zebra: Color::Indexed(236),
    ok: Color::Indexed(142),
    warning: Color::Indexed(208),
    error: Color::Indexed(196),
    selected_text: Color::Black,
    selected_background: Color::Indexed(214),
    series_1: Color::Indexed(230),
    series_2: Color::Indexed(166),
    series_3: Color::Indexed(143),
};

impl Default for Theme {
    fn default() -> Self {
        DEFAULT
    }
}

impl Theme {
    pub const PRESETS: &'static [(&'static str, Self)] =
        &[("Default", DEFAULT), ("Light", LIGHT), ("Amber", AMBER)];

    pub fn base(&self) -> Style {
        Style::default().fg(self.text)
    }

    pub fn series_style(&self, index: usize) -> Style {
        let colors = [self.series_1, self.series_2, self.series_3];
        Style::default().fg(colors[index % colors.len()])
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
            .bg(self.selected_background)
            .fg(self.selected_text)
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
        Style::default().fg(self.error)
    }

    pub fn warn_style(&self) -> Style {
        Style::default().fg(self.warning)
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
        let separator = Span::styled(" | ", self.dim_style());
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
                spans.push(Span::styled(" | ", self.dim_style()));
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

pub const fn spinner_frame(frame: u64) -> &'static str {
    SPINNER_FRAMES[(frame as usize) % SPINNER_FRAMES.len()]
}

pub fn status_parts(
    status: &ConnectionStatus,
    theme: &Theme,
) -> (&'static str, &'static str, Style) {
    let (symbol, label, color) = match status {
        ConnectionStatus::Unknown => ("o", "no data", theme.dim),
        ConnectionStatus::Reading => (":", "reading", theme.warning),
        ConnectionStatus::Connected => ("*", "connected", theme.ok),
        ConnectionStatus::Reconnecting => ("~", "reconnecting", theme.warning),
        ConnectionStatus::Error(_) => ("!", "error", theme.error),
    };
    (
        symbol,
        label,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub fn status_span(status: &ConnectionStatus, theme: &Theme) -> Span<'static> {
    let (symbol, label, style) = status_parts(status, theme);
    Span::styled(format!("{symbol} {label} "), style)
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn every_preset_keeps_its_graph_series_apart_from_each_other_and_the_primary() {
        for (name, theme) in Theme::PRESETS {
            let series = [theme.series_1, theme.series_2, theme.series_3];
            for (i, color) in series.iter().enumerate() {
                assert_ne!(
                    *color,
                    theme.accent,
                    "{name}: series {} blends with the primary",
                    i + 1
                );
                assert_ne!(
                    *color,
                    theme.background,
                    "{name}: series {} is invisible",
                    i + 1
                );
                assert!(
                    !series[i + 1..].contains(color),
                    "{name}: series {} repeats a color",
                    i + 1
                );
            }
        }
    }

    #[test]
    fn series_styles_cycle_through_the_theme_colors() {
        let theme = Theme::default();
        assert_eq!(theme.series_style(0).fg, Some(theme.series_1));
        assert_eq!(theme.series_style(2).fg, Some(theme.series_3));
        assert_eq!(theme.series_style(3).fg, Some(theme.series_1));
    }
}
