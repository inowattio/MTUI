use crate::app::App;
use crate::config::{KeybindAction, Keybinds};
use crate::input::{KeyCode, KeyEvent};
use crate::num_ops::wrap_index;
use crate::state::{SettingsCategory, SettingsField, SettingsFocus, StatusMessage};

pub(super) fn handle_settings_key(key_event: KeyEvent, app: &mut App) {
    dispatch_settings_key(key_event, app);
    if let Some(s) = app.settings_mut() {
        s.drop_api_drafts_if_left();
    }
}

fn dispatch_settings_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let capturing = app.settings().is_some_and(|s| s.kb_capturing);
    if !capturing && key_event.code == kb.panel {
        let no_changes = app.config_changes().is_empty();
        if let Some(s) = app.settings_mut() {
            s.cycle_category();
            if no_changes && s.current_category().is_changes() {
                s.focus = SettingsFocus::Categories;
            }
        }
        return;
    }
    match app
        .settings()
        .map_or(SettingsFocus::Categories, |s| s.focus)
    {
        SettingsFocus::Categories => handle_settings_category_key(key_event, app),
        SettingsFocus::Fields
            if app
                .settings()
                .is_some_and(|s| s.current_category().is_keybinds()) =>
        {
            handle_keybinds_key(key_event, app);
        }
        SettingsFocus::Fields
            if app
                .settings()
                .is_some_and(|s| s.current_category().is_changes()) =>
        {
            handle_changes_key(key_event, app);
        }
        SettingsFocus::Fields => handle_settings_field_key(key_event, app),
    }
}

fn handle_settings_category_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = SettingsCategory::ALL.len() as u16;
    let searching = app
        .settings()
        .is_some_and(|s| s.current_category().is_search());

    match key_event.code {
        KeyCode::Char(c) if searching => {
            if let Some(s) = app.settings_mut() {
                s.query_push(c);
            }
        }
        KeyCode::Backspace if searching => {
            if let Some(s) = app.settings_mut() {
                s.query_pop();
            }
        }
        c if c == KeyCode::Esc || c == kb.settings => app.close_settings(),
        KeyCode::Up => {
            if let Some(s) = app.settings_mut() {
                s.category = wrap_index(s.category, count, false);
                s.kb_selected = 0;
            }
        }
        KeyCode::Down => {
            if let Some(s) = app.settings_mut() {
                s.category = wrap_index(s.category, count, true);
                s.kb_selected = 0;
            }
        }
        KeyCode::Enter | KeyCode::Right => {
            let no_changes = app
                .settings()
                .is_some_and(|s| s.current_category().is_changes())
                && app.config_changes().is_empty();
            if !no_changes && let Some(s) = app.settings_mut() {
                s.enter_category();
            }
        }
        _ => {}
    }
}

fn handle_settings_field_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = app
        .settings()
        .map_or(0, |s| s.current_fields().len() as u16);
    let Some(field) = app
        .settings()
        .and_then(crate::state::SettingsParams::current_field)
    else {
        if key_event.code == KeyCode::Esc
            && let Some(s) = app.settings_mut()
        {
            s.focus = SettingsFocus::Categories;
        }
        return;
    };

    match key_event.code {
        KeyCode::Esc => {
            if let Some(s) = app.settings_mut() {
                s.focus = SettingsFocus::Categories;
            }
        }
        c if c == kb.settings && !field.is_text_input() => app.close_settings(),
        KeyCode::Up => {
            if let Some(s) = app.settings_mut() {
                s.field = wrap_index(s.field, count, false);
            }
        }
        KeyCode::Down => {
            if let Some(s) = app.settings_mut() {
                s.field = wrap_index(s.field, count, true);
            }
        }
        KeyCode::Left => app.settings_adjust(field, -1),
        KeyCode::Right => app.settings_adjust(field, 1),
        c if c == kb.pause && field.is_toggle() => app.settings_adjust(field, 1),
        KeyCode::Enter => match field {
            SettingsField::ClearPins => app.clear_pins(),
            SettingsField::ClearLabels => app.clear_labels(),
            SettingsField::ClearCustom => app.clear_custom(),
            SettingsField::Export => app.export(),
            SettingsField::ClearLibraryCache => app.clear_library_cache(),
            f if f.is_toggle() || f.is_theme_color() => app.settings_adjust(f, 1),
            SettingsField::Save => app.settings_save(),
            SettingsField::LoadConfig => app.settings_load(),
            SettingsField::ApiPort => app.apply_api_port(),
            SettingsField::ApiHost => app.apply_api_host(),
            _ => {}
        },
        KeyCode::Backspace => app.settings_backspace(field),
        KeyCode::Char(c) if field.is_text_input() => app.settings_text_input(field, c),
        KeyCode::Char(c) if field == SettingsField::StartupAddress => app.settings_address_digit(c),
        KeyCode::Char(c) if c.is_ascii_digit() => app.settings_digit(field, c as u8 - b'0'),
        _ => {}
    }
}

fn handle_changes_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    if key_event.code == kb.settings {
        app.close_settings();
        return;
    }
    let count = u16::try_from(app.config_changes().len()).unwrap_or(u16::MAX);
    let Some(s) = app.settings_mut() else {
        return;
    };
    match key_event.code {
        KeyCode::Esc => s.focus = SettingsFocus::Categories,
        KeyCode::Up => s.kb_move(true, count),
        KeyCode::Down => s.kb_move(false, count),
        c if c == kb.page_up => s.kb_page(true, count),
        c if c == kb.page_down => s.kb_page(false, count),
        _ => {}
    }
}

fn handle_keybinds_key(key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    let count = KeybindAction::ALL.len() as u16;
    let selected = app.settings().map_or(0, |s| s.kb_selected) as usize;

    if app.settings().is_some_and(|s| s.kb_capturing) {
        if let Some(s) = app.settings_mut() {
            s.kb_capturing = false;
        }
        if key_event.code == KeyCode::Esc {
            return;
        }
        if key_event.code.is_reserved() {
            app.set_settings_status(StatusMessage::warn(format!(
                "{} is reserved and cannot be bound",
                key_event.code
            )));
            return;
        }
        if let Some(&action) = KeybindAction::ALL.get(selected) {
            app.config.keybinds.set(action, key_event.code);
            app.refresh_dirty();
        }
        return;
    }

    match key_event.code {
        KeyCode::Esc => {
            if let Some(s) = app.settings_mut() {
                s.focus = SettingsFocus::Categories;
            }
        }
        KeyCode::Up => {
            if let Some(s) = app.settings_mut() {
                s.kb_move(true, count);
            }
        }
        KeyCode::Down => {
            if let Some(s) = app.settings_mut() {
                s.kb_move(false, count);
            }
        }
        c if c == kb.page_up => {
            if let Some(s) = app.settings_mut() {
                s.kb_page(true, count);
            }
        }
        c if c == kb.page_down => {
            if let Some(s) = app.settings_mut() {
                s.kb_page(false, count);
            }
        }
        KeyCode::Enter => {
            if let Some(s) = app.settings_mut() {
                s.kb_capturing = true;
            }
        }
        KeyCode::Backspace => {
            if let Some(&action) = KeybindAction::ALL.get(selected) {
                let default = Keybinds::default().get(action);
                app.config.keybinds.set(action, default);
                app.refresh_dirty();
            }
        }
        _ => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::config::{AddressMode, KeybindAction};
    use crate::handler::handle_key_events;
    use crate::handler::tests::app;
    use crate::input::{KeyCode, KeyEvent};
    use crate::state::{MessageKind, SettingsCategory, SettingsField, SettingsFocus};

    #[tokio::test]
    async fn rebinding_rejects_reserved_keys_but_accepts_others() {
        let mut app = app().await;
        app.open_settings();
        let keybinds = SettingsCategory::ALL
            .iter()
            .position(|&c| c == SettingsCategory::Keybinds)
            .unwrap() as u16;
        let action = KeybindAction::ALL[0];
        let before = app.config.keybinds.get(action);
        {
            let s = app.settings_mut().unwrap();
            s.category = keybinds;
            s.focus = SettingsFocus::Fields;
            s.kb_selected = 0;
            s.kb_capturing = true;
        }

        handle_key_events(KeyEvent::new(KeyCode::Up), &mut app);
        let s = app.settings().unwrap();
        assert!(!s.kb_capturing, "capture ends on a reserved key");
        assert_eq!(
            app.config.keybinds.get(action),
            before,
            "binding is untouched"
        );
        assert_eq!(
            s.status.as_ref().map(|m| m.kind),
            Some(MessageKind::Warn),
            "the user is told why"
        );

        app.settings_mut().unwrap().kb_capturing = true;
        handle_key_events(KeyEvent::new(KeyCode::Char('z')), &mut app);
        assert_eq!(app.config.keybinds.get(action), KeyCode::Char('z'));
        assert!(app.dirty);
    }

    #[tokio::test]
    async fn the_search_category_filters_settings_as_you_type() {
        let mut app = app().await;
        app.open_settings();
        app.settings_mut().unwrap().category = SettingsCategory::ALL.len() as u16 - 1;

        for c in "clock".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        let s = app.settings().unwrap();
        assert_eq!(s.query, "clock");
        assert_eq!(
            s.current_fields(),
            vec![SettingsField::ShowClock, SettingsField::ClockMillis]
        );
        assert_eq!(s.focus, SettingsFocus::Categories);

        let before = app.config.display.clock;
        handle_key_events(KeyEvent::new(KeyCode::Enter), &mut app);
        assert_eq!(app.settings().unwrap().focus, SettingsFocus::Fields);
        handle_key_events(KeyEvent::new(KeyCode::Enter), &mut app);
        assert_ne!(app.config.display.clock, before);

        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        assert_eq!(app.settings().unwrap().query, "cloc");
    }

    #[tokio::test]
    async fn startup_fields_are_locked_while_the_position_is_saved_on_exit() {
        let mut app = app().await;
        app.open_settings();
        let category = SettingsCategory::ALL
            .iter()
            .position(|&c| c == SettingsCategory::Data)
            .unwrap() as u16;
        let index = SettingsCategory::Data
            .fields()
            .iter()
            .position(|&f| f == SettingsField::StartupAddress)
            .unwrap() as u16;
        {
            let s = app.settings_mut().unwrap();
            s.category = category;
            s.focus = SettingsFocus::Fields;
            s.field = index;
        }
        let address = app.config.startup.address;

        app.config.save_position_on_exit = true;
        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Char('7')), &mut app);
        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        assert_eq!(
            app.config.startup.address, address,
            "locked fields ignore edits"
        );

        app.config.save_position_on_exit = false;
        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(app.config.startup.address, address + 1);
    }

    #[tokio::test]
    async fn clock_millis_is_locked_while_the_clock_is_hidden() {
        let mut app = app().await;
        app.open_settings();
        let category = SettingsCategory::ALL
            .iter()
            .position(|&c| c == SettingsCategory::Display)
            .unwrap() as u16;
        let index = SettingsCategory::Display
            .fields()
            .iter()
            .position(|&f| f == SettingsField::ClockMillis)
            .unwrap() as u16;
        {
            let s = app.settings_mut().unwrap();
            s.category = category;
            s.focus = SettingsFocus::Fields;
            s.field = index;
        }
        let millis = app.config.display.clock_millis;

        app.config.display.clock = false;
        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(
            app.config.display.clock_millis, millis,
            "locked fields ignore edits"
        );

        app.config.display.clock = true;
        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_ne!(app.config.display.clock_millis, millis);
    }

    #[tokio::test]
    async fn the_startup_address_is_typed_in_the_address_mode() {
        let mut app = app().await;
        app.open_settings();
        let category = SettingsCategory::ALL
            .iter()
            .position(|&c| c == SettingsCategory::Data)
            .unwrap() as u16;
        let index = SettingsCategory::Data
            .fields()
            .iter()
            .position(|&f| f == SettingsField::StartupAddress)
            .unwrap() as u16;
        {
            let s = app.settings_mut().unwrap();
            s.category = category;
            s.focus = SettingsFocus::Fields;
            s.field = index;
        }
        app.config.save_position_on_exit = false;

        app.config.startup.address = 0;
        for c in "200".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.config.startup.address, 200);
        for c in "f".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(
            app.config.startup.address, 200,
            "decimal ignores hex digits"
        );

        app.interpreter.set_address_mode(AddressMode::Hex);
        app.config.startup.address = 0;
        for c in "C8".chars() {
            handle_key_events(KeyEvent::new(KeyCode::Char(c)), &mut app);
        }
        assert_eq!(app.config.startup.address, 0xC8);
        handle_key_events(KeyEvent::new(KeyCode::Backspace), &mut app);
        assert_eq!(app.config.startup.address, 0xC);
    }
}
