use super::{App, fuzzy_score};
use crate::register::{RegisterCell, RegisterType};
use crate::state::{LabelParams, Popup, ReadPanel, SearchMatch, SearchParams};

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
        let rows = self.search_rows.get();
        if let Some(s) = self.search_mut() {
            s.selected = if down {
                s.selected.saturating_add(1)
            } else {
                s.selected.saturating_sub(1)
            };
            s.scroll(rows);
        }
    }

    pub fn search_commit(&mut self) {
        let target = self
            .popup_as::<SearchParams>()
            .and_then(|s| s.matches.get(s.selected as usize).map(|m| m.cell));
        let Some((register_type, position)) = target else {
            return;
        };

        let from = {
            let p = self.read();
            (p.register_type, p.position)
        };
        self.previous_position = Some(from);

        self.jump_to_cell(register_type, position);
        self.read_mut().popup = None;
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
        let (current_type, current_position) = {
            let p = self.read();
            (p.register_type, p.position)
        };

        let (register_type, has_explicit_type) = match query.chars().next() {
            Some('h' | 'H') => (RegisterType::Holding, true),
            Some('i' | 'I') => (RegisterType::Input, true),
            Some('c' | 'C') => (RegisterType::Coil, true),
            Some('d' | 'D') => (RegisterType::Discrete, true),
            _ => (current_type, false),
        };

        let mut matches: Vec<SearchMatch> = Vec::new();

        let numeric_query = if has_explicit_type {
            &query[1..]
        } else {
            query.as_str()
        };

        if let Some((address, hint)) = parse_target(numeric_query, current_position) {
            let cell = (register_type, address);
            matches.push(match self.labels.get(&cell) {
                Some(label) => SearchMatch::label(cell, label.clone()),
                None => SearchMatch::hint(cell, hint),
            });
        }
        let target = matches.first().map(|m| m.cell);

        let mut scored: Vec<(i32, RegisterCell, String)> = self
            .labels
            .iter()
            .filter(|&(&cell, _)| Some(cell) != target)
            .filter_map(|(&cell, text)| {
                fuzzy_score(&query, text).map(|score| (score, cell, text.clone()))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        matches.extend(
            scored
                .into_iter()
                .map(|(_, cell, text)| SearchMatch::label(cell, text)),
        );

        let rows = self.search_rows.get();
        if let Some(s) = self.search_mut() {
            s.matches = matches;
            s.selected = 0;
            s.top = 0;
            s.scroll(rows);
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

fn parse_target(input: &str, current: u16) -> Option<(u16, String)> {
    let input = input.trim();
    let (sign, rest) = if let Some(rest) = input.strip_prefix('+') {
        (1, rest)
    } else if let Some(rest) = input.strip_prefix('-') {
        (-1, rest)
    } else {
        return parse_address(input).map(|address| (address, "jump to this address".to_string()));
    };
    let offset = parse_address(rest)?;
    let address = (i32::from(current) + sign * i32::from(offset)).clamp(0, i32::from(u16::MAX));
    let symbol = if sign > 0 { '+' } else { '-' };
    Some((address as u16, format!("move by {symbol}{offset}")))
}

fn parse_address(input: &str) -> Option<u16> {
    let input = input.trim();
    let (digits, radix) = match input.strip_prefix(['x', 'X']) {
        Some(hex) => (hex, 16),
        None => match input
            .strip_prefix("0x")
            .or_else(|| input.strip_prefix("0X"))
        {
            Some(hex) => (hex, 16),
            None => (input, 10),
        },
    };
    let value = u32::from_str_radix(digits, radix).ok()?;
    Some(value.min(u16::MAX as u32) as u16)
}

#[cfg(test)]
mod tests {
    use super::{parse_address, parse_target};

    #[test]
    fn parses_decimal_addresses() {
        assert_eq!(parse_address("111"), Some(111));
        assert_eq!(parse_address(" 42 "), Some(42));
        assert_eq!(parse_address("70000"), Some(u16::MAX));
    }

    #[test]
    fn parses_hex_addresses_with_x_prefix() {
        assert_eq!(parse_address("x6F"), Some(0x6F));
        assert_eq!(parse_address("X6f"), Some(0x6F));
        assert_eq!(parse_address("0x6F"), Some(0x6F));
        assert_eq!(parse_address("xFFFF"), Some(u16::MAX));
        assert_eq!(parse_address("x10000"), Some(u16::MAX));
    }

    #[test]
    fn relative_targets_move_from_the_cursor() {
        assert_eq!(
            parse_target("+69", 100),
            Some((169, "move by +69".to_string()))
        );
        assert_eq!(
            parse_target("-100", 250),
            Some((150, "move by -100".to_string()))
        );
        assert_eq!(
            parse_target("+x10", 0),
            Some((16, "move by +16".to_string()))
        );
        assert_eq!(
            parse_target(" - 5 ", 10),
            Some((5, "move by -5".to_string()))
        );
    }

    #[test]
    fn relative_targets_clamp_to_the_address_range() {
        assert_eq!(parse_target("-100", 50).map(|t| t.0), Some(0));
        assert_eq!(parse_target("+100", 65500).map(|t| t.0), Some(u16::MAX));
    }

    #[test]
    fn absolute_targets_keep_the_jump_text() {
        assert_eq!(
            parse_target("111", 0),
            Some((111, "jump to this address".to_string()))
        );
        assert_eq!(
            parse_target("x6F", 0),
            Some((0x6F, "jump to this address".to_string()))
        );
        assert_eq!(parse_target("+", 10), None);
        assert_eq!(parse_target("-", 10), None);
    }

    #[test]
    fn rejects_non_numeric_input() {
        assert_eq!(parse_address(""), None);
        assert_eq!(parse_address("x"), None);
        assert_eq!(parse_address("xG1"), None);
        assert_eq!(parse_address("temp"), None);
        assert_eq!(parse_address("-5"), None);
    }
}
