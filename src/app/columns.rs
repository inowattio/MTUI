use super::{App, fuzzy_rank};
use crate::config::Column;
use crate::interpretator::RowSegment;
use crate::num_ops::{step_hscroll, wrap_index};
use crate::register::RegisterCell;
use crate::state::{ColumnsParams, Popup, ReadPanel, StatusMessage};
use chrono::Utc;

impl App {
    pub fn open_columns(&mut self) {
        self.read_mut().popup = Some(Popup::Columns(ColumnsParams::default()));
    }

    pub fn column_matches(&self) -> Vec<Column> {
        let Some(c) = self.popup_as::<ColumnsParams>() else {
            return Vec::new();
        };
        fuzzy_rank(&c.query, self.interpreter.ordered_columns(), |col| {
            col.name()
        })
    }

    pub fn columns_move_selected(&mut self, right: bool) {
        let matches = self.column_matches();
        let Some(selected) = self
            .popup_as::<ColumnsParams>()
            .map(|p| p.selected as usize)
        else {
            return;
        };
        let Some(&column) = matches.get(selected) else {
            return;
        };
        if !self.interpreter.move_column(column, right) {
            return;
        }
        self.refresh_dirty();
        let moved = self
            .column_matches()
            .iter()
            .position(|&c| c == column)
            .map(|index| index as u16);
        if let Some(index) = moved
            && let Some(p) = self.popup_as_mut::<ColumnsParams>()
        {
            p.selected = index;
        }
    }

    pub fn columns_input(&mut self, c: char) {
        if let Some(p) = self.popup_as_mut::<ColumnsParams>() {
            p.query.push(c);
            p.selected = 0;
        }
    }

    pub fn columns_backspace(&mut self) {
        if let Some(p) = self.popup_as_mut::<ColumnsParams>() {
            p.query.pop();
            p.selected = 0;
        }
    }

    pub fn columns_toggle_selected(&mut self) {
        let matches = self.column_matches();
        let Some(selected) = self
            .popup_as::<ColumnsParams>()
            .map(|p| p.selected as usize)
        else {
            return;
        };
        if let Some(&column) = matches.get(selected) {
            self.toggle_column(column);
        }
    }

    pub fn columns_move(&mut self, down: bool) {
        let count = self.column_matches().len() as u16;
        if let Some(p) = self.popup_as_mut::<ColumnsParams>() {
            let rows = count.div_ceil(2);
            let (col_start, col_len, row) = if p.selected < rows {
                (0, rows, p.selected)
            } else {
                (rows, count - rows, p.selected - rows)
            };
            p.selected = col_start + wrap_index(row, col_len, down);
        }
    }

    pub fn columns_switch(&mut self, right: bool) {
        let count = self.column_matches().len() as u16;
        if count == 0 {
            return;
        }
        if let Some(p) = self.popup_as_mut::<ColumnsParams>() {
            let rows = count.div_ceil(2);
            let row = if p.selected < rows {
                p.selected
            } else {
                p.selected - rows
            };
            p.selected = if right {
                (rows + row).min(count - 1)
            } else {
                row
            };
        }
    }

    pub fn toggle_graph(&mut self) {
        let p = self.read_mut();
        p.graph = !p.graph;
    }

    pub fn graph_hold_series(&mut self) {
        const MAX_HELD: usize = 3;
        let cell = self.cursor_cell();
        let name = self
            .label_text(cell.0, cell.1)
            .unwrap_or_else(|| format!("{}{}", cell.0.marker(), cell.1));

        let p = self.read_mut();
        let message = if let Some(i) = p.graph_series.iter().position(|&c| c == cell) {
            p.graph_series.remove(i);
            StatusMessage::ok(format!("Released \"{name}\" from the graph"))
        } else if p.graph_series.len() >= MAX_HELD {
            StatusMessage::warn(format!("Up to {MAX_HELD} held series"))
        } else {
            p.graph_series.push(cell);
            StatusMessage::ok(format!("Holding \"{name}\" on the graph"))
        };
        self.set_read_status(message);
    }

    pub fn copy_column_arm(&mut self) {
        if self.read().panel == ReadPanel::Matrix {
            self.copy_matrix_cell();
            return;
        }
        let segments = self.interpreter.row_segments();
        if segments.is_empty() {
            return;
        }
        let index = segments
            .iter()
            .position(|s| s.name == "address")
            .unwrap_or(0) as u16;
        self.read_mut().copy_column = Some(index);
        self.scroll_column_into_view();
    }

    pub fn copy_column_move(&mut self, right: bool) {
        let last = self.interpreter.row_segments().len().saturating_sub(1) as u16;
        let Some(index) = self.read().copy_column else {
            return;
        };
        let next = if right {
            index.saturating_add(1).min(last)
        } else {
            index.saturating_sub(1)
        };
        self.read_mut().copy_column = Some(next);
        self.scroll_column_into_view();
    }

    pub fn copy_column_cancel(&mut self) {
        self.read_mut().copy_column = None;
    }

    pub fn copy_column_commit(&mut self) {
        let Some(index) = self.read().copy_column else {
            return;
        };
        self.read_mut().copy_column = None;
        let Some(segment) = self.interpreter.row_segments().get(index as usize).copied() else {
            return;
        };
        let cell = self.cursor_cell();
        let row = match self.cell_row(cell, Utc::now()) {
            Some((row, _)) => row,
            None => self.interpreter.placeholder(cell.1, self.label(cell)),
        };
        let text = segment.text(&row);
        let name = segment.name;
        let message = if text.is_empty() {
            StatusMessage::warn(format!("Nothing to copy in {name}"))
        } else if self.set_clipboard(text.clone()) {
            StatusMessage::ok(format!("Copied {name} '{text}' to clipboard"))
        } else {
            StatusMessage::err("Clipboard unavailable")
        };
        self.set_read_status(message);
    }

    pub fn copy_column_segment(&self) -> Option<RowSegment> {
        let index = self.read().copy_column?;
        self.interpreter.row_segments().get(index as usize).copied()
    }

    pub fn copy_column_name(&self) -> Option<&'static str> {
        Some(self.copy_column_segment()?.name)
    }

    fn copy_matrix_cell(&mut self) {
        let cell = self.cursor_cell();
        let message = match self.cell_value(cell) {
            None => StatusMessage::warn("Nothing read here yet"),
            Some(value) if self.set_clipboard(value.to_string()) => {
                StatusMessage::ok(format!("Copied {value} to clipboard"))
            }
            Some(_) => StatusMessage::err("Clipboard unavailable"),
        };
        self.set_read_status(message);
    }

    fn scroll_column_into_view(&mut self) {
        let Some(index) = self.read().copy_column else {
            return;
        };
        let Some(segment) = self.interpreter.row_segments().get(index as usize).copied() else {
            return;
        };
        let prefix = self.interpreter.prefix_width() as usize;
        if segment.start < prefix {
            self.read_mut().col_offset = 0;
            return;
        }
        let visible = (self.viewport_width as usize).saturating_sub(prefix).max(1);
        let start = segment.start - prefix;
        let end = segment.end().saturating_sub(prefix);
        let offset = self.read().col_offset as usize;
        let next = if start < offset {
            start
        } else if end > offset + visible {
            end - visible
        } else {
            offset
        };
        let max = self.h_max_offset.get() as usize;
        self.read_mut().col_offset = next.min(max) as u16;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn set_clipboard(&mut self, text: String) -> bool {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok().map(super::ClipboardHandle);
        }
        matches!(
            self.clipboard.as_mut().map(|c| c.0.set_text(text)),
            Some(Ok(()))
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn set_clipboard(&self, _text: String) -> bool {
        false
    }

    fn graphable_columns(&self) -> Vec<Column> {
        let mut cols: Vec<Column> = Column::ALL
            .iter()
            .copied()
            .filter(|&c| c.is_graphable() && self.interpreter.is_enabled(c))
            .collect();
        if self.interpreter.is_enabled(Column::Custom)
            && self.custom_rule(self.cursor_cell()).is_some()
        {
            cols.push(Column::Custom);
        }
        cols
    }

    pub fn graph_cycle_len(&self) -> usize {
        self.graphable_columns().len()
    }

    pub(super) fn graph_extra_registers(&self) -> Vec<RegisterCell> {
        if !self.read().graph {
            return Vec::new();
        }
        let column = self.active_graph_column();
        let mut regs: Vec<RegisterCell> = Vec::new();
        for &(kind, address) in &self.read().graph_series {
            if kind.is_bit() {
                regs.push((kind, address));
            } else if column == Some(Column::Custom) {
                if let Some(rule) = self.custom_rule((kind, address)) {
                    regs.extend(rule.word_addresses().map(|a| (kind, a)));
                }
            } else {
                let width = column.and_then(Column::graph_width).unwrap_or(1) as u16;
                regs.extend((0..width).map(|o| (kind, address.wrapping_add(o))));
            }
        }
        regs.sort();
        regs.dedup();
        regs
    }

    pub fn active_graph_column(&self) -> Option<Column> {
        let cols = self.graphable_columns();
        let current = self.read().graph_column;
        if cols.contains(&current) {
            Some(current)
        } else {
            cols.first().copied()
        }
    }

    pub fn clear_graph_history(&mut self) {
        let (kind, address) = self.cursor_cell();
        let addresses: Vec<u16> = if kind.is_bit() {
            vec![address]
        } else {
            match self.active_graph_column() {
                Some(Column::Custom) => self
                    .custom_rule((kind, address))
                    .map_or_else(|| vec![address], |rule| rule.word_addresses().collect()),
                Some(column) => {
                    let width = column.graph_width().unwrap_or(1) as u16;
                    (0..width).map(|o| address.wrapping_add(o)).collect()
                }
                None => vec![address],
            }
        };
        addresses
            .into_iter()
            .map(|a| (kind, a))
            .chain(self.graph_extra_registers())
            .for_each(|cell| {
                let _ = self.value_history.remove(&cell);
            });
        log::info!("Cleared graph history");
        self.set_read_status(StatusMessage::ok("Cleared graph history"));
    }

    pub fn cycle_graph_interpretation(&mut self) {
        let cols = self.graphable_columns();
        if cols.is_empty() {
            return;
        }
        let current = self.read().graph_column;
        let next = match cols.iter().position(|&c| c == current) {
            Some(i) => cols[(i + 1) % cols.len()],
            None => cols[0],
        };
        self.read_mut().graph_column = next;
    }

    pub fn scroll_columns(&mut self, right: bool) {
        let max = self.h_max_offset.get();
        let p = self.read_mut();
        p.col_offset = step_hscroll(p.col_offset, max, right);
    }

    pub(super) fn sync_auto_widths(&mut self) {
        if self.interpreter.label_width() == 0 {
            let longest = self
                .labels
                .values()
                .map(|label| label.chars().count())
                .max()
                .unwrap_or(0);
            self.interpreter.set_label_auto(longest);
        }
        if self.interpreter.custom_width() == 0 {
            let longest = self
                .custom_rules
                .keys()
                .filter_map(|&cell| self.custom_text(cell))
                .map(|text| text.chars().count())
                .max()
                .unwrap_or(0);
            self.interpreter.set_custom_auto(longest);
        }
    }

    pub fn toggle_column(&mut self, column: Column) {
        self.interpreter.toggle(column);
        self.refresh_dirty();
    }
}
