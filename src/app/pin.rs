use super::App;
use crate::state::{ReadPanel, StatusMessage};

impl App {
    pub fn clear_pins(&mut self) {
        let n = self.pinned_registers.len();
        self.pinned_registers.clear();
        self.note_cleared(n, "pinned register");
    }

    pub fn pin(&mut self) {
        let (panel, register_type, position, pinned_index) = {
            let p = self.read();
            (p.panel, p.register_type, p.position, p.pinned_index)
        };

        let selection = match panel {
            ReadPanel::Main | ReadPanel::Matrix => (register_type, position),
            _ => match self.panel_cell_at(pinned_index as usize) {
                Some(cell) => cell,
                None => return,
            },
        };

        let pinned = if let Some(pos) = self.pinned_registers.iter().position(|x| *x == selection) {
            self.pinned_registers.remove(pos);
            false
        } else {
            self.pinned_registers.push(selection);
            true
        };

        self.pinned_registers.sort();
        self.refresh_dirty();

        let len = self.panel_len();
        let scroll_rows = self.panel_scroll_rows();
        self.read_mut().scroll_pinned(scroll_rows, len);

        let (kind, addr) = selection;
        let verb = if pinned { "Pinned" } else { "Unpinned" };
        self.set_read_status(StatusMessage::ok(format!("{verb} {kind:?} @{addr}")));
    }
}
