mod columns;
mod confirm;
mod custom;
mod help;
mod inspect;
mod label;
mod logs;
mod search;
mod slave;
mod sweep_config;
mod write;

use crate::app::App;
use crate::state::Popup;
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

pub fn draw_popup(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, popup: &Popup) {
    let kb = &app.config.keybinds;
    match popup {
        Popup::Help => help::draw(frame, area, theme, kb),
        Popup::Dump(d) => confirm::draw(
            frame,
            area,
            theme,
            "Dump",
            &format!("Dump {} read register(s) to a file?", app.read_count()),
            &d.result,
            &format!(
                " {} \u{b7} confirm   backspace/{} cancel",
                kb.action, kb.exit
            ),
        ),
        Popup::Search(s) => search::draw(frame, area, theme, kb, s),
        Popup::Label(l) => label::draw(frame, area, theme, kb, l),
        Popup::Custom(c) => custom::draw(frame, area, theme, app, c),
        Popup::Columns(selected) => columns::draw(frame, area, theme, app, *selected),
        Popup::Write(write) => write::draw(frame, area, theme, kb, write),
        Popup::Slave(value) => slave::draw(frame, area, theme, kb, *value),
        Popup::SweepConfig(s) => sweep_config::draw(frame, area, theme, kb, s, app.sweep.active),
        Popup::Logs(logs) => logs::draw(frame, area, theme, kb, logs),
        Popup::Inspect => inspect::draw(frame, area, theme, app),
        Popup::Quit => confirm::draw(
            frame,
            area,
            theme,
            "Unsaved changes",
            " Quit anyway?",
            &None,
            &format!(
                " {}/{} \u{b7} confirm   backspace cancel",
                kb.action, kb.exit
            ),
        ),
    }
}

fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    title: &str,
    width: u16,
    lines: Vec<Line<'static>>,
) {
    let height = lines.len() as u16 + 2;
    let rect = centered_rect(width, height, area);
    frame.render_widget(Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(theme.panel(title)), rect);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
