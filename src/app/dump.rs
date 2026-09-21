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
        let filename = super::file_name(
            &[
                "dump",
                &self.config.name,
                &now.format("%Y%m%d_%H%M%S").to_string(),
            ],
            "csv",
        );

        let segments = self.interpreter.row_segments();
        let mut out =
            csv_line(std::iter::once("type").chain(segments.iter().map(|segment| segment.name)));
        for &cell in self.read_log.keys() {
            let Some((row, _)) = self.cell_row(cell, read_now) else {
                continue;
            };
            let fields: Vec<String> = segments.iter().map(|segment| segment.text(&row)).collect();
            out.push_str(&csv_line(
                std::iter::once(cell.0.name().to_lowercase().as_str())
                    .chain(fields.iter().map(String::as_str)),
            ));
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

fn csv_line<'a>(fields: impl Iterator<Item = &'a str>) -> String {
    let mut line = fields.map(csv_field).collect::<Vec<_>>().join(",");
    line.push('\n');
    line
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::csv_line;

    #[test]
    fn csv_lines_quote_only_what_needs_it() {
        assert_eq!(
            csv_line(["holding", "5", "a b"].into_iter()),
            "holding,5,a b\n"
        );
        assert_eq!(
            csv_line(["set: v, A", "say \"hi\"", ""].into_iter()),
            "\"set: v, A\",\"say \"\"hi\"\"\",\n"
        );
    }
}
