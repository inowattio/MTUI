use crate::app::App;
use crate::input::{KeyCode, KeyEvent};

pub(super) fn handle_logs_view_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    match key_event.code {
        c if c == KeyCode::Esc || c == kb.app_logs => app.close_log_view(),
        KeyCode::Up => app.log_view_scroll(-1),
        KeyCode::Down => app.log_view_scroll(1),
        c if c == kb.page_up => app.log_view_scroll(-(app.layout.visible_rows as i32)),
        c if c == kb.page_down => app.log_view_scroll(app.layout.visible_rows as i32),
        c if c == kb.write => app.log_view_toggle_wrap(),
        c if c == kb.panel => app.log_view_cycle_mode(),
        c if c == kb.copy_column => app.copy_app_logs(),
        c if c == kb.dump => app.dump_app_logs(),
        KeyCode::Left => app.log_view_hscroll(false),
        KeyCode::Right => app.log_view_hscroll(true),
        _ => {}
    }
}

pub(super) fn handle_diff_view_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let page = app.layout.visible_rows as isize;
    match key_event.code {
        KeyCode::Esc => app.close_diff_view(),
        c if c == kb.panel => app.diff_toggle_changed(),
        KeyCode::Up => app.diff_scroll(-1),
        KeyCode::Down => app.diff_scroll(1),
        c if c == kb.page_up => app.diff_scroll(-page),
        c if c == kb.page_down => app.diff_scroll(page),
        KeyCode::Left => app.diff_hscroll(false),
        KeyCode::Right => app.diff_hscroll(true),
        _ => {}
    }
}

pub(super) fn handle_library_view_key(key_event: KeyEvent, app: &mut App) {
    match key_event.code {
        KeyCode::Esc => app.library_back(),
        KeyCode::Enter => app.library_commit(),
        KeyCode::Up => app.library_move(-1),
        KeyCode::Down => app.library_move(1),
        KeyCode::PageUp => app.library_page(false),
        KeyCode::PageDown => app.library_page(true),
        KeyCode::Home => app.library_move(i32::MIN),
        KeyCode::End => app.library_move(i32::MAX),
        KeyCode::Backspace => app.library_backspace(),
        KeyCode::Char(c) => app.library_input(c),
        _ => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::handler::tests::{DUMP, app, paste, screen};

    #[tokio::test]
    async fn the_diff_screen_renders_with_nothing_read() {
        let mut app = app().await;
        paste(&mut app, DUMP);
        let screen = screen(&mut app);
        assert!(
            screen.contains("0 changed, 0 same, 2 unread | Diff"),
            "{screen}"
        );
        assert!(screen.contains("[Tab] Changed"), "{screen}");
    }
}
