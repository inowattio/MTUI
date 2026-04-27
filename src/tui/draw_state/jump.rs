use crate::state::JumpParams;
use ratatui::layout::Alignment;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

pub fn draw(
    params: &JumpParams,
    frame: &mut Frame,
    outer: Block,
    base_style: Style,
    device: String,
) {
    let content = format!("Jump from {} at: {:?}", params.from, params.to);

    frame.render_widget(
        Paragraph::new(format!("Device: {device}\n{content}"))
            .block(outer)
            .style(base_style)
            .alignment(Alignment::Left),
        frame.area(),
    );
}
