use ratatui::{
    layout::Alignment,
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use crate::app::App;

/// Renders the user interface widgets.
pub fn render(app: &mut App, frame: &mut Frame) {
    // This is where you add new widgets.
    // See the following resources:
    // - https://docs.rs/ratatui/latest/ratatui/widgets/index.html
    // - https://github.com/ratatui-org/ratatui/tree/master/examples
    frame.render_widget(
        Paragraph::new(format!("Index: {} {}\n{}", app.position, app.displaying_type(), app.rendered_data))
        .block(
            Block::default()
                .title("Press `q` to stop running, up and down to move, r to refresh values and t to switch between register types.")
                .title_alignment(Alignment::Center)
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded),
        )
        .style(Style::default().fg(Color::Cyan).bg(Color::Black))
        .alignment(Alignment::Left),
        frame.size(),
    )
}
