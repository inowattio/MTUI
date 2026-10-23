use super::{App, fuzzy_score};
use crate::config::AddressMode;
use crate::register::{RegisterCell, RegisterType};
use crate::state::{Popup, ReadPanel, SearchMatch, SearchParams};

impl App {
    fn search_mut(&mut self) -> Option<&mut SearchParams> {
        self.popup_as_mut()
    }

    pub fn open_search(&mut self) {
        self.read_mut().popup = Some(Popup::Search(SearchParams::default()));
        self.recompute_search();
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
        let rows = self.layout.search_rows;
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

        self.jump_to_cell(register_type, position);
        self.read_mut().popup = None;
    }

    fn jump_to_cell(&mut self, register_type: RegisterType, position: u16) {
        if register_type != self.read().register_type {
            self.stop_sweep();
        }
        let p = self.read_mut();
        if p.panel != ReadPanel::Matrix {
            p.panel = ReadPanel::Main;
        }
        p.register_type = register_type;
        p.position = position;
        self.scroll_to_cursor();
    }

    fn recompute_search(&mut self) {
        let Some(query) = self.popup_as::<SearchParams>().map(|s| s.query.clone()) else {
            return;
        };
        let address_mode = self.interpreter.address_mode();
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

        if let Some((address, hint)) = parse_target(numeric_query, current_position, address_mode) {
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

        let rows = self.layout.search_rows;
        if let Some(s) = self.search_mut() {
            s.matches = matches;
            s.selected = 0;
            s.top = 0;
            s.scroll(rows);
        }
    }
}

fn parse_target(input: &str, current: u16, mode: AddressMode) -> Option<(u16, String)> {
    let input = input.trim();
    let (sign, rest) = if let Some(rest) = input.strip_prefix('+') {
        (1, rest)
    } else if let Some(rest) = input.strip_prefix('-') {
        (-1, rest)
    } else {
        return parse_address(input, mode)
            .map(|address| (address, "jump to this address".to_string()));
    };
    let offset = parse_address(rest, mode)?;
    let address = (i32::from(current) + sign * i32::from(offset)).clamp(0, i32::from(u16::MAX));
    let symbol = if sign > 0 { '+' } else { '-' };
    Some((address as u16, format!("move by {symbol}{offset}")))
}

fn parse_address(input: &str, mode: AddressMode) -> Option<u16> {
    let input = input.trim();
    let (digits, radix) = if let Some(dec) = input.strip_prefix(['n', 'N']) {
        (dec, 10)
    } else if let Some(hex) = input.strip_prefix(['x', 'X']) {
        (hex, 16)
    } else if let Some(hex) = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
    {
        (hex, 16)
    } else {
        (input, u32::from(mode.radix()))
    };
    let value = u32::from_str_radix(digits, radix).ok()?;
    Some(value.min(u16::MAX as u32) as u16)
}

#[cfg(test)]
mod tests {
    use super::{AddressMode, parse_address, parse_target};

    #[test]
    fn parses_bare_addresses_in_the_configured_mode() {
        assert_eq!(parse_address("111", AddressMode::Dec), Some(111));
        assert_eq!(parse_address(" 42 ", AddressMode::Dec), Some(42));
        assert_eq!(parse_address("70000", AddressMode::Dec), Some(u16::MAX));
        assert_eq!(parse_address("6F", AddressMode::Hex), Some(0x6F));
        assert_eq!(parse_address("10000", AddressMode::Hex), Some(u16::MAX));
    }

    #[test]
    fn prefixes_force_the_radix() {
        assert_eq!(parse_address("x6F", AddressMode::Dec), Some(0x6F));
        assert_eq!(parse_address("X6f", AddressMode::Dec), Some(0x6F));
        assert_eq!(parse_address("0x6F", AddressMode::Dec), Some(0x6F));
        assert_eq!(parse_address("xFFFF", AddressMode::Dec), Some(u16::MAX));
        assert_eq!(parse_address("x10000", AddressMode::Dec), Some(u16::MAX));
        assert_eq!(parse_address("n111", AddressMode::Hex), Some(111));
        assert_eq!(parse_address("N42", AddressMode::Hex), Some(42));
        assert_eq!(parse_address("n6F", AddressMode::Hex), None);
    }

    #[test]
    fn relative_targets_move_from_the_cursor() {
        assert_eq!(
            parse_target("+69", 100, AddressMode::Dec),
            Some((169, "move by +69".to_string()))
        );
        assert_eq!(
            parse_target("-100", 250, AddressMode::Dec),
            Some((150, "move by -100".to_string()))
        );
        assert_eq!(
            parse_target("+x10", 0, AddressMode::Dec),
            Some((16, "move by +16".to_string()))
        );
        assert_eq!(
            parse_target("+n10", 0, AddressMode::Hex),
            Some((10, "move by +10".to_string()))
        );
        assert_eq!(
            parse_target(" - 5 ", 10, AddressMode::Dec),
            Some((5, "move by -5".to_string()))
        );
    }

    #[test]
    fn relative_targets_clamp_to_the_address_range() {
        assert_eq!(
            parse_target("-100", 50, AddressMode::Dec).map(|t| t.0),
            Some(0)
        );
        assert_eq!(
            parse_target("+100", 65500, AddressMode::Dec).map(|t| t.0),
            Some(u16::MAX)
        );
    }
}
