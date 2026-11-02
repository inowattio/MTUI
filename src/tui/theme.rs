use crate::app::Trend;
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
    pub trend_up: Color,
    pub trend_down: Color,
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
    trend_up: Color::Green,
    trend_down: Color::Red,
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
    trend_up: Color::Green,
    trend_down: Color::Red,
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
    trend_up: Color::Green,
    trend_down: Color::Red,
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

    fn heated(&self, color: Color, heat: f32) -> Color {
        if heat >= 1.0 {
            return color;
        }
        blend(self.text, color, heat, truecolor()).unwrap_or(color)
    }

    pub fn changed_color(&self, heat: f32) -> Color {
        self.heated(self.changed, heat)
    }

    pub fn trend_style(&self, trend: Trend, heat: f32) -> Style {
        let color = match trend {
            Trend::Up => self.trend_up,
            Trend::Down => self.trend_down,
        };
        Style::default().fg(self.heated(color, heat))
    }

    pub fn row_style(&self, zebra: bool, heat: Option<f32>) -> Style {
        let base = if zebra {
            Style::default().fg(self.text).bg(self.zebra)
        } else {
            self.base()
        };

        match heat {
            Some(heat) => base.fg(self.changed_color(heat)),
            None => base,
        }
    }

    pub fn placeholder_style(&self, zebra: bool) -> Style {
        if zebra {
            self.dim_style().bg(self.zebra)
        } else {
            self.dim_style()
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

const ANSI_RGB: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

#[cfg(target_arch = "wasm32")]
const fn truecolor() -> bool {
    true
}

#[cfg(not(target_arch = "wasm32"))]
fn truecolor() -> bool {
    static TRUECOLOR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRUECOLOR.get_or_init(|| {
        std::env::var("COLORTERM")
            .is_ok_and(|value| matches!(value.as_str(), "truecolor" | "24bit"))
    })
}

const fn indexed_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => ANSI_RGB[index as usize],
        16..=231 => {
            let i = index - 16;
            (
                CUBE_LEVELS[(i / 36) as usize],
                CUBE_LEVELS[(i / 6 % 6) as usize],
                CUBE_LEVELS[(i % 6) as usize],
            )
        }
        _ => {
            let level = 8 + 10 * (index - 232);
            (level, level, level)
        }
    }
}

const fn color_rgb(color: Color) -> Option<(u8, u8, u8)> {
    let index = match color {
        Color::Reset => return None,
        Color::Rgb(r, g, b) => return Some((r, g, b)),
        Color::Indexed(index) => index,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    Some(indexed_rgb(index))
}

fn distance((r1, g1, b1): (u8, u8, u8), (r2, g2, b2): (u8, u8, u8)) -> u32 {
    let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).unsigned_abs().pow(2);
    d(r1, r2) + d(g1, g2) + d(b1, b2)
}

fn nearest_indexed(rgb: (u8, u8, u8)) -> u8 {
    let level = |v: u8| {
        (0..CUBE_LEVELS.len())
            .min_by_key(|&i| (i32::from(CUBE_LEVELS[i]) - i32::from(v)).unsigned_abs())
            .unwrap_or(0) as u8
    };
    let cube = 16 + 36 * level(rgb.0) + 6 * level(rgb.1) + level(rgb.2);
    let average = (u32::from(rgb.0) + u32::from(rgb.1) + u32::from(rgb.2)) / 3;
    let gray = 232 + (average.saturating_sub(3) / 10).min(23) as u8;
    if distance(indexed_rgb(gray), rgb) < distance(indexed_rgb(cube), rgb) {
        gray
    } else {
        cube
    }
}

fn blend(from: Color, to: Color, amount: f32, truecolor: bool) -> Option<Color> {
    let (from, to) = (color_rgb(from)?, color_rgb(to)?);
    let amount = amount.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| {
        (f32::from(b) - f32::from(a))
            .mul_add(amount, f32::from(a))
            .round() as u8
    };
    let rgb = (mix(from.0, to.0), mix(from.1, to.1), mix(from.2, to.2));
    Some(if truecolor {
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    } else {
        Color::Indexed(nearest_indexed(rgb))
    })
}

pub fn status_span(status: &ConnectionStatus, theme: &Theme) -> Span<'static> {
    let (symbol, label, style) = status_parts(status, theme);
    Span::styled(format!("{symbol} {label} "), style)
}

#[cfg(test)]
mod tests {
    use super::{Theme, blend, nearest_indexed};
    use ratatui::style::Color;

    #[test]
    fn a_blend_moves_from_the_first_color_to_the_second() {
        let (black, white) = (Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50));
        assert_eq!(blend(black, white, 0.0, true), Some(black));
        assert_eq!(blend(black, white, 1.0, true), Some(white));
        assert_eq!(
            blend(black, white, 0.5, true),
            Some(Color::Rgb(100, 50, 25))
        );
    }

    #[test]
    fn named_and_indexed_colors_blend_through_their_palette_values() {
        assert_eq!(
            blend(Color::White, Color::Yellow, 1.0, true),
            Some(Color::Rgb(205, 205, 0))
        );
        assert_eq!(
            blend(Color::Indexed(16), Color::Indexed(231), 1.0, true),
            Some(Color::Rgb(255, 255, 255))
        );
        assert_eq!(
            blend(Color::Indexed(232), Color::Indexed(255), 0.0, true),
            Some(Color::Rgb(8, 8, 8))
        );
    }

    #[test]
    fn the_default_color_cannot_be_blended() {
        assert_eq!(blend(Color::Reset, Color::Yellow, 0.5, true), None);
        assert_eq!(blend(Color::White, Color::Reset, 0.5, true), None);
    }

    #[test]
    fn without_truecolor_a_blend_lands_on_the_closest_palette_entry() {
        assert_eq!(nearest_indexed((255, 255, 0)), 226);
        assert_eq!(nearest_indexed((128, 128, 128)), 244);
        assert_eq!(nearest_indexed((0, 0, 0)), 16);
        assert_eq!(
            blend(Color::Rgb(0, 0, 0), Color::Rgb(255, 0, 0), 1.0, false),
            Some(Color::Indexed(196))
        );
    }

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
