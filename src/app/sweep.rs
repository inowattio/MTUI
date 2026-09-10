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
            self.stop_sweep();
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
                "Sweep started | {from}..{to}{}",
                if continuous { " (loop)" } else { "" }
            );
        }
        self.close_popup();
    }

    pub(super) fn stop_sweep(&mut self) {
        if self.sweep.active {
            self.sweep.active = false;
            log::info!("Sweep stopped");
        }
    }

    pub(super) fn advance_sweep(&mut self, errored: bool) {
        let (start, amount) = self.read_window();
        let covered = if errored { 1 } else { amount };
        self.sweep.errored = errored;

        let end = start.saturating_add(covered - 1);
        if end >= self.sweep.to {
            if self.sweep.continuous {
                self.sweep.current = self.sweep.from;
                self.sweep.errored = false;
            } else {
                self.sweep.active = false;
                log::info!("Sweep complete");
            }
        } else {
            self.sweep.current = end + 1;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::config::Config;

    async fn sweeping(from: u16, to: u16, batch: u16, continuous: bool) -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.config.registers_batch = batch;
        app.sweep.from = from;
        app.sweep.to = to;
        app.sweep.continuous = continuous;
        app.sweep.current = from;
        app.sweep.active = true;
        app.read_mut().position = from;
        app
    }

    fn step(app: &mut App, errored: bool) -> (u16, u16) {
        app.read_mut().position = app.sweep.current;
        let window = app.read_window();
        app.advance_sweep(errored);
        window
    }

    #[tokio::test]
    async fn the_last_window_stops_at_the_upper_bound() {
        let mut app = sweeping(0, 10, 4, false).await;
        assert_eq!(step(&mut app, false), (0, 4));
        assert_eq!(step(&mut app, false), (4, 4));
        assert_eq!(step(&mut app, false), (8, 3));
        assert!(!app.sweep.active, "complete once the bound is covered");
    }

    #[tokio::test]
    async fn an_exact_multiple_is_not_read_twice() {
        let mut app = sweeping(0, 7, 4, false).await;
        assert_eq!(step(&mut app, false), (0, 4));
        assert_eq!(step(&mut app, false), (4, 4));
        assert!(!app.sweep.active);
    }

    #[tokio::test]
    async fn a_continuous_sweep_wraps_to_from() {
        let mut app = sweeping(5, 6, 4, true).await;
        assert_eq!(step(&mut app, false), (5, 2));
        assert!(app.sweep.active);
        assert_eq!(app.sweep.current, 5);
    }

    #[tokio::test]
    async fn errors_step_one_register_until_a_read_succeeds() {
        let mut app = sweeping(0, 10, 4, false).await;
        assert_eq!(step(&mut app, true), (0, 4));
        assert_eq!(app.sweep.current, 1);
        assert_eq!(step(&mut app, false), (1, 1));
        assert_eq!(step(&mut app, false), (2, 4));
        assert_eq!(app.sweep.current, 6);
    }

    #[tokio::test]
    async fn a_batch_wider_than_the_range_reads_it_in_one_go() {
        let mut app = sweeping(0, 10, 20, false).await;
        assert_eq!(step(&mut app, false), (0, 11));
        assert!(!app.sweep.active);

        let mut app = sweeping(0, 10, 20, true).await;
        assert_eq!(step(&mut app, false), (0, 11));
        assert!(app.sweep.active);
        assert_eq!(app.sweep.current, 0);
    }

    #[tokio::test]
    async fn a_single_register_range_completes_in_one_read() {
        let mut app = sweeping(65535, 65535, 10, false).await;
        assert_eq!(step(&mut app, false), (65535, 1));
        assert!(!app.sweep.active);
    }
}
