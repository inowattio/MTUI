use super::App;
use crate::interpretator::format_ago;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{Popup, ReadPanel};
use chrono::{DateTime, Local, Utc};
use std::collections::VecDeque;

impl App {
    pub fn open_inspect(&mut self) {
        self.read_mut().popup = Some(Popup::Inspect);
    }

    pub fn panel_cell_at(&self, index: usize) -> Option<RegisterCell> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => {
                self.pinned_registers.get(index).copied()
            }
            ReadPanel::Labeled => self.labels.keys().nth(index).copied(),
            ReadPanel::Custom => self.custom_rules.keys().nth(index).copied(),
        }
    }

    pub fn panel_window(&self, start: usize, count: usize) -> Vec<RegisterCell> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => self
                .pinned_registers
                .iter()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
            ReadPanel::Labeled => self
                .labels
                .keys()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
            ReadPanel::Custom => self
                .custom_rules
                .keys()
                .skip(start)
                .take(count)
                .copied()
                .collect(),
        }
    }

    pub fn panel_len(&self) -> u16 {
        let len = match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => self.pinned_registers.len(),
            ReadPanel::Labeled => self.labels.len(),
            ReadPanel::Custom => self.custom_rules.len(),
        };
        len as u16
    }

    pub fn cursor_cell(&self) -> RegisterCell {
        let (panel, register_type, position, index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };
        match panel {
            ReadPanel::Main | ReadPanel::Matrix => (register_type, position),
            _ => self
                .panel_cell_at(index as usize)
                .unwrap_or((register_type, position)),
        }
    }

    pub fn cell_value(&self, cell: RegisterCell) -> Option<u16> {
        self.read_log.get(&cell).map(|&(value, _)| value)
    }

    pub fn cell_changed(&self, cell: RegisterCell) -> bool {
        self.changed.get(&cell).copied().unwrap_or(false)
    }

    pub fn inspect_lines(&self) -> (RegisterCell, Vec<(&'static str, String)>) {
        let cell = self.cursor_cell();
        let (kind, addr) = cell;
        let Some(&(value, time)) = self.read_log.get(&cell) else {
            return (cell, Vec::new());
        };
        let neighbor = |offset: u16| {
            self.read_log
                .get(&(kind, addr.saturating_add(offset)))
                .map(|&(v, _)| v)
        };
        let custom = self.custom_value(cell, value, self.config.device.word_order, &neighbor);
        let label = self.labels.get(&cell).map(String::as_str);
        let mut lines = vec![
            (
                "read at",
                time.with_timezone(&Local)
                    .format("%H:%M:%S.%3f")
                    .to_string(),
            ),
            ("ago", format_ago(Utc::now().signed_duration_since(time))),
        ];
        lines.extend(self.interpreter.interpret_all(
            value,
            [neighbor(1), neighbor(2), neighbor(3)],
            custom.as_deref(),
            label,
        ));
        (cell, lines)
    }

    pub fn custom_count(&self) -> usize {
        self.custom_rules.len()
    }

    pub fn value_history(&self, cell: RegisterCell) -> Option<&VecDeque<u16>> {
        self.value_history.get(&cell)
    }

    pub fn read_count(&self) -> usize {
        self.read_log.len()
    }

    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    pub fn cell_row(&self, cell: RegisterCell, now: DateTime<Local>) -> Option<(String, bool)> {
        let (kind, addr) = cell;
        let &(value, time) = self.read_log.get(&cell)?;
        let neighbor = |offset: u16| {
            self.read_log
                .get(&(kind, addr.saturating_add(offset)))
                .map(|&(v, _)| v)
        };
        let custom = self.custom_value(cell, value, self.config.device.word_order, &neighbor);
        let label = self.labels.get(&cell).map(String::as_str);
        let row = self.interpreter.format_row(
            addr,
            value,
            [neighbor(1), neighbor(2), neighbor(3)],
            time.with_timezone(&Local),
            now,
            custom.as_deref(),
            label,
        );
        Some((row, self.cell_changed(cell)))
    }

    pub fn ascii_string_for(&self, cells: impl Iterator<Item = RegisterCell>) -> String {
        let values: Vec<RegisterCellValue> = cells
            .filter_map(|cell| self.read_log.get(&cell).map(|&(value, _)| (cell, value)))
            .collect();
        self.interpreter.ascii_string(&values)
    }

    pub fn label_text(&self, register_type: RegisterType, address: u16) -> Option<String> {
        self.labels.get(&(register_type, address)).cloned()
    }
}
