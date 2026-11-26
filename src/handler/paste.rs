use crate::app::App;
use crate::state::{DiscoveryField, PopupKind, StatusMessage};

pub fn handle_paste(data: String, app: &mut App) {
    if app.settings().is_some() || app.log_view().is_some() {
        return;
    }

    let trimmed = data.trim();
    if trimmed.is_empty() {
        return;
    }

    if app.diff_view().is_some() {
        app.paste_into_diff_view(trimmed);
        return;
    }

    if app.library_view().is_some() {
        app.library_paste(trimmed);
        return;
    }

    if let Some(d) = app.discovery_mut() {
        if d.current_field() == DiscoveryField::CustomPath {
            let first_line = trimmed.lines().next().unwrap_or_default();
            d.custom_path.push_str(first_line);
            d.status = None;
        }
        return;
    }

    if app.popup_kind().is_none()
        && let Some(address) = app.interpreter.address_mode().parse(trimmed)
    {
        if app.read().panel.follows_position() {
            app.read_mut().position = address;
            app.scroll_to_cursor();
        }
        return;
    }

    if trimmed.bytes().all(|b| b.is_ascii_digit()) {
        paste_digits(trimmed, app);
        return;
    }

    if matches!(app.popup_kind(), None | Some(PopupKind::Dump)) {
        app.close_popup();
        app.paste_text(trimmed);
    }
}

fn paste_digits(digits: &str, app: &mut App) {
    match app.popup_kind() {
        Some(PopupKind::Write) => {
            if let Some(w) = app.write_mut() {
                w.value = Some(digits.parse().unwrap_or(i64::MAX));
            }
            app.clamp_write_value();
        }
        Some(PopupKind::Search) => {
            for c in digits.chars() {
                app.search_input(c);
            }
        }
        None if app.read().panel.follows_position() => {
            let mode = app.interpreter.address_mode();
            app.read_mut().position = mode.parse_saturating(digits).unwrap_or(u16::MAX);
            app.scroll_to_cursor();
            if mode.parse(digits).is_none() {
                app.set_read_status(StatusMessage::warn(format!(
                    "Pasted address is past {} - moved to the last one",
                    mode.display(u16::MAX)
                )));
            }
        }
        _ => {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::config::AddressMode;
    use crate::handler::handle_key_events;
    use crate::handler::tests::{DUMP, app, paste, snapshot_len, warning};
    use crate::input::{KeyCode, KeyEvent};
    use crate::register::RegisterType;
    use crate::scratch::ScratchDir;
    use crate::state::{PopupKind, ReadPanel, State};
    use std::assert_matches;

    #[tokio::test]
    async fn pasted_registers_json_still_opens_the_import_popup() {
        let mut app = app().await;
        paste(&mut app, r#"{"holdings":[{"address":3,"pinned":true}]}"#);
        assert_eq!(app.popup_kind(), Some(PopupKind::Import));
        assert_eq!(snapshot_len(&app), None);
    }

    #[tokio::test]
    async fn a_pasted_dump_opens_the_diff_screen_and_escape_returns_unchanged() {
        let mut app = app().await;
        {
            let read = app.read_mut();
            read.position = 42;
            read.panel = ReadPanel::Pinned;
        }

        paste(&mut app, DUMP);
        assert_eq!(snapshot_len(&app), Some(2));
        assert_matches!(app.state, State::Diff(_));

        let kb = app.config.keybinds;
        handle_key_events(KeyEvent::new(kb.panel), &mut app);
        assert!(
            app.diff_view().unwrap().changed_only,
            "the panel key switches tabs"
        );
        handle_key_events(KeyEvent::new(KeyCode::Char('w')), &mut app);
        assert!(app.diff_view().is_some(), "read keys do nothing here");

        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        let read = app.read();
        assert_eq!(read.position, 42);
        assert_eq!(read.panel, ReadPanel::Pinned);
        assert_eq!(read.popup, None);
        assert!(app.running, "escape closes the diff, not the app");
    }

    #[tokio::test]
    async fn a_dropped_dump_file_opens_the_diff_screen() {
        let scratch = ScratchDir::new("dropped-dump");
        let path = scratch.write("my dump.csv", DUMP);
        let escaped = path.replace(' ', "\\ ");
        let uri = format!("file://{}", path.replace(' ', "%20"));

        for pasted in [
            path.clone(),
            format!("'{path}'"),
            format!("\"{path}\""),
            escaped,
            uri,
        ] {
            let mut app = app().await;
            paste(&mut app, &format!("{pasted}\n"));
            assert_eq!(snapshot_len(&app), Some(2), "{pasted}");
        }
    }

    #[tokio::test]
    async fn a_dropped_file_that_is_no_dump_says_so() {
        let scratch = ScratchDir::new("dropped-other");
        let notes = scratch.write("notes.txt", "hello");
        let broken = scratch.write("broken.csv", "type,address,u16\ninput,1\n");

        let mut app = app().await;
        paste(&mut app, &notes);
        assert_eq!(
            warning(&app),
            format!("notes.txt: {}", crate::constants::message::NOT_A_DUMP)
        );
        paste(&mut app, &broken);
        assert_eq!(warning(&app), "broken.csv: Dump row on line 2 is malformed");
        paste(&mut app, &format!("{notes}.missing"));
        assert_eq!(
            warning(&app),
            crate::constants::message::PASTE_NOT_REGISTERS
        );
        assert_eq!(snapshot_len(&app), None);
    }

    #[tokio::test]
    async fn a_huge_dropped_file_is_refused_without_reading_it_all() {
        const HUGE: u64 = 1 << 30;
        let scratch = ScratchDir::new("dropped-huge");
        let huge = |file: &str, content: &str| {
            let path = scratch.write(file, content);
            std::fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_len(HUGE))
                .expect("sparse scratch file");
            path
        };
        let image = huge("disk.img", "hello");
        let dump = huge("huge.csv", DUMP);

        let mut app = app().await;
        paste(&mut app, &image);
        assert_eq!(
            warning(&app),
            format!("disk.img: {}", crate::constants::message::NOT_A_DUMP)
        );
        paste(&mut app, &dump);
        let refused = warning(&app);
        assert!(refused.starts_with("Couldn't read huge.csv: "), "{refused}");
        assert_eq!(snapshot_len(&app), None);
    }

    #[tokio::test]
    async fn a_dump_pasted_over_the_dump_popup_opens_the_diff_screen() {
        let mut app = app().await;
        app.open_dump();
        paste(&mut app, DUMP);
        assert_eq!(snapshot_len(&app), Some(2));
        handle_key_events(KeyEvent::new(KeyCode::Esc), &mut app);
        assert_eq!(app.popup_kind(), None, "the dump popup is gone");

        app.open_dump();
        paste(&mut app, "hello there");
        assert_eq!(app.popup_kind(), None);
        assert_eq!(
            warning(&app),
            crate::constants::message::PASTE_NOT_REGISTERS
        );

        app.open_help();
        paste(&mut app, DUMP);
        assert_eq!(
            app.popup_kind(),
            Some(PopupKind::Help),
            "other popups ignore it"
        );
        assert_eq!(snapshot_len(&app), None);
    }

    #[tokio::test]
    async fn a_pasted_address_jumps_in_the_address_mode() {
        let mut app = app().await;
        for (text, expected) in [("200", 200), ("0x1F", 0x1F), ("x1F", 0x1F)] {
            paste(&mut app, text);
            assert_eq!(app.read().position, expected, "{text:?}");
        }

        app.interpreter.set_address_mode(AddressMode::Hex);
        for (text, expected) in [
            ("200", 0x200),
            ("1F", 0x1F),
            ("0x1F", 0x1F),
            (" c8\n", 0xC8),
            ("n200", 200),
        ] {
            paste(&mut app, text);
            assert_eq!(app.read().position, expected, "{text:?}");
        }
        assert_eq!(app.popup_kind(), None);
        assert_eq!(snapshot_len(&app), None);
    }

    #[tokio::test]
    async fn unrecognised_or_broken_pastes_warn_instead_of_opening_the_diff() {
        use crate::constants::message;
        let mut app = app().await;
        for (text, expected) in [
            ("hello there", message::PASTE_NOT_REGISTERS.to_string()),
            (
                "type,address,label\ninput,1,x\n",
                message::DUMP_NO_RAW_COLUMN.to_string(),
            ),
            ("type,u16\ninput,1\n", message::DUMP_NO_ADDRESS.to_string()),
            (
                "type,address,u16\ninput,1,2\ninput,two,3\n",
                "Dump row on line 3 is malformed".to_string(),
            ),
        ] {
            paste(&mut app, text);
            assert_eq!(warning(&app), expected, "{text:?}");
            assert_eq!(snapshot_len(&app), None);
        }
        assert!(message::PASTE_NOT_REGISTERS.contains("dump"));
    }

    #[tokio::test]
    async fn a_second_dump_pasted_in_the_diff_screen_replaces_the_snapshot() {
        let mut app = app().await;
        paste(&mut app, DUMP);
        app.diff_view_mut().unwrap().top = 1;
        app.diff_view_mut().unwrap().h_offset = 8;

        paste(
            &mut app,
            "type,address,u16\nholding,1,1\nholding,2,2\nholding,3,3\n",
        );
        let d = app.diff_view().unwrap();
        assert_eq!(d.snapshot.len(), 3);
        assert_eq!((d.top, d.h_offset), (0, 0), "scrolling starts over");

        for ignored in [
            "12",
            "hello",
            r#"{"holdings":[{"address":3,"pinned":true}]}"#,
            "type,address,u16\ninput,x,1\n",
        ] {
            paste(&mut app, ignored);
            assert_eq!(snapshot_len(&app), Some(3), "{ignored:?} is ignored");
        }
    }

    #[tokio::test]
    async fn an_overlong_pasted_address_clamps_to_the_last_one() {
        let mut app = app().await;
        for text in ["655361", "70000", "99999999999999999999999"] {
            app.read_mut().position = 42;
            paste(&mut app, text);
            assert_eq!(app.read().position, u16::MAX, "{text}");
            assert!(warning(&app).contains("past 65535"), "{text}");
        }
    }

    #[tokio::test]
    async fn an_in_range_pasted_address_warns_nothing() {
        let mut app = app().await;
        paste(&mut app, "65535");
        assert_eq!(app.read().position, u16::MAX);
        assert!(app.read().status.is_none());
    }

    #[tokio::test]
    async fn an_overlong_pasted_write_value_clamps_to_the_max() {
        let mut app = app().await;
        app.read_mut().register_type = RegisterType::Holding;
        app.open_write();
        assert_eq!(app.popup_kind(), Some(PopupKind::Write));
        for text in ["655361", "99999999999999999999999"] {
            paste(&mut app, text);
            assert_eq!(
                app.write_mut().unwrap().value,
                Some(i64::from(u16::MAX)),
                "{text}"
            );
        }
    }
}
