use super::App;
use crate::state::{DumpParams, Popup, StatusMessage};
use chrono::{Local, Utc};
use std::fs;

impl App {
    pub fn open_dump(&mut self) {
        self.read_mut().popup = Some(Popup::Dump(DumpParams::default()));
    }

    fn dump_read_log(&self) -> StatusMessage {
        if self.read_log.is_empty() {
            return StatusMessage::info("Nothing read yet to dump.");
        }

        let now = Local::now();
        let read_now = Utc::now();
        let filename = format!("dump_{}.txt", now.format("%Y%m%d_%H%M%S"));

        let mut out = String::new();
        let mut last_kind = None;
        for &cell in self.read_log.keys() {
            if last_kind != Some(cell.0) {
                if last_kind.is_some() {
                    out.push('\n');
                }
                out.push_str(&format!("{:?}\n{}\n", cell.0, self.interpreter.header()));
                last_kind = Some(cell.0);
            }
            if let Some((row, _)) = self.cell_row(cell, read_now) {
                out.push_str(row.trim_end());
                out.push('\n');
            }
        }

        match fs::write(&filename, out) {
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
