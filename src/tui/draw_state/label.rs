use crate::state::LabelParams;
use ratatui::layout::Alignment;
use ratatui::prelude::Style;
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

pub fn draw(
    params: &LabelParams,
    frame: &mut Frame,
    outer: Block,
    base_style: Style,
    device: String,
) {
    let text = if params.text.is_empty() {
        "(empty - will remove label)".to_string()
    } else {
        params.text.clone()
    };

    let result = params
        .result
        .as_deref()
        .map(|r| format!("\n{r}"))
        .unwrap_or_default();

    let content = format!(
        "Label at {} ({:?}): {}{}",
        params.position, params.register_type, text, result
    );

    frame.render_widget(
        Paragraph::new(format!("Device: {device}\n{content}"))
            .block(outer)
            .style(base_style)
            .alignment(Alignment::Left),
        frame.area(),
    );
}
