use crate::app::{App, StatTarget};
use crate::config::AddressMode;
use crate::constants::NO_VALUE;
use crate::input::KeyCode;
use crate::interpretator::format_ago;
use crate::register::RegisterType;
use crate::state::StatsTab;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use std::fmt::Write as _;
use std::time::Duration;

const LABEL_W: usize = 9;
const VALUE_W: usize = 28;

pub(super) fn draw(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, tab: StatsTab) {
    let s = app.stats.tab(tab);
    let address_mode = app.interpreter.address_mode();

    let field = |label: &str, value: String| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!(" {label:<LABEL_W$}"), theme.dim_style()),
            Span::styled(value, theme.base()),
        ])
    };

    let tab_index = StatsTab::ALL.iter().position(|&t| t == tab).unwrap_or(0);
    let mut tabs = vec![Span::raw(" ")];
    tabs.extend(theme.tab_spans(StatsTab::ALL.map(StatsTab::label), tab_index));

    let mut errors = s.errors.to_string();
    if let Some(rate) = s.error_rate() {
        let _ = write!(errors, " ({rate:.1}%)");
    }

    let latency = match s.latency() {
        Some((min, avg, max)) => {
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            format!("{:.2} / {:.2} / {:.2} ms", ms(min), ms(avg), ms(max))
        }
        None => NO_VALUE.to_string(),
    };

    let (message, target, when) = match s.last_error() {
        Some(error) => (
            super::truncate(&error.message, VALUE_W),
            error.target.map_or_else(
                || NO_VALUE.to_string(),
                |t| target_text(t, address_mode, tab == StatsTab::All),
            ),
            format_ago(Utc::now().signed_duration_since(error.at)),
        ),
        None => (
            NO_VALUE.to_string(),
            NO_VALUE.to_string(),
            NO_VALUE.to_string(),
        ),
    };

    let mut lines: Vec<Line> = vec![
        Line::from(tabs),
        Line::default(),
        field("Ok", s.ok.to_string()),
        field("Errors", errors),
        field("Latency", latency),
        Line::default(),
        field("Error", message),
        field("Target", target),
        field("When", when),
        Line::default(),
        Line::from(Span::styled(
            " Counted since connect or clear",
            theme.dim_style(),
        )),
    ];

    let footer = [
        Hint::key(app.config.keybinds.panel, "Next"),
        Hint::key(app.config.keybinds.clear_session, "Clear"),
        Hint::key(KeyCode::Esc, "Close"),
    ];
    let content_w = lines.iter().map(Line::width).max().unwrap_or(0) as u16;
    // leading space, label, value, a column of right padding and borders (2)
    let width = hints::min_width((1 + LABEL_W + VALUE_W + 3) as u16, &footer).max(content_w + 3);
    super::push_footer(&mut lines, theme, footer);

    super::render(frame, area, theme, "Statistics", width, lines);
}

fn target_text(target: StatTarget, address_mode: AddressMode, with_op: bool) -> String {
    let (text, op) = match target {
        StatTarget::Read(kind, address, count) => {
            (registers_text(kind, address, count, address_mode), "read")
        }
        StatTarget::Write(kind, address, count) => {
            (registers_text(kind, address, count, address_mode), "write")
        }
        StatTarget::Function(code) => (format!("function {code:#04X}"), "raw"),
    };
    if with_op {
        format!("{text} ({op})")
    } else {
        text
    }
}

fn registers_text(kind: RegisterType, address: u16, count: u16, mode: AddressMode) -> String {
    let last = address.saturating_add(count.saturating_sub(1));
    if last == address {
        format!("{} {}", kind.name(), mode.display(address))
    } else {
        format!(
            "{} {}-{}",
            kind.name(),
            mode.display(address),
            mode.display(last)
        )
    }
}
