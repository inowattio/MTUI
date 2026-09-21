use super::App;
use crate::state::{Popup, UnitParams};

impl App {
    pub fn open_unit(&mut self) {
        let (address, amount) = self.read_window();
        let register_type = self.read().register_type;
        let id = self.config.device.unit_id;
        let endpoint = self.config.device.interface.endpoint();
        let remembered = match self.unit_scan.take() {
            Some((previous, params)) if previous == endpoint => Some(params),
            _ => None,
        };
        let params = remembered
            .unwrap_or_default()
            .resumed(id, register_type, address, amount);
        self.read_mut().popup = Some(Popup::Unit(params));
    }

    pub async fn commit_unit(&mut self) {
        let id = self.popup_as::<UnitParams>().map(|p| p.id);
        if let Some(id) = id {
            self.apply_unit(id).await;
        }
    }

    pub async fn commit_unit_hit(&mut self, index: usize) {
        let id = self
            .popup_as::<UnitParams>()
            .and_then(|p| p.hits.get(index))
            .map(|hit| hit.unit_id);
        let Some(id) = id else {
            return;
        };
        self.set_unit(id).await;
        if let Some(p) = self.popup_as_mut::<UnitParams>() {
            p.id = id;
        }
    }

    async fn apply_unit(&mut self, id: u8) {
        self.set_unit(id).await;
        self.close_popup();
        self.refresh().await;
    }

    async fn set_unit(&mut self, id: u8) {
        if let Some(device) = &self.device {
            device.set_unit(id).await;
        }
        self.config.device.unit_id = id;
        self.refresh_writes_log_state();
        self.refresh_dirty();
        log::info!("Unit id set to {id}");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::state::UnitParams;

    #[tokio::test]
    async fn changing_the_unit_id_marks_the_config_dirty() {
        let mut app = App::boot(Config::default(), String::new()).await;
        assert!(!app.dirty);
        let original = app.config.device.unit_id;

        app.open_unit();
        app.popup_as_mut::<UnitParams>().unwrap().id = original.wrapping_add(1);
        app.commit_unit().await;
        assert_eq!(app.config.device.unit_id, original.wrapping_add(1));
        assert!(app.dirty, "a new unit id is an unsaved change");

        app.open_unit();
        app.popup_as_mut::<UnitParams>().unwrap().id = original;
        app.commit_unit().await;
        assert!(!app.dirty, "restoring the saved id is clean again");
    }
}
