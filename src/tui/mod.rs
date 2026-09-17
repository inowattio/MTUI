mod draw_state;
mod hints;
mod make_bottom_title;
mod make_top_title;
mod render;
mod rows_table;
#[cfg(not(target_arch = "wasm32"))]
mod terminal;
pub mod theme;

pub use render::render;
#[cfg(not(target_arch = "wasm32"))]
pub use terminal::Tui;

#[cfg(test)]
pub(crate) mod test_util {
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::{Frame, Terminal};

    pub(crate) fn buffer_rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).map_or(" ", |c| c.symbol()))
                    .collect()
            })
            .collect()
    }

    pub(crate) fn draw_rows(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(draw).unwrap();
        buffer_rows(terminal.backend().buffer())
    }
}
