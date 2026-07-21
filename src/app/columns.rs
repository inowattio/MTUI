use super::{fuzzy_rank, App};
use crate::config::Column;
use crate::state::{ColumnsParams, Popup, StatusMessage};

impl App {
    pub fn open_columns(&mut self) {
        self.read_mut().popup = Some(Popup::Columns(ColumnsParams::default()));
    }

    pub fn column_matches(&self) -> Vec<Column> {
        let Some(c) = self.popup_as::<ColumnsParams>() else {
            return Vec::new();
        };
        fuzzy_rank(&c.query, Column::ALL, |col| col.name())
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
        if count == 0 {
            return;
        }
        if let Some(p) = self.popup_as_mut::<ColumnsParams>() {
            let rows = count.div_ceil(2);
            let (col_start, col_len, row) = if p.selected < rows {
                (0, rows, p.selected)
            } else {
                (rows, count - rows, p.selected - rows)
            };
            let new_row = if down {
                (row + 1) % col_len
            } else {
                (row + col_len - 1) % col_len
            };
            p.selected = col_start + new_row;
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

    pub fn copy_address(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        let message = {
            let (_, address) = self.cursor_cell();

            if self.clipboard.is_none() {
                self.clipboard = arboard::Clipboard::new().ok().map(super::ClipboardHandle);
            }
            match self
                .clipboard
                .as_mut()
                .map(|c| c.0.set_text(address.to_string()))
            {
                Some(Ok(())) => StatusMessage::ok(format!("Copied address {address} to clipboard")),
                _ => StatusMessage::err("Clipboard unavailable"),
            }
        };
        #[cfg(target_arch = "wasm32")]
        let message = StatusMessage::err("Clipboard unavailable");
        self.set_read_status(message);
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
                    .map_or_else(|| vec![address], |rule| rule.word_addresses()),
                Some(column) => {
                    let width = column.graph_width().unwrap_or(1) as u16;
                    (0..width).map(|o| address.wrapping_add(o)).collect()
                }
                None => vec![address],
            }
        };
        let n: usize = addresses
            .iter()
            .filter_map(|&a| self.value_history.remove(&(kind, a)))
            .map(|h| h.len())
            .sum();
        log::info!("Cleared graph history at {address} ({n} sample(s))");
        self.set_read_status(StatusMessage::ok(format!(
            "Cleared graph history ({n} sample(s))"
        )));
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
        const STEP: u16 = 8;
        let max = self.h_max_offset.get();
        let p = self.read_mut();
        // Re-clamp first so a stale offset responds on the first key press.
        let current = p.col_offset.min(max);
        p.col_offset = if right {
            (current + STEP).min(max)
        } else {
            current.saturating_sub(STEP)
        };
    }

    pub fn toggle_column(&mut self, column: Column) {
        self.interpreter.toggle(column);
    }
}
