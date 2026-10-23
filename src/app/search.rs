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

        let prefixed_type = match query.chars().next() {
            Some('h' | 'H') => Some(RegisterType::Holding),
            Some('i' | 'I') => Some(RegisterType::Input),
            Some('c' | 'C') => Some(RegisterType::Coil),
            Some('d' | 'D') => Some(RegisterType::Discrete),
            _ => None,
        };

        let mut readings: Vec<(RegisterType, &str)> = Vec::new();
        let starts_with_hex_digit = query.starts_with(|c: char| c.is_ascii_hexdigit());
        if prefixed_type.is_none() || (address_mode == AddressMode::Hex && starts_with_hex_digit) {
            readings.push((current_type, query.as_str()));
        }
        if let Some(register_type) = prefixed_type {
            readings.push((register_type, &query[1..]));
        }

        let mut matches: Vec<SearchMatch> = Vec::new();
        for (register_type, numeric_query) in readings {
            let Some((address, hint)) = parse_target(numeric_query, current_position, address_mode)
            else {
                continue;
            };
            let cell = (register_type, address);
            if matches.iter().any(|m| m.cell == cell) {
                continue;
            }
            matches.push(match self.labels.get(&cell) {
                Some(label) => SearchMatch::label(cell, label.clone()),
                None => SearchMatch::hint(cell, hint),
            });
        }
        let targets: Vec<RegisterCell> = matches.iter().map(|m| m.cell).collect();

        let mut scored: Vec<(i32, RegisterCell, String)> = self
            .labels
            .iter()
            .filter(|&(cell, _)| !targets.contains(cell))
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
        return mode
            .parse_saturating(input)
            .map(|address| (address, "jump to this address".to_string()));
    };
    let offset = mode.parse_saturating(rest)?;
    let address = (i32::from(current) + sign * i32::from(offset)).clamp(0, i32::from(u16::MAX));
    let symbol = if sign > 0 { '+' } else { '-' };
    Some((
        address as u16,
        format!("move by {symbol}{}", mode.display(offset)),
    ))
}

#[cfg(test)]
mod tests {
    use super::{AddressMode, parse_target};
    use crate::app::App;
    use crate::config::Config;
    use crate::register::RegisterType;
    use crate::state::SearchParams;

    #[cfg(not(target_arch = "wasm32"))]
    async fn search(mode: AddressMode, query: &str) -> Vec<(RegisterType, u16)> {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.interpreter.set_address_mode(mode);
        app.read_mut().register_type = RegisterType::Holding;
        app.open_search();
        for c in query.chars() {
            app.search_input(c);
        }
        app.popup_as::<SearchParams>()
            .map(|s| s.matches.iter().map(|m| m.cell).collect())
            .unwrap_or_default()
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn hex_mode_offers_the_address_before_the_type_prefix() {
        let cells = search(AddressMode::Hex, "C8").await;
        assert_eq!(
            cells[..2],
            [(RegisterType::Holding, 0xC8), (RegisterType::Coil, 8)]
        );
        let cells = search(AddressMode::Hex, "d0").await;
        assert_eq!(
            cells[..2],
            [(RegisterType::Holding, 0xD0), (RegisterType::Discrete, 0)]
        );
        let cells = search(AddressMode::Hex, "C").await;
        assert_eq!(cells.first(), Some(&(RegisterType::Holding, 0xC)));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn non_hex_prefixes_and_decimal_mode_keep_one_reading() {
        let cells = search(AddressMode::Hex, "i10").await;
        assert_eq!(cells.first(), Some(&(RegisterType::Input, 0x10)));
        assert!(!cells.contains(&(RegisterType::Holding, 0x10)));
        let cells = search(AddressMode::Dec, "c8").await;
        assert_eq!(cells.first(), Some(&(RegisterType::Coil, 8)));
        assert!(
            !cells
                .iter()
                .any(|c| c.0 == RegisterType::Holding && c.1 == 8)
        );
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
            Some((10, "move by +A".to_string()))
        );
        assert_eq!(
            parse_target("+10", 0, AddressMode::Hex),
            Some((16, "move by +10".to_string()))
        );
        assert_eq!(
            parse_target("-x10", 0x20, AddressMode::Hex),
            Some((0x10, "move by -10".to_string()))
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
