use super::*;

impl App {
    pub fn open_help(&mut self) {
        self.read_mut().popup = Some(Popup::Help(HelpParams::default()));
    }

    pub fn help_input(&mut self, c: char) {
        if let Some(Popup::Help(h)) = &mut self.read_mut().popup {
            h.query.push(c);
            h.selected = 0;
        }
    }

    pub fn help_backspace(&mut self) {
        if let Some(Popup::Help(h)) = &mut self.read_mut().popup {
            h.query.pop();
            h.selected = 0;
        }
    }

    pub fn help_move(&mut self, down: bool) {
        let count = self.help_matches().len() as u16;
        if let Some(Popup::Help(h)) = &mut self.read_mut().popup {
            if count == 0 {
                h.selected = 0;
            } else {
                h.selected = wrap_index(h.selected, count, down);
            }
        }
    }

    pub fn help_matches(&self) -> Vec<KeybindAction> {
        let Some(Popup::Help(h)) = &self.read().popup else {
            return Vec::new();
        };
        fuzzy_rank(&h.query, KeybindAction::ALL, |a| a.label())
    }

    pub fn help_selected_action(&self) -> Option<KeybindAction> {
        match &self.read().popup {
            Some(Popup::Help(h)) if !h.query.is_empty() => {
                self.help_matches().get(h.selected as usize).copied()
            }
            _ => None,
        }
    }

    pub fn help_commit(&mut self) -> Option<KeybindAction> {
        let action = self.help_selected_action();
        if action.is_some() {
            self.close_popup();
        }
        action
    }
}
