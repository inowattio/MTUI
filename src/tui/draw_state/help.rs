use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Paragraph};
use crate::constants::keybind::*;

pub fn draw(frame: &mut Frame, outer: Block, base_style: Style, device: String) {
    let content = format!("{EXIT} - Exit/Back
{MOVE_UP}/{MOVE_DOWN} - Move Cursor
{REFRESH} - Refresh Data
{TOGGLE} - Switch Register Type
{WRITE} - Write
{JUMP} - Jump
{DUMP} - Dump
{HELP} - Help
{PIN} - Add/Remove Pin (Read only)
{ACTION} - Action");
    
    frame.render_widget(
        Paragraph::new(format!("Device: {device}\n{content}"))
            .block(outer)
            .style(base_style)
            .alignment(Alignment::Left),
        frame.area(),
    );
}
