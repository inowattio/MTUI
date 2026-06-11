mod draw_state;
mod make_bottom_title;
mod make_top_title;
mod render;
#[cfg(not(target_arch = "wasm32"))]
mod terminal;
pub mod theme;

pub use render::render;
#[cfg(not(target_arch = "wasm32"))]
pub use terminal::Tui;
