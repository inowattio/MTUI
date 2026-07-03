use super::App;
use crate::config::Column;
use crate::interpretator::{fmt_num, format_ago, graph_value};
use crate::num_ops::cycle;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{InspectMode, Popup, ReadPanel};
use chrono::{DateTime, Local, Utc};
use std::collections::VecDeque;

const INSPECT_COLUMNS: &[Column] = &[
    Column::U16,
    Column::I16,
    Column::U8s,
    Column::I8s,
    Column::Hex,
    Column::Hex32,
    Column::F16,
    Column::Bcd,
    Column::Bcd32,
    Column::U32,
    Column::I32,
    Column::U32M10K,
    Column::I32M10K,
    Column::U64,
    Column::I64,
    Column::F32,
    Column::F64,
    Column::Ascii,
    Column::Bits,
    Column::Custom,
];

impl App {
    pub fn open_inspect(&mut self) {
        self.read_mut().popup = Some(Popup::Inspect(InspectMode::default()));
    }

    pub fn inspect_cycle(&mut self, forward: bool) {
        if let Some(mode) = self.popup_as_mut::<InspectMode>() {
            *mode = cycle(&InspectMode::ALL, *mode, forward);
        }
    }

    pub fn open_about(&mut self) {
        self.read_mut().popup = Some(Popup::About);
    }

    pub fn open_stats(&mut self) {
        self.read_mut().popup = Some(Popup::Stats);
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

    pub(super) fn panel_refresh_window(&self, batch: usize) -> Vec<RegisterCell> {
        let cursor = self.cursor_cell();
        let kind = cursor.0;
        let same: Vec<RegisterCell> = self
            .panel_window(0, self.panel_len() as usize)
            .into_iter()
            .filter(|&(k, _)| k == kind)
            .collect();
        if same.is_empty() {
            return Vec::new();
        }
        let batch = batch.max(1).min(same.len());
        let pos = same.iter().position(|&c| c == cursor).unwrap_or(0);
        let start = pos.saturating_sub(batch / 2).min(same.len() - batch);
        same[start..start + batch].to_vec()
    }

    fn panel_has_type(&self, kind: RegisterType) -> bool {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => {
                self.pinned_registers.iter().any(|&(k, _)| k == kind)
            }
            ReadPanel::Labeled => self.labels.keys().any(|&(k, _)| k == kind),
            ReadPanel::Custom => self.custom_rules.keys().any(|&(k, _)| k == kind),
        }
    }

    pub fn panel_group_breaks(&self) -> u16 {
        let present = RegisterType::ALL
            .iter()
            .filter(|&&kind| self.panel_has_type(kind))
            .count() as u16;
        present.saturating_sub(1)
    }

    pub fn panel_scroll_rows(&self) -> u16 {
        self.visible_rows
            .get()
            .saturating_sub(self.panel_group_breaks())
            .max(1)
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

    pub fn inspect_lines(&self, mode: InspectMode) -> (RegisterCell, Vec<(&'static str, String)>) {
        let cell = self.cursor_cell();
        if mode != InspectMode::Now {
            return (cell, self.inspect_aggregates(cell, mode));
        }
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

    fn inspect_aggregates(
        &self,
        cell: RegisterCell,
        mode: InspectMode,
    ) -> Vec<(&'static str, String)> {
        let samples = self.value_history(cell).map_or(0, VecDeque::len);
        if samples == 0 {
            return Vec::new();
        }
        let mut lines = vec![("samples", samples.to_string())];
        for &column in INSPECT_COLUMNS {
            lines.push((column.name(), self.aggregate_text(cell, column, mode)));
        }
        lines.push(("label", self.labels.get(&cell).cloned().unwrap_or_default()));
        lines
    }

    fn aggregate_text(&self, cell: RegisterCell, column: Column, mode: InspectMode) -> String {
        let series = self.column_history(cell, column);
        if series.is_empty() {
            return "--".to_string();
        }
        let is_float = column.graph_is_float() || series.iter().any(|v| v.fract() != 0.0);
        match mode {
            InspectMode::Min => fmt_num(
                series.iter().copied().fold(f64::INFINITY, f64::min),
                is_float,
            ),
            InspectMode::Max => fmt_num(
                series.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                is_float,
            ),
            InspectMode::Avg => {
                let avg = series.iter().sum::<f64>() / series.len() as f64;
                if is_float {
                    fmt_num(avg, true)
                } else {
                    format!("{avg:.1}")
                }
            }
            InspectMode::Now => unreachable!("aggregates are not computed for the now mode"),
        }
    }

    pub fn column_history(&self, cell: RegisterCell, column: Column) -> Vec<f64> {
        let order = self.config.device.word_order;
        if column == Column::Custom {
            let Some(rule) = self.custom_rule(cell) else {
                return Vec::new();
            };
            let width = rule.repr.register_count();
            return self.combined_history(cell, width, |regs| rule.numeric(regs, order));
        }
        let Some(width) = column.graph_width() else {
            return Vec::new();
        };
        self.combined_history(cell, width, |regs| graph_value(column, order, regs))
    }

    fn combined_history<F>(&self, cell: RegisterCell, width: usize, mut value: F) -> Vec<f64>
    where
        F: FnMut(&[u16]) -> Option<f64>,
    {
        let (kind, address) = cell;
        let mut histories = Vec::with_capacity(width);
        for offset in 0..width as u16 {
            match self.value_history((kind, address.wrapping_add(offset))) {
                Some(history) => histories.push(history),
                None => return Vec::new(),
            }
        }

        let len = histories.iter().map(|h| h.len()).min().unwrap_or(0);
        let mut regs = vec![0u16; width];
        let mut values = Vec::with_capacity(len);
        for i in 0..len {
            for (k, history) in histories.iter().enumerate() {
                regs[k] = history[history.len() - len + i];
            }
            if let Some(v) = value(&regs) {
                values.push(v);
            }
        }
        values
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
