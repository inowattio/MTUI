use super::App;
use crate::config::{BatchAnchor, Column};
use crate::constants::NO_VALUE;
use crate::interpretator::{ascii_words, fmt_num, graph_value};
use crate::num_ops::cycle;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{InspectMode, Popup, ReadPanel};
use chrono::{DateTime, Local, Utc};
use std::collections::{BTreeSet, VecDeque};

const INSPECT_COLUMNS: &[Column] = &[
    Column::U16,
    Column::I16,
    Column::U8,
    Column::I8,
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

    fn all_panel_cells(&self) -> Box<dyn Iterator<Item = RegisterCell> + '_> {
        match self.read().panel {
            ReadPanel::Main | ReadPanel::Pinned | ReadPanel::Matrix => {
                Box::new(self.pinned_registers.iter().copied())
            }
            ReadPanel::Labeled => Box::new(self.labels.keys().copied()),
            ReadPanel::Custom => Box::new(self.custom_rules.keys().copied()),
        }
    }

    fn panel_cells(&self) -> Box<dyn Iterator<Item = RegisterCell> + '_> {
        if !self.config.filter_panels_by_type {
            return self.all_panel_cells();
        }
        let current = self.read().register_type;
        Box::new(
            self.all_panel_cells()
                .filter(move |&(kind, _)| kind == current),
        )
    }

    pub fn panel_hidden_by_type(&self) -> bool {
        self.config.filter_panels_by_type
            && self.panel_cells().next().is_none()
            && self.all_panel_cells().next().is_some()
    }

    pub fn scroll_to_cursor(&mut self) {
        let rows = self.visible_rows.get();
        let cols = self.matrix_cols();
        self.read_mut().scroll_to_cursor(rows, cols);
    }

    pub(super) fn clamp_panel_cursor(&mut self) {
        if matches!(self.read().panel, ReadPanel::Main | ReadPanel::Matrix) {
            return;
        }
        let rows = self.panel_scroll_rows();
        let len = self.panel_len();
        self.read_mut().scroll_pinned(rows, len);
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

        let window =
            if self.read().panel == ReadPanel::Custom && self.config.batch.custom_by_registers {
                let costs: Vec<usize> = same
                    .iter()
                    .map(|cell| {
                        self.custom_rules
                            .get(cell)
                            .map_or(1, |rule| rule.repr.register_count())
                    })
                    .collect();
                let (start, end) = sized_window(&costs, pos, batch, self.config.batch.anchor);
                &same[start..end]
            } else {
                let batch = batch.min(same.len());
                let start = match self.config.batch.anchor {
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
                for word_address in rule.word_addresses().skip(1) {
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

    pub fn toggle_panel(&mut self) {
        let enabled = self.config.cycle_panels;
        let mut next = self.read().panel;
        for _ in 0..ReadPanel::ALL.len() {
            next = cycle(&ReadPanel::ALL, next, true);
            if enabled.enabled(next) {
                break;
            }
        }
        self.read_mut().panel = next;
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
        self.read_log.get(&cell).map(|entry| entry.value)
    }

    pub fn cell_changed(&self, cell: RegisterCell) -> bool {
        self.changed_since(cell, Utc::now())
    }

    fn changed_since(&self, cell: RegisterCell, now: DateTime<Utc>) -> bool {
        let Some(&at) = self.changed.get(&cell) else {
            return false;
        };
        match self.config.changed_expiry_ms {
            0 => true,
            ms => now.signed_duration_since(at).num_milliseconds() < ms as i64,
        }
    }

    pub fn inspect_lines(&self, mode: InspectMode) -> Vec<(&'static str, String)> {
        let cell = self.cursor_cell();
        if mode != InspectMode::Now {
            return self.inspect_aggregates(cell, mode);
        }
        let (kind, addr) = cell;
        let Some(entry) = self.read_log.get(&cell) else {
            return Vec::new();
        };
        let (value, time) = (entry.value, entry.at);
        let at = |address: u16| self.read_log.get(&(kind, address)).map(|e| e.value);
        let custom = self.custom_value(cell, value, &at);
        let label = self.labels.get(&cell).map(String::as_str);
        let time_text = time
            .with_timezone(&Local)
            .format("%H:%M:%S.%3f")
            .to_string();
        self.interpreter.interpret_all(
            addr,
            value,
            [1, 2, 3].map(|offset| at(addr.saturating_add(offset))),
            &time_text,
            Utc::now().signed_duration_since(time),
            custom.as_deref(),
            label,
        )
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
            return self.combined_history(cell.0, rule.word_addresses(), |regs| {
                rule.numeric(regs, order)
            });
        }
        let Some(width) = column.graph_width() else {
            return Vec::new();
        };
        let addresses = (0..width as u16).map(|o| cell.1.wrapping_add(o));
        self.combined_history(cell.0, addresses, |regs| graph_value(column, order, regs))
    }

    fn combined_history<F>(
        &self,
        kind: RegisterType,
        addresses: impl Iterator<Item = u16>,
        mut value: F,
    ) -> Vec<(DateTime<Utc>, f64)>
    where
        F: FnMut(&[u16]) -> Option<f64>,
    {
        let mut histories = Vec::new();
        for address in addresses {
            match self.value_history((kind, address)) {
                Some(history) => histories.push(history),
                None => return Vec::new(),
            }
        }

        let len = histories.iter().map(|h| h.len()).min().unwrap_or(0);
        let mut regs = vec![0u16; histories.len()];
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

    pub fn cell_row(&self, cell: RegisterCell, now: DateTime<Utc>) -> Option<(String, bool)> {
        let (kind, addr) = cell;
        let entry = self.read_log.get(&cell)?;
        let value = entry.value;
        let at = |address: u16| self.read_log.get(&(kind, address)).map(|e| e.value);
        let custom = self.custom_value(cell, value, &at);
        let row = self.interpreter.format_row(
            addr,
            value,
            [1, 2, 3].map(|offset| at(addr.saturating_add(offset))),
            &entry.time_text,
            now.signed_duration_since(entry.at),
            custom.as_deref(),
            self.label(cell),
        );
        Some((row, self.changed_since(cell, now)))
    }

    pub fn matrix_cols(&self) -> u16 {
        match self.config.matrix.columns {
            0 => fit_matrix_cols(self.viewport_width),
            n => n,
        }
    }

    pub fn custom_text(&self, cell: RegisterCell) -> Option<String> {
        let value = self.read_log.get(&cell)?.value;
        let at = |address: u16| self.read_log.get(&(cell.0, address)).map(|e| e.value);
        self.custom_value(cell, value, &at)
    }

    pub fn ascii_string_for(&self, cells: impl Iterator<Item = RegisterCell>) -> String {
        let words: Vec<u16> = cells
            .filter_map(|cell| self.read_log.get(&cell).map(|e| e.value))
            .collect();
        ascii_words(&words)
    }

    pub fn label_text(&self, register_type: RegisterType, address: u16) -> Option<String> {
        self.label((register_type, address)).map(str::to_string)
    }

    pub fn label(&self, cell: RegisterCell) -> Option<&str> {
        self.labels.get(&cell).map(String::as_str)
    }
}

pub const MATRIX_PREFIX_W: u16 = 7;
pub const MATRIX_CELL_W: u16 = 6;

pub fn fit_matrix_cols(width: u16) -> u16 {
    (width.saturating_sub(MATRIX_PREFIX_W) / MATRIX_CELL_W).max(1)
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod panel_tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::register::RegisterType::{Holding, Input};
    use crate::state::ReadPanel;

    async fn pinned_app() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.pinned_registers = vec![(Holding, 1), (Input, 2), (Holding, 3)];
        app.read_mut().panel = ReadPanel::Pinned;
        app.read_mut().register_type = Holding;
        app
    }

    #[tokio::test]
    async fn the_type_filter_narrows_the_panel_and_drops_group_breaks() {
        let mut app = pinned_app().await;
        assert_eq!(app.panel_len(), 3);
        assert_eq!(app.panel_group_breaks(), 1);
        assert!(!app.panel_hidden_by_type());

        app.config.filter_panels_by_type = true;
        assert_eq!(app.panel_len(), 2);
        assert_eq!(app.panel_group_breaks(), 0);
        assert_eq!(app.panel_cell_at(1), Some((Holding, 3)));

        app.read_mut().register_type = Input;
        assert_eq!(app.panel_window(0, 10), vec![(Input, 2)]);
    }

    #[tokio::test]
    async fn switching_type_clamps_the_cursor_and_reports_hidden_cells() {
        let mut app = pinned_app().await;
        app.config.filter_panels_by_type = true;
        app.read_mut().pinned_index = 1;
        assert_eq!(app.cursor_cell(), (Holding, 3));

        app.toggle_type();
        assert_eq!(app.read().register_type, Input);
        assert_eq!(app.read().pinned_index, 0);
        assert_eq!(app.cursor_cell(), (Input, 2));

        app.toggle_type();
        assert!(app.panel_hidden_by_type());
        assert_eq!(app.panel_len(), 0);
    }

    #[tokio::test]
    async fn disabled_setting_keeps_every_type_listed() {
        let mut app = pinned_app().await;
        app.read_mut().pinned_index = 2;
        app.toggle_type();
        assert_eq!(app.cursor_cell(), (Holding, 3));
        assert_eq!(app.panel_len(), 3);
    }
}

#[cfg(test)]
mod tests {
    use super::sized_window;
    use crate::config::BatchAnchor;

    #[test]
    fn fitted_columns_fill_the_width_and_never_drop_below_one() {
        assert_eq!(super::fit_matrix_cols(0), 1);
        assert_eq!(super::fit_matrix_cols(13), 1);
        assert_eq!(super::fit_matrix_cols(19), 2);
        assert_eq!(super::fit_matrix_cols(80), 12);
        assert_eq!(super::fit_matrix_cols(120), 18);
    }

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
