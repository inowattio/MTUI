use super::discovery::handle_discovery_key;
use super::{move_read_cursor, run_action};
use crate::app::App;
use crate::input::{KeyCode, KeyEvent};
use crate::num_ops::{digit_add, digit_remove};
use crate::state::{
    CustomParams, FunctionsParams, PopupKind, SweepConfigParams, SweepField, UnitField, UnitParams,
};

pub(super) fn handle_popup_key(kind: PopupKind, key_event: KeyEvent, app: &mut App) {
    let kb = app.config.keybinds;
    match kind {
        PopupKind::Discovery => handle_discovery_key(key_event, app),

        PopupKind::Help => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => {
                if let Some(action) = app.help_commit() {
                    run_action(app, action);
                }
            }
            KeyCode::Up => app.help_move(false),
            KeyCode::Down => app.help_move(true),
            KeyCode::Backspace => app.help_backspace(),
            KeyCode::Char(c) => app.help_input(c),
            _ => {}
        },

        PopupKind::About => match key_event.code {
            c if c == KeyCode::Esc || c == kb.about => app.close_popup(),
            _ => {}
        },

        PopupKind::Stats => match key_event.code {
            c if c == KeyCode::Esc || c == kb.stats => app.close_popup(),
            c if c == kb.clear_session => app.clear_session_data(),
            c if c == kb.panel => app.stats_cycle(),
            _ => {}
        },

        PopupKind::Inspect => match key_event.code {
            c if c == KeyCode::Esc || c == kb.inspect => app.close_popup(),
            c if c == kb.refresh => app.refresh(),
            c if c == kb.word_order => app.toggle_word_order(),
            KeyCode::Left => app.inspect_cycle(false),
            KeyCode::Right => app.inspect_cycle(true),
            KeyCode::Up | KeyCode::Down => move_read_cursor(app, key_event.code),
            c if c == kb.page_up || c == kb.page_down => {
                move_read_cursor(app, key_event.code);
            }
            _ => {}
        },

        PopupKind::Functions => {
            let detail = app
                .popup_as::<FunctionsParams>()
                .is_some_and(|params| params.detail);
            if detail {
                match key_event.code {
                    KeyCode::Esc | KeyCode::Tab => app.functions_focus(false),
                    KeyCode::Char(c) if app.function_accepts(c) => app.function_input(c),
                    KeyCode::Enter => app.function_send(),
                    KeyCode::Up => app.function_move(false),
                    KeyCode::Down => app.function_move(true),
                    KeyCode::Left => app.function_cycle(false),
                    KeyCode::Right => app.function_cycle(true),
                    c if c == kb.page_up => app.function_scroll(false),
                    c if c == kb.page_down => app.function_scroll(true),
                    KeyCode::Backspace => app.function_backspace(),
                    KeyCode::Char(c) => app.function_input(c),
                    _ => {}
                }
            } else {
                match key_event.code {
                    KeyCode::Right | KeyCode::Tab => app.functions_focus(true),
                    c if c == KeyCode::Esc || c == kb.other_functions => app.close_popup(),
                    KeyCode::Up => app.functions_select(false),
                    KeyCode::Down => app.functions_select(true),
                    KeyCode::Enter => app.functions_enter(),
                    c if c == kb.refresh => app.function_send(),
                    c if c == kb.page_up => app.function_scroll(false),
                    c if c == kb.page_down => app.function_scroll(true),
                    _ => {}
                }
            }
        }

        PopupKind::Raw => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => app.raw_send(),
            KeyCode::Up | KeyCode::Down => app.raw_move(),
            KeyCode::Backspace => app.raw_backspace(),
            KeyCode::Char(c) => app.raw_input(c),
            _ => {}
        },

        PopupKind::Dump => match key_event.code {
            KeyCode::Enter => app.commit_dump(),
            c if c == KeyCode::Esc || c == kb.dump => app.close_popup(),
            _ => {}
        },

        PopupKind::Columns => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => app.columns_toggle_selected(),
            KeyCode::Up => app.columns_move(false),
            KeyCode::Down => app.columns_move(true),
            KeyCode::Left => app.columns_switch(false),
            KeyCode::Right => app.columns_switch(true),
            KeyCode::Backspace => app.columns_backspace(),
            KeyCode::Char(',') => app.columns_move_selected(false),
            KeyCode::Char('.') => app.columns_move_selected(true),
            KeyCode::Char(c) => app.columns_input(c),
            _ => {}
        },

        PopupKind::Write => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => app.commit_write(),
            c if c == kb.write => app.write_toggle_type(),
            KeyCode::Tab => app.write_toggle_raw(),
            KeyCode::Up => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(|v| v.checked_add(1));
                }
                app.clamp_write_value();
            }
            KeyCode::Down => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(|v| v.checked_sub(1));
                }
                app.clamp_write_value();
            }
            KeyCode::Left => app.write_move_bit(true),
            KeyCode::Right => app.write_move_bit(false),
            c if c == kb.pause => app.write_toggle_bit(),
            KeyCode::Char('-') => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.and_then(i64::checked_neg);
                }
                app.clamp_write_value();
            }
            KeyCode::Backspace => {
                if let Some(w) = app.write_mut() {
                    w.value = w.value.map(|mut n| {
                        digit_remove(&mut n);
                        n
                    });
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c as u8 - b'0';
                if let Some(w) = app.write_mut() {
                    let mut n = w.value.unwrap_or_default();
                    digit_add(&mut n, digit);
                    w.value = Some(n);
                }
                app.clamp_write_value();
            }
            _ => {}
        },

        PopupKind::Search => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => app.search_commit(),
            KeyCode::Up => app.search_move(false),
            KeyCode::Down => app.search_move(true),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(c) => app.search_input(c),
            _ => {}
        },

        PopupKind::Label => match key_event.code {
            KeyCode::Esc => app.close_popup(),
            KeyCode::Enter => app.commit_label(),
            KeyCode::Delete => app.remove_label(),
            KeyCode::Backspace => app.label_backspace(),
            KeyCode::Char(c) => app.label_input(c),
            _ => {}
        },

        PopupKind::Custom => {
            let Some((field, in_list)) = app
                .popup_as::<CustomParams>()
                .map(|c| (c.current_field(), c.list_index.is_some()))
            else {
                return;
            };
            match key_event.code {
                KeyCode::Tab => app.custom_toggle_list(),
                KeyCode::Esc if in_list => app.custom_toggle_list(),
                KeyCode::Esc => app.close_popup(),
                KeyCode::Up => app.custom_move(false),
                KeyCode::Down => app.custom_move(true),
                KeyCode::Left => app.custom_cycle(field, false),
                KeyCode::Right => app.custom_cycle(field, true),
                KeyCode::Delete => app.remove_custom(),
                KeyCode::Enter => app.custom_enter(field),
                KeyCode::Backspace if in_list => app.custom_list_remove(),
                KeyCode::Backspace => app.custom_backspace(field),
                KeyCode::Char(c) => app.custom_char(field, c),
                _ => {}
            }
        }

        PopupKind::Unit => {
            let Some(field) = app.popup_as::<UnitParams>().map(UnitParams::current_field) else {
                return;
            };
            match key_event.code {
                c if c == KeyCode::Esc || c == kb.unit_id => app.close_popup(),
                KeyCode::Enter => match field {
                    UnitField::Id => app.commit_unit(),
                    UnitField::Hit(index) => app.commit_unit_hit(index),
                    UnitField::Repr | UnitField::Exceptions => app.unit_toggle(field),
                    UnitField::From | UnitField::To | UnitField::Mode | UnitField::Scan => {
                        app.unit_scan_action();
                    }
                },
                KeyCode::Up => app.unit_move(false),
                KeyCode::Down => app.unit_move(true),
                KeyCode::Tab => app.unit_switch_column(),
                c if c == kb.pause && field.is_toggle() => app.unit_toggle(field),
                KeyCode::Left | KeyCode::Right if field.is_toggle() => app.unit_toggle(field),
                KeyCode::Backspace => app.unit_backspace(field),
                KeyCode::Char(c) if c.is_ascii_digit() => app.unit_digit(field, c),
                _ => {}
            }
        }

        PopupKind::SweepConfig => {
            let Some(field) = app
                .popup_as::<SweepConfigParams>()
                .map(SweepConfigParams::current_field)
            else {
                return;
            };
            match key_event.code {
                c if c == KeyCode::Esc || c == kb.sweep => app.close_popup(),
                KeyCode::Enter => app.sweep_action(),
                KeyCode::Up => app.sweep_config_move(false),
                KeyCode::Down => app.sweep_config_move(true),
                c if c == kb.pause && field == SweepField::Mode => app.sweep_config_toggle(),
                KeyCode::Left | KeyCode::Right if field == SweepField::Mode => {
                    app.sweep_config_toggle();
                }
                KeyCode::Backspace => app.sweep_config_backspace(field),
                KeyCode::Char(c) if c.is_ascii_hexdigit() => app.sweep_config_digit(field, c),
                _ => {}
            }
        }

        PopupKind::LibraryDisclaimer => match key_event.code {
            KeyCode::Enter => app.accept_library_disclaimer(),
            KeyCode::Esc | KeyCode::Backspace => app.close_popup(),
            _ => {}
        },

        PopupKind::NoLibraries => match key_event.code {
            KeyCode::Enter | KeyCode::Esc => app.close_popup(),
            c if c == kb.settings => app.open_library_settings(),
            _ => {}
        },

        PopupKind::Import | PopupKind::LoadConfig | PopupKind::Quit => {
            type Action = fn(&mut App);
            let (confirm, cancel): (Action, Action) = match kind {
                PopupKind::Import => (App::apply_import, App::cancel_import),
                PopupKind::LoadConfig => (App::confirm_load_config, App::close_popup),
                _ => (App::quit, App::close_popup),
            };
            match key_event.code {
                KeyCode::Enter => confirm(app),
                KeyCode::Up if kind == PopupKind::Import => app.import_move(false),
                KeyCode::Down if kind == PopupKind::Import => app.import_move(true),
                KeyCode::Char(c) if kind == PopupKind::Import => app.import_input(c),
                KeyCode::Backspace if kind == PopupKind::Import && app.editing_import_text() => {
                    app.import_backspace();
                }
                KeyCode::Char('s') if kind == PopupKind::Quit => app.save_and_quit(),
                KeyCode::Char('s') if kind == PopupKind::LoadConfig => {
                    app.save_and_load_config();
                }
                c if c == KeyCode::Esc || c == KeyCode::Backspace => cancel(app),
                _ => {}
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{App, StatTarget};
    use crate::handler::handle_key_events;
    use crate::handler::tests::{app, screen};
    use crate::input::{KeyCode, KeyEvent};
    use crate::register::RegisterType;
    use crate::state::{
        PopupKind, SettingsCategory, SettingsFocus, StatsTab, UnitField, UnitParams,
    };

    #[tokio::test]
    async fn an_empty_search_cannot_trap_the_cursor_in_the_fields_pane() {
        let mut app = app().await;
        app.open_settings();
        app.settings_mut().unwrap().category = SettingsCategory::ALL.len() as u16 - 1;

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(app.settings().unwrap().focus, SettingsFocus::Categories);
        handle_key_events(KeyEvent::new(KeyCode::Enter), &mut app);
        assert_eq!(app.settings().unwrap().focus, SettingsFocus::Categories);

        {
            let s = app.settings_mut().unwrap();
            s.focus = SettingsFocus::Fields;
        }
        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        assert_eq!(app.settings().unwrap().focus, SettingsFocus::Categories);
    }

    #[tokio::test]
    async fn order_keys_reorder_columns_instead_of_typing() {
        let mut app = app().await;
        app.open_columns();
        let first = app.interpreter.ordered_columns()[0];

        handle_key_events(KeyEvent::new(KeyCode::Char('.')), &mut app);

        assert_eq!(app.interpreter.ordered_columns()[1], first, "moved right");
        assert!(app.dirty, "the order is part of the config");
        assert_eq!(
            app.popup_as::<crate::state::ColumnsParams>()
                .map(|p| p.query.as_str()),
            Some(""),
            "the order key is not typed into the search"
        );

        handle_key_events(KeyEvent::new(KeyCode::Char(',')), &mut app);
        assert_eq!(app.interpreter.ordered_columns()[0], first, "and back");
    }

    #[tokio::test]
    async fn the_stats_popup_clears_the_counters_in_place() {
        let mut app = app().await;
        app.stats()
            .reads
            .record_ok(std::time::Duration::from_millis(5));
        app.open_stats();
        assert_eq!(app.popup_kind(), Some(PopupKind::Stats));

        handle_key_events(KeyEvent::new(app.config.keybinds.clear_session), &mut app);
        assert_eq!(app.stats().reads.ok, 0);
        assert_eq!(app.stats().reads.latency(), None);
        assert_eq!(
            app.popup_kind(),
            Some(PopupKind::Stats),
            "the popup stays open to show the reset"
        );
    }

    #[tokio::test]
    async fn the_stats_popup_cycles_its_tabs_with_the_panel_key() {
        let mut app = app().await;
        let panel = app.config.keybinds.panel;
        app.open_stats();
        assert_eq!(app.popup_as::<StatsTab>(), Some(&StatsTab::All));

        handle_key_events(KeyEvent::new(panel), &mut app);
        assert_eq!(app.popup_as::<StatsTab>(), Some(&StatsTab::Reads));

        handle_key_events(KeyEvent::new(panel), &mut app);
        handle_key_events(KeyEvent::new(panel), &mut app);
        assert_eq!(app.popup_as::<StatsTab>(), Some(&StatsTab::Others));

        handle_key_events(KeyEvent::new(panel), &mut app);
        assert_eq!(
            app.popup_as::<StatsTab>(),
            Some(&StatsTab::All),
            "wraps around"
        );

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert_eq!(
            app.popup_as::<StatsTab>(),
            Some(&StatsTab::All),
            "arrows no longer switch tabs"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn the_stats_popup_names_where_the_last_error_happened() {
        let mut app = app().await;
        app.stats().writes.record_error(
            "Illegal data address",
            Some(StatTarget::Write(RegisterType::Holding, 16, 2)),
        );
        app.open_stats();

        app.stats()
            .reads
            .record_ok(std::time::Duration::from_micros(850));
        app.stats()
            .reads
            .record_ok(std::time::Duration::from_micros(1150));

        let all = screen(&mut app);
        assert!(all.contains("Latency  0.85 / 1.00 / 1.15 ms"), "{all}");
        assert!(all.contains("Illegal data address"), "{all}");
        assert!(all.contains("Holding 16-17 (write)"), "{all}");

        let panel = app.config.keybinds.panel;
        for _ in 0..3 {
            handle_key_events(KeyEvent::new(panel), &mut app);
        }
        let others = screen(&mut app);
        assert!(!others.contains("Illegal data address"), "{others}");
    }

    #[tokio::test]
    async fn the_unit_popup_toggles_ascii_hit_data() {
        let mut app = app().await;
        app.open_unit();
        let kb = app.config.keybinds;
        let index = UnitParams::default()
            .fields()
            .iter()
            .position(|&f| f == UnitField::Repr)
            .unwrap();
        for _ in 0..index {
            handle_key_events(KeyEvent::new(KeyCode::Down), &mut app);
        }
        fn unit(app: &App) -> &UnitParams {
            app.popup_as().unwrap()
        }
        assert_eq!(unit(&app).current_field(), UnitField::Repr);

        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        assert!(unit(&app).ascii, "arrows switch to ASCII");
        handle_key_events(KeyEvent::new(KeyCode::Enter), &mut app);
        assert!(!unit(&app).ascii, "enter switches back");
        assert!(
            !unit(&app).active(),
            "enter on this field never starts a scan"
        );

        handle_key_events(KeyEvent::new(KeyCode::Down), &mut app);
        assert_eq!(unit(&app).current_field(), UnitField::Exceptions);
        assert!(unit(&app).show_exceptions, "listed by default");
        handle_key_events(KeyEvent::new(kb.pause), &mut app);
        assert!(!unit(&app).show_exceptions, "space hides them");
        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        assert!(unit(&app).show_exceptions);
        assert!(!unit(&app).active());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn hiding_exceptions_applies_to_the_list_at_once() {
        use crate::register::RegisterType;
        let mut app = app().await;
        app.open_unit();
        {
            let p = app.popup_as_mut::<UnitParams>().unwrap();
            p.register_type = RegisterType::Input;
            p.address = 38;
            p.amount = 1;
            p.from = 1;
            p.to = 3;
        }
        app.unit_scan_action();
        crate::app::settle_until(
            &mut app,
            |app| !app.popup_as::<UnitParams>().unwrap().active(),
            "scan",
        )
        .await;
        assert_eq!(app.popup_as::<UnitParams>().unwrap().hits.len(), 3);

        let before = screen(&mut app);
        assert!(before.contains("Found 3"), "{before}");
        assert_eq!(before.matches("Illegal").count(), 3);

        let index = UnitParams::default()
            .fields()
            .iter()
            .position(|&f| f == UnitField::Exceptions)
            .unwrap();
        for _ in 0..index {
            handle_key_events(KeyEvent::new(KeyCode::Down), &mut app);
        }
        handle_key_events(KeyEvent::new(KeyCode::Right), &mut app);
        let hidden = screen(&mut app);
        assert!(hidden.contains("Found 0"), "{hidden}");
        assert!(!hidden.contains("Illegal"), "exceptions vanish at once");
        assert!(hidden.contains("(no hits)"));

        handle_key_events(KeyEvent::new(KeyCode::Left), &mut app);
        let shown = screen(&mut app);
        assert!(shown.contains("Found 3"), "{shown}");
        assert_eq!(shown.matches("Illegal").count(), 3);
    }
}
