use super::{fuzzy_score, App};
use crate::register::{RegisterCell, RegisterType};
use crate::state::{LabelParams, Popup, ReadPanel, SearchParams};

impl App {
    fn search_mut(&mut self) -> Option<&mut SearchParams> {
        self.popup_as_mut()
    }

    pub fn open_search(&mut self) {
        self.read_mut().popup = Some(Popup::Search(SearchParams::default()));
        self.recompute_search();
    }

    pub fn open_label(&mut self) {
        let (label_type, label_pos) = self.cursor_cell();
        let text = self
            .labels
            .get(&(label_type, label_pos))
            .cloned()
            .unwrap_or_default();
        self.read_mut().popup = Some(Popup::Label(LabelParams {
            position: label_pos,
            register_type: label_type,
            text,
        }));
    }

    pub fn search_input(&mut self, c: char) {
        if let Some(s) = self.search_mut() {
            s.query.push(c);
        }
        self.recompute_search();
    }

    pub fn search_backspace(&mut self) {
        if let Some(s) = self.search_mut() {
            s.query.pop();
        }
        self.recompute_search();
    }

    pub fn search_move(&mut self, down: bool) {
        let rows = crate::constants::SEARCH_POPUP_ROWS;
        if let Some(s) = self.search_mut() {
            s.selected = if down {
                s.selected.saturating_add(1)
            } else {
                s.selected.saturating_sub(1)
            };
            s.scroll(rows);
        }
    }

    pub fn search_commit(&mut self) -> bool {
        let target = self
            .popup_as::<SearchParams>()
            .and_then(|s| s.matches.get(s.selected as usize).map(|(cell, _)| *cell));
        let Some((register_type, position)) = target else {
            return false;
        };

        let from = {
            let p = self.read();
            (p.register_type, p.position)
        };
        self.previous_position = Some(from);

        self.jump_to_cell(register_type, position);
        self.read_mut().popup = None;
        true
    }

    pub fn cycle_position(&mut self) {
        let Some((register_type, position)) = self.previous_position else {
            return;
        };
        let current = {
            let p = self.read();
            (p.register_type, p.position)
        };
        self.previous_position = Some(current);

        self.jump_to_cell(register_type, position);
    }

    fn jump_to_cell(&mut self, register_type: RegisterType, position: u16) {
        if register_type != self.read().register_type {
            self.stop_sweep();
        }
        let rows = self.visible_rows.get();
        let cols = self.config.matrix_cols;
        let p = self.read_mut();
        if p.panel != ReadPanel::Matrix {
            p.panel = ReadPanel::Main;
        }
        p.register_type = register_type;
        p.position = position;
        p.scroll_to_cursor(rows, cols);
    }

    fn recompute_search(&mut self) {
        let Some(query) = self.popup_as::<SearchParams>().map(|s| s.query.clone()) else {
            return;
        };
        let current_type = self.read().register_type;

        let (register_type, has_explicit_type) = match query.chars().next() {
            Some('h' | 'H') => (RegisterType::Holding, true),
            Some('i' | 'I') => (RegisterType::Input, true),
            Some('c' | 'C') => (RegisterType::Coil, true),
            Some('d' | 'D') => (RegisterType::Discrete, true),
            _ => (current_type, false),
        };

        let mut matches: Vec<(RegisterCell, String)> = Vec::new();

        let numeric_query = if has_explicit_type {
            &query[1..]
        } else {
            query.as_str()
        };

        if let Ok(parsed_address) = numeric_query.trim().parse::<u32>() {
            let address = if parsed_address > u16::MAX as u32 {
                u16::MAX
            } else {
                parsed_address as u16
            };

            matches.push(((register_type, address), "jump to this address".to_string()));
        }

        let mut scored: Vec<(i32, RegisterCell, String)> = self
            .labels
            .iter()
            .filter_map(|(&cell, text)| {
                fuzzy_score(&query, text).map(|score| (score, cell, text.clone()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        matches.extend(scored.into_iter().map(|(_, cell, text)| (cell, text)));

        if let Some(s) = self.search_mut() {
            s.matches = matches;
            s.selected = 0;
            s.top = 0;
            s.scroll(crate::constants::SEARCH_POPUP_ROWS);
        }
    }

    pub fn label_input(&mut self, c: char) {
        if let Some(l) = self.popup_as_mut::<LabelParams>() {
            l.text.push(c);
        }
    }

    pub fn label_backspace(&mut self) {
        if let Some(l) = self.popup_as_mut::<LabelParams>() {
            l.text.pop();
        }
    }

    pub fn commit_label(&mut self) {
        let Some((position, register_type, text)) = self
            .popup_as::<LabelParams>()
            .map(|l| (l.position, l.register_type, l.text.clone()))
        else {
            return;
        };

        let key = (register_type, position);
        if text.is_empty() {
            self.labels.remove(&key);
        } else {
            self.labels.insert(key, text);
        }
        self.refresh_dirty();

        self.read_mut().popup = None;
    }
}
