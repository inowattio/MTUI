use super::App;
use crate::num_ops::{digit_add, digit_remove, wrap_index};
use crate::state::{Popup, SweepConfigParams, SweepField};

impl App {
    fn sweep_config_mut(&mut self) -> Option<&mut SweepConfigParams> {
        self.popup_as_mut()
    }

    pub fn open_sweep(&mut self) {
        if !self.is_reading() {
            return;
        }
        let params = SweepConfigParams {
            from: self.sweep.from,
            to: self.sweep.to,
            continuous: self.sweep.continuous,
            selected: (SweepField::ALL.len() - 1) as u16,
        };
        self.read_mut().popup = Some(Popup::SweepConfig(params));
    }

    pub fn sweep_action(&mut self) {
        let Some(p) = self.popup_as::<SweepConfigParams>() else {
            return;
        };
        let (mut from, mut to, continuous) = (p.from, p.to, p.continuous);
        if to < from {
            std::mem::swap(&mut from, &mut to);
        }
        self.sweep.from = from;
        self.sweep.to = to;
        self.sweep.continuous = continuous;

        if self.sweep.active {
            self.sweep.active = false;
            log::info!("Sweep stopped");
        } else {
            self.sweep.current = from;
            self.sweep.errored = false;
            self.sweep.active = true;
            let rows = self.visible_rows.get();
            let cols = self.config.matrix_cols;
            {
                let p = self.read_mut();
                p.position = from;
                p.scroll_to_cursor(rows, cols);
            }
            log::info!(
                "Sweep started \u{b7} {from}..{to}{}",
                if continuous { " (loop)" } else { "" }
            );
        }
        self.close_popup();
    }

    pub(super) fn advance_sweep(&mut self, errored: bool) {
        let batch = self.config.registers_batch.max(1);
        let advance = if errored || self.sweep.errored {
            1
        } else {
            batch
        };
        self.sweep.errored = errored;

        if self.sweep.current >= self.sweep.to {
            if self.sweep.continuous {
                self.sweep.current = self.sweep.from;
                self.sweep.errored = false;
            } else {
                self.sweep.active = false;
                log::info!("Sweep complete");
            }
        } else {
            self.sweep.current = self
                .sweep
                .current
                .saturating_add(advance)
                .min(self.sweep.to);
        }
    }

    pub fn sweep_config_move(&mut self, down: bool) {
        if let Some(p) = self.sweep_config_mut() {
            let n = SweepField::ALL.len() as u16;
            p.selected = wrap_index(p.selected, n, down);
        }
    }

    pub fn sweep_config_toggle(&mut self) {
        if let Some(p) = self.sweep_config_mut() {
            p.continuous = !p.continuous;
        }
    }

    pub fn sweep_config_digit(&mut self, field: SweepField, c: char) {
        if !c.is_ascii_digit() {
            return;
        }
        let digit = c as u8 - b'0';
        if let Some(p) = self.sweep_config_mut() {
            match field {
                SweepField::From => digit_add(&mut p.from, digit),
                SweepField::To => digit_add(&mut p.to, digit),
                SweepField::Mode | SweepField::Action => {}
            }
        }
    }

    pub fn sweep_config_backspace(&mut self, field: SweepField) {
        if let Some(p) = self.sweep_config_mut() {
            match field {
                SweepField::From => digit_remove(&mut p.from),
                SweepField::To => digit_remove(&mut p.to),
                SweepField::Mode | SweepField::Action => {}
            }
        }
    }
}
