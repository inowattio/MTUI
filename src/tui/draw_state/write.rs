use ratatui::Frame;
use ratatui::layout::Alignment;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Paragraph};
use crate::state::WriteParams;

pub fn draw(params: &WriteParams, frame: &mut Frame, outer: Block, base_style: Style, device: String) {
    let content = format!("Write at {} value: {} ({:?})\nResult: {:?}",
                          params.position, params.value.map_or("none".to_string(), |n| n.to_string()), params.write_type, params.result);

    frame.render_widget(
        Paragraph::new(format!("Device: {device}\n{content}"))
            .block(outer)
            .style(base_style)
            .alignment(Alignment::Left),
        frame.area(),
    );
}
