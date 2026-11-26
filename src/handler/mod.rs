mod discovery;
mod paste;
mod popups;
mod settings;
mod views;

use crate::app::App;
use crate::config::KeybindAction;
use crate::input::{KeyCode, KeyEvent};
use crate::state::{ReadPanel, StatusMessage};
pub use paste::handle_paste;
use popups::handle_popup_key;
use settings::handle_settings_key;
use views::{handle_diff_view_key, handle_library_view_key, handle_logs_view_key};

pub fn handle_key_events(key_event: KeyEvent, app: &mut App) {
    if key_event.is_ctrl_c() {
        app.interrupt();
        return;
    }

    if key_event.ctrl && matches!(key_event.code, KeyCode::Char(_)) {
        return;
    }

    let kb = app.config.keybinds;

    if app.settings().is_some() {
        handle_settings_key(key_event, app);
        return;
    }

    if app.log_view().is_some() {
        handle_logs_view_key(key_event, app);
        return;
    }

    if app.diff_view().is_some() {
        handle_diff_view_key(key_event, app);
        return;
    }

    if app.library_view().is_some() {
        handle_library_view_key(key_event, app);
        return;
    }

    if let Some(kind) = app.popup_kind() {
        handle_popup_key(kind, key_event, app);
        return;
    }

    if app.hex_letters() {
        match key_event.code {
            KeyCode::Char(c @ ('a'..='f' | 'A'..='F')) => return type_address_digit(app, c),
            KeyCode::Esc => return app.toggle_hex_letters(),
            _ => {}
        }
    }

    if app.read().graph && key_event.code == kb.dump {
        app.cycle_graph_interpretation();
        return;
    }

    if app.read().graph && key_event.code == kb.pin {
        app.graph_hold_series();
        return;
    }

    if app.read().copy_column.is_some() {
        match key_event.code {
            KeyCode::Left => return app.copy_column_move(false),
            KeyCode::Right => return app.copy_column_move(true),
            KeyCode::Enter => return app.copy_column_commit(),
            KeyCode::Esc => return app.copy_column_cancel(),
            c if c == kb.copy_column => return app.copy_column_commit(),
            KeyCode::Up | KeyCode::Down => {}
            _ => app.copy_column_cancel(),
        }
    }

    if let Some(action) = kb.action_for(key_event.code) {
        run_action(app, action);
        return;
    }

    match key_event.code {
        KeyCode::Esc => app.request_quit(),
        KeyCode::Up | KeyCode::Down => move_read_cursor(app, key_event.code),
        KeyCode::Left | KeyCode::Right if app.read().panel == ReadPanel::Matrix => {
            let p = app.read_mut();
            p.position = step_pos(p.position, key_event.code == KeyCode::Left, 1);
            app.scroll_to_cursor();
        }
        KeyCode::Left => app.scroll_columns(false),
        KeyCode::Right => app.scroll_columns(true),
        KeyCode::Char(c) if c.is_ascii_digit() && app.read().panel.follows_position() => {
            type_address_digit(app, c);
        }
        KeyCode::Backspace if app.read().panel.follows_position() => {
            let mode = app.typing_mode();
            let p = app.read_mut();
            p.position = mode.pop_digit(p.position);
            app.scroll_to_cursor();
        }
        _ => {}
    }
}

fn type_address_digit(app: &mut App, c: char) {
    let Some(position) = app.typing_mode().push_digit(app.read().position, c) else {
        app.set_read_status(StatusMessage::warn(format!(
            "Can't append {c} - address would go past {}",
            app.interpreter.address_mode().display(u16::MAX)
        )));
        return;
    };
    app.read_mut().position = position;
    app.scroll_to_cursor();
}

const fn step_pos(value: u16, up: bool, step: u16) -> u16 {
    if up {
        value.saturating_sub(step)
    } else {
        value.saturating_add(step)
    }
}

fn move_read_cursor(app: &mut App, code: KeyCode) {
    let rows = app.layout.visible_rows;
    let panel_len = app.panel_len();
    let cols = app.matrix_cols();
    let kb = app.config.keybinds;
    let step = if code == kb.page_up || code == kb.page_down {
        rows
    } else {
        1
    };
    let up = code == KeyCode::Up || code == kb.page_up;
    let scroll_rows = app.panel_scroll_rows();
    let p = app.read_mut();
    match p.panel {
        ReadPanel::Main => p.position = step_pos(p.position, up, step),
        ReadPanel::Matrix => {
            let step = step.saturating_mul(cols.max(1));
            p.position = step_pos(p.position, up, step);
        }
        _ => {
            p.pinned_index = step_pos(p.pinned_index, up, step);
            p.scroll_pinned(scroll_rows, panel_len);
            return;
        }
    }
    app.scroll_to_cursor();
}

fn run_action(app: &mut App, action: KeybindAction) {
    use KeybindAction::*;
    match action {
        Pin => app.pin(),
        Dump => app.open_dump(),
        Help => app.open_help(),
        About => app.open_about(),
        Refresh => app.refresh(),
        SwitchType => app.toggle_type(),
        Write => app.open_write(),
        GoTo => app.open_search(),
        Label => app.open_label(),
        CustomRule => app.open_custom(),
        Columns => app.open_columns(),
        Pause => app.toggle_pause(),
        WordOrder => app.toggle_word_order(),
        UnitId => app.open_unit(),
        Inspect => app.open_inspect(),
        OtherFunctions => app.open_functions(),
        DeviceId => app.open_device_id(),
        Library => app.open_library(),
        RawRequest => app.open_raw(),
        Graph => app.toggle_graph(),
        Device => app.open_discovery(),
        Settings => app.open_settings(),
        CopyColumn => app.copy_column_arm(),
        AppLogs => app.open_log_view(),
        Stats => app.open_stats(),
        Sweep => app.open_sweep(),
        ClearSession => {
            if app.read().graph {
                app.clear_graph_history();
            } else {
                app.clear_session_data();
            }
        }
        CycleConfig => app.cycle_config(),
        Panel => {
            app.toggle_panel();
            let len = app.panel_len();
            let scroll_rows = app.panel_scroll_rows();
            app.read_mut().scroll_pinned(scroll_rows, len);
            app.scroll_to_cursor();
        }
        BatchDecrease | BatchIncrease => app.adjust_batch(action == BatchIncrease),
        HexLetters => app.toggle_hex_letters(),
        PageUp | PageDown => {
            move_read_cursor(app, app.config.keybinds.get(action));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::handle_key_events;
    use crate::app::App;
    use crate::config::{AddressMode, Config};
    use crate::input::{KeyCode, KeyEvent};
    use crate::scratch::ScratchDir;
    use crate::state::{MessageKind, Popup, PopupKind, ReadPanel, State};
    use std::assert_matches;

    pub(super) async fn app() -> App {
        App::boot(Config::default(), String::new()).await
    }

    #[tokio::test]
    async fn address_typing_and_pasting_leave_the_position_alone_in_list_panels() {
        let mut app = app().await;
        app.read_mut().position = 5;

        for panel in [ReadPanel::Pinned, ReadPanel::Labeled, ReadPanel::Custom] {
            app.read_mut().panel = panel;
            handle_key_events(KeyEvent::new(KeyCode::Char('3')), &mut app);
            handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
            super::handle_paste("42".to_string(), &mut app);
            super::handle_paste("99999999".to_string(), &mut app);
            assert_eq!(app.read().position, 5, "{panel:?}");
        }

        app.read_mut().panel = ReadPanel::Main;
        handle_key_events(KeyEvent::new(KeyCode::Char('3')), &mut app);
        assert_eq!(app.read().position, 53);
    }

    #[tokio::test]
    async fn copy_column_walks_the_row_and_escape_only_leaves_the_mode() {
        let mut app = app().await;
        let kb = app.config.keybinds;

        handle_key_events(KeyEvent::new(kb.copy_column), &mut app);
        assert_eq!(
            app.copy_column_name(),
            Some("address"),
            "arms on the address"
        );

        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        assert_eq!(
            app.copy_column_name(),
            Some("address"),
            "the anchor is the left edge"
        );

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(app.copy_column_name(), Some("time"));

        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        assert_eq!(app.copy_column_name(), Some("address"));

        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        assert_eq!(app.copy_column_name(), None);
        assert!(app.running, "escape leaves the mode without quitting");
    }

    #[tokio::test]
    async fn copy_column_commits_on_enter_and_reports() {
        let mut app = app().await;
        let kb = app.config.keybinds;

        handle_key_events(KeyEvent::new(kb.copy_column), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Enter), &mut app);

        assert_eq!(app.copy_column_name(), None, "committing disarms");
        assert!(app.read().status.is_some(), "the copy reports an outcome");
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn screen(app: &mut App) -> String {
        crate::tui::test_util::draw_rows(120, 40, |frame| crate::tui::render(app, frame)).join("\n")
    }

    const fn ctrl(c: char) -> KeyEvent {
        KeyEvent::with_ctrl(KeyCode::Char(c), true)
    }

    #[tokio::test]
    async fn ctrl_c_quits_a_clean_session_at_once() {
        let mut app = app().await;
        handle_key_events(ctrl('c'), &mut app);
        assert!(!app.running);
    }

    #[tokio::test]
    async fn ctrl_c_asks_before_discarding_unsaved_changes() {
        let mut app = app().await;
        app.dirty = true;
        handle_key_events(ctrl('c'), &mut app);
        assert!(app.running, "unsaved changes must be confirmed first");
        assert_eq!(app.popup_kind(), Some(PopupKind::Quit));

        handle_key_events(ctrl('c'), &mut app);
        assert!(!app.running, "a second Ctrl+C confirms");
    }

    #[tokio::test]
    async fn ctrl_c_in_settings_returns_to_the_read_view_to_ask() {
        let mut app = app().await;
        app.open_settings();
        app.dirty = true;
        handle_key_events(ctrl('c'), &mut app);
        assert!(app.running);
        assert_matches!(&app.state, State::Read(p) if p.popup == Some(Popup::Quit));
    }

    #[tokio::test]
    async fn s_in_the_quit_prompt_saves_and_quits() {
        let scratch = ScratchDir::new("quit");
        let path = scratch.path("config.json");
        let mut app = App::boot(Config::default(), path.clone()).await;
        app.pin();
        assert!(app.dirty);
        let startup = app.config.startup;
        handle_key_events(KeyEvent::new(KeyCode::Down), &mut app);
        assert_ne!(app.read().position, startup.address);

        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        assert_eq!(app.popup_kind(), Some(PopupKind::Quit));

        handle_key_events(KeyEvent::new(KeyCode::Char('s')), &mut app);
        assert!(!app.running);
        assert!(!app.dirty);
        let saved: Config = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            saved
                .registers
                .inputs
                .iter()
                .filter(|e| e.pinned)
                .map(|e| e.address)
                .collect::<Vec<_>>(),
            app.pinned_registers.iter().map(|c| c.1).collect::<Vec<_>>()
        );
        assert_eq!(
            saved.startup.address, startup.address,
            "saving does not overwrite the configured startup position"
        );
    }

    #[tokio::test]
    async fn a_failed_save_keeps_the_session_and_reports_it() {
        let blocker = std::env::temp_dir().join(format!("mtui-blocker-{}", std::process::id()));
        std::fs::write(&blocker, b"").unwrap();
        let path = blocker.join("config.json");
        let mut app = App::boot(Config::default(), path).await;
        app.pin();
        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Char('s')), &mut app);
        assert!(app.running);
        assert!(app.dirty);
        assert_eq!(app.popup_kind(), None);
        assert_eq!(
            app.read().status.as_ref().map(|s| s.kind),
            Some(MessageKind::Err)
        );
        let _ = std::fs::remove_file(blocker);
    }

    #[tokio::test]
    async fn ignore_dirty_skips_the_prompt() {
        let mut app = app().await;
        app.dirty = true;
        app.config.skip_unsaved_warning = true;
        handle_key_events(ctrl('c'), &mut app);
        assert!(!app.running);
    }

    #[tokio::test]
    async fn ctrl_letters_do_not_fire_the_bare_letter_binding() {
        let mut app = app().await;
        handle_key_events(ctrl('w'), &mut app);
        assert_eq!(
            app.popup_kind(),
            None,
            "Ctrl+W must not open the write popup"
        );

        handle_key_events(KeyEvent::new(KeyCode::Char('c')), &mut app);
        assert_eq!(
            app.popup_kind(),
            Some(PopupKind::Columns),
            "plain c still opens Columns"
        );
    }

    pub(super) const DUMP: &str =
        "type,address,time,u16\ninput,0,12:00:00.000,1\ninput,1,12:00:00.000,2\n";

    pub(super) fn paste(app: &mut App, text: &str) {
        super::handle_paste(text.to_string(), app);
    }

    pub(super) fn snapshot_len(app: &App) -> Option<usize> {
        app.diff_view().map(|d| d.snapshot.len())
    }

    pub(super) fn warning(app: &App) -> String {
        let status = app.read().status.as_ref().expect("a status is shown");
        assert_eq!(status.kind, MessageKind::Warn, "{}", status.text);
        status.text.clone()
    }

    #[tokio::test]
    async fn ctrl_c_in_the_diff_screen_returns_to_the_read_view_to_ask() {
        let mut app = app().await;
        paste(&mut app, DUMP);
        app.dirty = true;
        handle_key_events(ctrl('c'), &mut app);
        assert!(app.running);
        assert_matches!(&app.state, State::Read(p) if p.popup == Some(Popup::Quit));
    }

    #[tokio::test]
    async fn a_typed_digit_past_the_last_address_warns_and_keeps_the_position() {
        let mut app = app().await;
        app.read_mut().position = 9999;
        handle_key_events(KeyEvent::new(KeyCode::Char('9')), &mut app);
        assert_eq!(app.read().position, 9999);
        assert!(warning(&app).contains("Can't append 9"));

        app.read_mut().status = None;
        app.read_mut().position = 6553;
        handle_key_events(KeyEvent::new(KeyCode::Char('5')), &mut app);
        assert_eq!(app.read().position, 65535);
        assert!(app.read().status.is_none());
    }

    #[tokio::test]
    async fn typed_digits_follow_the_hex_address_mode() {
        let mut app = app().await;
        app.interpreter.set_address_mode(AddressMode::Hex);
        app.read_mut().position = 0;
        for c in ['1', '0'] {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.read().position, 0x10);

        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        assert_eq!(app.read().position, 0x1);

        app.read_mut().position = 0x1000;
        handle_key_events(KeyEvent::new(KeyCode::Char('0')), &mut app);
        assert_eq!(app.read().position, 0x1000);
        assert!(warning(&app).contains("past FFFF"));
    }

    #[tokio::test]
    async fn backtick_toggles_hex_letters_as_address_digits() {
        let mut app = app().await;
        app.interpreter.set_address_mode(AddressMode::Hex);
        app.read_mut().position = 0;
        let kb = app.config.keybinds;

        handle_key_events(KeyEvent::new(kb.hex_letters), &mut app);
        assert!(app.hex_letters());
        let screen = screen(&mut app);
        let bottom = screen.lines().last().unwrap();
        assert!(bottom.contains("HEX "), "{bottom}");
        assert!(bottom.find("HEX") < bottom.find("Help"), "{bottom}");
        for c in ['1', 'a', 'B'] {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.read().position, 0x1AB);
        assert_eq!(app.popup_kind(), None, "a no longer opens About");

        handle_key_events(KeyEvent::new(kb.hex_letters), &mut app);
        assert!(!app.hex_letters());
        handle_key_events(KeyEvent::new(KeyCode::Char('a')), &mut app);
        assert_eq!(app.popup_kind(), Some(PopupKind::About));
    }

    #[tokio::test]
    async fn escape_leaves_hex_letters_before_quitting() {
        let mut app = app().await;
        app.interpreter.set_address_mode(AddressMode::Hex);
        app.toggle_hex_letters();
        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        assert!(!app.hex_letters());
        assert!(app.running);
        assert_eq!(app.popup_kind(), None);
    }

    #[tokio::test]
    async fn hex_letters_type_hex_digits_in_decimal_mode_too() {
        let mut app = app().await;
        app.read_mut().position = 0;
        app.toggle_hex_letters();
        for c in ['1', '0', 'f'] {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.read().position, 0x10F);

        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        assert_eq!(app.read().position, 0x10);

        app.read_mut().position = 0x1000;
        handle_key_events(KeyEvent::new(KeyCode::Char('a')), &mut app);
        assert_eq!(app.read().position, 0x1000);
        assert!(warning(&app).contains("past 65535"));
    }
}
