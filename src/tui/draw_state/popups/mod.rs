mod columns;
mod confirm;
mod custom;
mod device_id;
mod help;
mod import;
mod inspect;
mod label;
mod logs;
mod raw;
mod search;
mod slave;
mod sweep_config;
mod write;

use crate::app::App;
use crate::input::KeyCode;
use crate::state::Popup;
use crate::tui::hints::{self, Hint};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

pub fn draw_popup(frame: &mut Frame, area: Rect, theme: &Theme, app: &App, popup: &Popup) {
    let kb = &app.config.keybinds;
    match popup {
        Popup::Help(h) => help::draw(frame, area, theme, kb, app, h),
        Popup::Dump(d) => confirm::draw(
            frame,
            area,
            theme,
            "Dump",
            &format!("Dump {} read register(s) to a file?", app.read_count()),
            &d.result,
            hints::footer(
                theme,
                [
                    Hint::key(kb.action, "Confirm"),
                    Hint::pair(KeyCode::Backspace, kb.exit, "Cancel"),
                ],
            ),
        ),
        Popup::Search(s) => search::draw(frame, area, theme, kb, s),
        Popup::Label(l) => label::draw(frame, area, theme, kb, l),
        Popup::Custom(c) => custom::draw(frame, area, theme, app, c),
        Popup::Columns(params) => columns::draw(frame, area, theme, app, params),
        Popup::Write(write) => write::draw(frame, area, theme, kb, write),
        Popup::Slave(value) => slave::draw(frame, area, theme, kb, *value),
        Popup::SweepConfig(s) => sweep_config::draw(frame, area, theme, kb, s, app.sweep.active),
        Popup::Logs(logs) => logs::draw(frame, area, theme, kb, logs),
        Popup::Inspect => inspect::draw(frame, area, theme, app),
        Popup::DeviceId(params) => device_id::draw(frame, area, theme, kb, params),
        Popup::Raw(params) => raw::draw(frame, area, theme, kb, params),
        Popup::Import(params) => import::draw(frame, area, theme, kb, params),
        Popup::Quit => confirm::draw(
            frame,
            area,
            theme,
            "Unsaved changes",
            " Quit anyway?",
            &None,
            hints::footer(
                theme,
                [
                    Hint::pair(kb.action, kb.exit, "Confirm"),
                    Hint::key(KeyCode::Backspace, "Cancel"),
                ],
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

pub(super) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}
