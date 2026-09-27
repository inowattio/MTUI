use super::App;
use crate::constants::message;
use crate::snapshot::{TYPE_COLUMN, csv_line};
use crate::state::{DumpParams, Popup, StatusMessage};
use chrono::{DateTime, Local, Utc};
use std::fs;

impl App {
    pub fn open_dump(&mut self) {
        self.read_mut().popup = Some(Popup::Dump(DumpParams::default()));
    }

    pub fn dump_csv(&self, now: DateTime<Utc>) -> String {
        let segments = self.interpreter.row_segments();
        let mut out = csv_line(
            std::iter::once(TYPE_COLUMN).chain(segments.iter().map(|segment| segment.name)),
        );
        for &cell in self.read_log.keys() {
            let Some((row, _)) = self.cell_row(cell, now) else {
                continue;
            };
            let fields: Vec<String> = segments.iter().map(|segment| segment.text(&row)).collect();
            out.push_str(&csv_line(
                std::iter::once(cell.0.name().to_lowercase().as_str())
                    .chain(fields.iter().map(String::as_str)),
            ));
        }
        out
    }

    fn dump_read_log(&self) -> StatusMessage {
        if self.read_log.is_empty() {
            return StatusMessage::info(message::NOTHING_TO_DUMP);
        }

        let now = Local::now();
        let read_now = Utc::now();
        let filename = super::file_name(
            &[
                "dump",
                &self.config.name,
                &now.format("%Y%m%d_%H%M%S").to_string(),
            ],
            "csv",
        );

        match fs::write(&filename, self.dump_csv(read_now)) {
            Ok(()) => StatusMessage::ok(format!("Saved to {filename}")),
            Err(e) => StatusMessage::err(format!("Dump failed: {e}")),
        }
    }

    pub fn commit_dump(&mut self) {
        let result = self.dump_read_log();
        if let Some(d) = self.popup_as_mut::<DumpParams>() {
            d.result = Some(result);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{App, settle_until};
    use crate::config::{AddressMode, Column, Config};
    use crate::register::RegisterType;
    use crate::snapshot::{Snapshot, parse_dump};
    use chrono::Utc;

    const RAW: [Column; 4] = [Column::U16, Column::Hex, Column::I16, Column::Bits];

    async fn read_app() -> App {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.read_mut().register_type = RegisterType::Input;
        app.refresh();
        settle_until(&mut app, |app| app.background_task.is_none(), "read").await;
        assert!(!app.read_log.is_empty(), "the mock answers the read");
        app
    }

    fn assert_matches_the_read_log(app: &App, snapshot: &Snapshot) {
        let read: Vec<_> = app
            .read_log
            .iter()
            .map(|(&cell, entry)| (cell, entry.value, Some(entry.time_text.to_string())))
            .collect();
        let parsed: Vec<_> = snapshot
            .iter()
            .map(|(&cell, entry)| (cell, entry.value, entry.time.clone()))
            .collect();
        assert_eq!(parsed, read);
    }

    #[tokio::test]
    async fn a_dump_parses_back_to_the_read_values() {
        let app = read_app().await;
        let dump = app.dump_csv(Utc::now());
        assert!(dump.starts_with("type,address,time,u16,"), "{dump}");

        let snapshot = parse_dump(&dump, app.interpreter.address_mode()).expect("a valid dump");
        assert_matches_the_read_log(&app, &snapshot);
    }

    #[tokio::test]
    async fn each_raw_column_alone_is_enough_to_recover_the_values() {
        let mut app = read_app().await;
        for kept in RAW {
            for column in RAW {
                if app.interpreter.is_enabled(column) != (column == kept) {
                    app.interpreter.toggle(column);
                }
            }
            let dump = app.dump_csv(Utc::now());
            let snapshot = parse_dump(&dump, app.interpreter.address_mode())
                .unwrap_or_else(|e| panic!("{}: {e}", kept.name()));
            assert_matches_the_read_log(&app, &snapshot);
        }
    }

    #[tokio::test]
    async fn a_hex_address_dump_parses_back_in_hex_mode() {
        let mut app = read_app().await;
        app.read_mut().position = 200;
        app.refresh();
        settle_until(&mut app, |app| app.background_task.is_none(), "read").await;
        app.interpreter.set_address_mode(AddressMode::Hex);

        let dump = app.dump_csv(Utc::now());
        assert!(dump.contains("\ninput,C8,"), "{dump}");
        let snapshot = parse_dump(&dump, AddressMode::Hex).expect("a valid dump");
        assert_matches_the_read_log(&app, &snapshot);
    }
}
