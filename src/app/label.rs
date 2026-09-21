use super::App;
use crate::state::{LabelParams, Popup};

impl App {
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
        self.sync_auto_widths();
        self.refresh_dirty();

        self.read_mut().popup = None;
    }

    pub fn clear_labels(&mut self) {
        let n = self.labels.len();
        self.labels.clear();
        self.sync_auto_widths();
        self.note_cleared(n, "label");
    }
}
