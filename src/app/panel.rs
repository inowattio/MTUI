use super::App;
use crate::config::{BatchAnchor, Column};
use crate::constants::NO_VALUE;
use crate::interpretator::{fmt_num, format_ago, graph_value};
use crate::num_ops::cycle;
use crate::register::{RegisterCell, RegisterCellValue, RegisterType};
use crate::state::{InspectMode, Popup, ReadPanel};
use chrono::{DateTime, Local, Utc};
use std::collections::{BTreeSet, VecDeque};

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

    fn panel_cells(&self) -> Box<dyn Iterator<Item = RegisterCell> + '_> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => {
                Box::new(self.pinned_registers.iter().copied())
            }
            ReadPanel::Labeled => Box::new(self.labels.keys().copied()),
            ReadPanel::Custom => Box::new(self.custom_rules.keys().copied()),
        }
    }

    pub fn panel_cell_at(&self, index: usize) -> Option<RegisterCell> {
        self.panel_cells().nth(index)
    }

    pub fn panel_window(&self, start: usize, count: usize) -> Vec<RegisterCell> {
        self.panel_cells().skip(start).take(count).collect()
    }

    pub fn panel_len(&self) -> u16 {
        self.panel_cells().count() as u16
    }

    pub fn panel_read_cells(&self) -> Vec<RegisterCell> {
        let (_, amount) = self.read_window();
        self.panel_refresh_window(amount as usize)
    }

    pub(super) fn panel_refresh_window(&self, batch: usize) -> Vec<RegisterCell> {
        let cursor = self.cursor_cell();
        let kind = cursor.0;
        let same: Vec<RegisterCell> = self.panel_cells().filter(|&(k, _)| k == kind).collect();
        if same.is_empty() {
            return Vec::new();
        }
        let batch = batch.max(1);
        let pos = same.iter().position(|&c| c == cursor).unwrap_or(0);

        let window = if self.read().panel == ReadPanel::Custom && self.config.custom_batch_by_size {
            let costs: Vec<usize> = same
                .iter()
                .map(|cell| {
                    self.custom_rules
                        .get(cell)
                        .map_or(1, |rule| rule.repr.register_count())
                })
                .collect();
            let (start, end) = sized_window(&costs, pos, batch, self.config.batch_anchor);
            &same[start..end]
        } else {
            let batch = batch.min(same.len());
            let start = match self.config.batch_anchor {
                BatchAnchor::Start => pos,
                BatchAnchor::Middle => pos.saturating_sub(batch / 2),
                BatchAnchor::End => pos.saturating_sub(batch - 1),
            }
            .min(same.len() - batch);
            &same[start..start + batch]
        };

        let mut cells = BTreeSet::new();
        for &(kind, addr) in window {
            cells.insert((kind, addr));
            if let Some(rule) = self.custom_rules.get(&(kind, addr)) {
                for word_address in rule.word_addresses().into_iter().skip(1) {
                    cells.insert((kind, word_address));
                }
            }
        }
        cells.into_iter().collect()
    }

    fn panel_has_type(&self, kind: RegisterType) -> bool {
        self.panel_cells().any(|(k, _)| k == kind)
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
        let Some(&at) = self.changed.get(&cell) else {
            return false;
        };
        match self.config.changed_expiry_ms {
            None => true,
            Some(ms) => Utc::now().signed_duration_since(at).num_milliseconds() < ms as i64,
        }
    }

    pub fn inspect_lines(&self, mode: InspectMode) -> Vec<(&'static str, String)> {
        let cell = self.cursor_cell();
        if mode != InspectMode::Now {
            return self.inspect_aggregates(cell, mode);
        }
        let (kind, addr) = cell;
        let Some(&(value, time)) = self.read_log.get(&cell) else {
            return Vec::new();
        };
        let at = |address: u16| self.read_log.get(&(kind, address)).map(|&(v, _)| v);
        let custom = self.custom_value(cell, value, self.config.device.word_order, &at);
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
            [1, 2, 3].map(|offset| at(addr.saturating_add(offset))),
            custom.as_deref(),
            label,
        ));
        lines
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
        let series: Vec<f64> = self
            .column_history(cell, column)
            .into_iter()
            .map(|(_, v)| v)
            .collect();
        if series.is_empty() {
            return NO_VALUE.to_string();
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

    pub fn column_history(&self, cell: RegisterCell, column: Column) -> Vec<(DateTime<Utc>, f64)> {
        let order = self.config.device.word_order;
        if column == Column::Custom {
            let Some(rule) = self.custom_rule(cell) else {
                return Vec::new();
            };
            return self.combined_history(cell.0, &rule.word_addresses(), |regs| {
                rule.numeric(regs, order)
            });
        }
        let Some(width) = column.graph_width() else {
            return Vec::new();
        };
        let addresses: Vec<u16> = (0..width as u16).map(|o| cell.1.wrapping_add(o)).collect();
        self.combined_history(cell.0, &addresses, |regs| graph_value(column, order, regs))
    }

    fn combined_history<F>(
        &self,
        kind: RegisterType,
        addresses: &[u16],
        mut value: F,
    ) -> Vec<(DateTime<Utc>, f64)>
    where
        F: FnMut(&[u16]) -> Option<f64>,
    {
        let mut histories = Vec::with_capacity(addresses.len());
        for &address in addresses {
            match self.value_history((kind, address)) {
                Some(history) => histories.push(history),
                None => return Vec::new(),
            }
        }

        let len = histories.iter().map(|h| h.len()).min().unwrap_or(0);
        let mut regs = vec![0u16; addresses.len()];
        let mut values = Vec::with_capacity(len);
        for i in 0..len {
            for (k, history) in histories.iter().enumerate() {
                regs[k] = history[history.len() - len + i].0;
            }

            // The words of one sample come from the same batch read, so the
            // first word's timestamp stands for the whole sample
            let at = histories[0][histories[0].len() - len + i].1;
            if let Some(v) = value(&regs) {
                values.push((at, v));
            }
        }
        values
    }

    pub fn custom_count(&self) -> usize {
        self.custom_rules.len()
    }

    pub fn value_history(&self, cell: RegisterCell) -> Option<&VecDeque<(u16, DateTime<Utc>)>> {
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
        let at = |address: u16| self.read_log.get(&(kind, address)).map(|&(v, _)| v);
        let custom = self.custom_value(cell, value, self.config.device.word_order, &at);
        let label = self.labels.get(&cell).map(String::as_str);
        let row = self.interpreter.format_row(
            addr,
            value,
            [1, 2, 3].map(|offset| at(addr.saturating_add(offset))),
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

fn sized_window(costs: &[usize], pos: usize, budget: usize, anchor: BatchAnchor) -> (usize, usize) {
    let (mut start, mut end) = (pos, pos + 1);
    let mut left = budget.saturating_sub(costs[pos]);
    let mut prefer_before = true;
    loop {
        let before = (start > 0 && costs[start - 1] <= left).then(|| costs[start - 1]);
        let after = (end < costs.len() && costs[end] <= left).then(|| costs[end]);
        let grow_before = match (before, after) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(_), Some(_)) => match anchor {
                BatchAnchor::Start => false,
                BatchAnchor::End => true,
                BatchAnchor::Middle => {
                    prefer_before = !prefer_before;
                    !prefer_before
                }
            },
        };
        if grow_before {
            start -= 1;
            left -= before.expect("checked");
        } else {
            end += 1;
            left -= after.expect("checked");
        }
    }
    (start, end)
}

#[cfg(test)]
mod tests {
    use super::sized_window;
    use crate::config::BatchAnchor;

    fn middle(costs: &[usize], pos: usize, budget: usize) -> (usize, usize) {
        sized_window(costs, pos, budget, BatchAnchor::Middle)
    }

    #[test]
    fn cursor_alone_when_over_budget() {
        assert_eq!(middle(&[4, 1], 0, 2), (0, 1));
        assert_eq!(middle(&[2, 1], 0, 2), (0, 1));
    }

    #[test]
    fn grows_alternately_around_cursor() {
        assert_eq!(middle(&[1, 2, 1], 1, 4), (0, 3));
        assert_eq!(middle(&[1, 1, 1, 1, 1], 2, 3), (1, 4));
    }

    #[test]
    fn clamps_at_edges_by_extending_the_other_side() {
        assert_eq!(middle(&[1, 1, 1, 1], 3, 3), (1, 4));
        assert_eq!(middle(&[1, 1, 1, 1], 0, 3), (0, 3));
    }

    #[test]
    fn skips_neighbors_that_do_not_fit() {
        assert_eq!(middle(&[4, 1, 1], 1, 2), (1, 3));
    }

    #[test]
    fn anchor_picks_the_growth_side() {
        let costs = [1usize, 1, 1];
        assert_eq!(sized_window(&costs, 1, 2, BatchAnchor::Start), (1, 3));
        assert_eq!(sized_window(&costs, 1, 2, BatchAnchor::End), (0, 2));
    }

    #[test]
    fn anchor_spills_to_the_other_side_at_edges() {
        let costs = [1usize, 1, 1];
        assert_eq!(sized_window(&costs, 2, 2, BatchAnchor::Start), (1, 3));
        assert_eq!(sized_window(&costs, 0, 2, BatchAnchor::End), (0, 2));
    }
}
