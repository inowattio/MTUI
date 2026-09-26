use super::{
    AllowUnitFlag, ApiBindState, ApiDevice, App, BindStateFlag, BoundPort, ReadOnlyFlag, StatusFlag,
};
use crate::modbus::ModbusDevice;
use std::sync::atomic::Ordering;

impl App {
    pub fn api_device(&self) -> ApiDevice {
        self.api_device.clone()
    }

    pub fn api_bound_port_handle(&self) -> BoundPort {
        self.api_bound_port.clone()
    }

    pub fn api_read_only_handle(&self) -> ReadOnlyFlag {
        self.api_read_only.clone()
    }

    pub fn api_allow_unit_id_handle(&self) -> AllowUnitFlag {
        self.api_allow_unit_id.clone()
    }

    pub fn api_status_handle(&self) -> StatusFlag {
        self.api_status.clone()
    }

    pub fn api_bind_handle(&self) -> BindStateFlag {
        self.api_bind.clone()
    }

    pub fn api_bind_state(&self) -> ApiBindState {
        ApiBindState::from_code(self.api_bind.load(Ordering::Relaxed))
    }

    pub(super) fn sync_api_read_only(&self) {
        self.api_read_only
            .store(self.config.read_only, Ordering::Relaxed);
    }

    pub(super) fn sync_api_allow_unit_id(&self) {
        self.api_allow_unit_id
            .store(self.config.api.unit_id_override, Ordering::Relaxed);
    }

    pub(super) fn sync_api_status(&self) {
        self.api_status
            .store(self.connection.code(), Ordering::Relaxed);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn reconcile_api_server(&mut self) {
        let desired = self.config.api.desired_port();
        if desired == self.api_server_port {
            return;
        }

        if let Some(handle) = self.api_server.take() {
            handle.abort();
            log::info!("API server stopped");
        }
        self.api_bound_port.store(0, Ordering::Relaxed);
        self.api_bind
            .store(ApiBindState::Pending.code(), Ordering::Relaxed);
        self.api_server_port = desired;

        if let Some(port) = desired {
            self.api_server = Some(tokio::spawn(crate::api::serve(
                port,
                self.api_device(),
                self.api_bound_port_handle(),
                self.writes_log_handle(),
                self.api_read_only_handle(),
                self.api_allow_unit_id_handle(),
                self.api_status_handle(),
                self.api_bind_handle(),
            )));
        }
    }

    pub fn apply_api_port(&mut self) {
        let Some(port) = self.settings_mut().and_then(|s| s.api_port_draft.take()) else {
            return;
        };
        self.config.api.port = port;
        self.refresh_dirty();
        #[cfg(not(target_arch = "wasm32"))]
        self.reconcile_api_server();
    }

    pub fn api_bound_port(&self) -> Option<u16> {
        match self.api_bound_port.load(Ordering::Relaxed) {
            0 => None,
            port => Some(port),
        }
    }

    pub(super) fn sync_api_device(&self) {
        if let Ok(mut slot) = self.api_device.lock() {
            *slot = self.device.clone();
        }
    }

    pub(super) fn take_device(&mut self) -> Option<ModbusDevice> {
        let previous = self.device.take();
        self.sync_api_device();
        previous
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{ApiBindState, App, settle_until};
    use crate::config::Config;
    use crate::handler::handle_key_events;
    use crate::input::{KeyCode, KeyEvent};
    use crate::state::{SettingsCategory, SettingsField, SettingsFocus};

    fn focus_field(app: &mut App, field: SettingsField) {
        let category = SettingsCategory::ALL
            .iter()
            .position(|c| c.fields().contains(&field))
            .unwrap();
        let index = SettingsCategory::ALL[category]
            .fields()
            .iter()
            .position(|&f| f == field)
            .unwrap();
        let s = app.settings_mut().unwrap();
        s.category = category as u16;
        s.focus = SettingsFocus::Fields;
        s.field = index as u16;
    }

    async fn app_on_the_api_port_field(port: u16) -> App {
        let mut config = Config::default();
        config.api.enabled = true;
        config.api.port = port;
        let mut app = App::boot(config, String::new()).await;
        assert_eq!(app.api_server_port, Some(port), "the server starts at boot");

        app.open_settings();
        focus_field(&mut app, SettingsField::ApiPort);
        app
    }

    async fn press(app: &mut App, code: KeyCode) {
        handle_key_events(KeyEvent::new(code), app).await;
        app.reconcile_api_server();
    }

    fn draft(app: &App) -> Option<u16> {
        app.settings().and_then(|s| s.api_port_draft)
    }

    async fn bind_outcome(app: &mut App) -> ApiBindState {
        settle_until(app, |a| a.api_bind_state() != ApiBindState::Pending, "bind").await;
        app.api_bind_state()
    }

    fn screen(app: &mut App) -> String {
        crate::tui::test_util::draw_rows(120, 40, |frame| crate::tui::render(app, frame)).join("\n")
    }

    #[tokio::test]
    async fn stepping_the_port_edits_the_draft_too() {
        let mut app = app_on_the_api_port_field(49000).await;

        press(&mut app, KeyCode::Right).await;
        press(&mut app, KeyCode::Right).await;
        assert_eq!(draft(&app), Some(49002));
        assert_eq!(app.api_server_port, Some(49000));

        press(&mut app, KeyCode::Enter).await;
        assert_eq!(app.api_server_port, Some(49002));
    }

    #[tokio::test]
    async fn leaving_the_field_without_enter_discards_the_draft() {
        for leave in [KeyCode::Down, KeyCode::Esc] {
            let mut app = app_on_the_api_port_field(49000).await;
            press(&mut app, KeyCode::Right).await;

            press(&mut app, leave).await;
            assert_eq!(draft(&app), None, "{leave:?} drops the draft");
            assert_eq!(app.config.api.port, 49000);
            assert_eq!(app.api_server_port, Some(49000));
        }
    }

    #[tokio::test]
    async fn closing_settings_without_enter_keeps_the_applied_port() {
        let mut app = app_on_the_api_port_field(49000).await;
        press(&mut app, KeyCode::Right).await;

        let close = app.config.keybinds.settings;
        press(&mut app, close).await;
        assert!(app.settings().is_none(), "settings closed");
        assert_eq!(app.config.api.port, 49000);
        assert_eq!(app.api_server_port, Some(49000));
    }

    #[tokio::test]
    async fn a_failed_bind_is_reported_only_until_the_port_is_changed() {
        let taken = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = taken.local_addr().unwrap().port();

        let mut app = app_on_the_api_port_field(port).await;
        assert_eq!(bind_outcome(&mut app).await, ApiBindState::Failed);
        assert!(screen(&mut app).contains(&format!("{port} (bind failed)")));

        press(&mut app, KeyCode::Backspace).await;
        let edited = screen(&mut app);
        assert!(!edited.contains("bind failed"), "{edited}");
        assert!(edited.contains(&(port / 10).to_string()));
    }

    #[tokio::test]
    async fn enabling_the_server_and_applying_a_port_shows_that_it_listens() {
        let free = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);

        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_settings();
        focus_field(&mut app, SettingsField::ApiEnabled);

        press(&mut app, KeyCode::Enter).await;
        assert_eq!(bind_outcome(&mut app).await, ApiBindState::Bound);
        assert!(screen(&mut app).contains("any (listening on :"));

        focus_field(&mut app, SettingsField::ApiPort);
        for c in port.to_string().chars() {
            press(&mut app, KeyCode::Char(c)).await;
        }
        assert!(
            !screen(&mut app).contains("listening"),
            "a draft shows no state"
        );

        press(&mut app, KeyCode::Enter).await;
        assert_eq!(bind_outcome(&mut app).await, ApiBindState::Bound);
        let applied = screen(&mut app);
        assert!(
            applied.contains(&format!("{port} (listening)")),
            "{applied}"
        );
    }
}
