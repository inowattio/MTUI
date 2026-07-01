use super::{
    AllowSlaveFlag, ApiBindState, ApiDevice, App, BindStateFlag, BoundPort, ReadOnlyFlag,
    StatusFlag,
};
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

    pub fn api_allow_slave_id_handle(&self) -> AllowSlaveFlag {
        self.api_allow_slave_id.clone()
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

    pub(super) fn sync_api_allow_slave_id(&self) {
        self.api_allow_slave_id
            .store(self.config.allow_api_slave_id, Ordering::Relaxed);
    }

    pub(super) fn sync_api_status(&self) {
        self.api_status
            .store(self.connection.code(), Ordering::Relaxed);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn reconcile_api_server(&mut self) {
        let desired = self.config.port;
        if desired == self.api_server_port {
            self.api_pending_port = desired;
            return;
        }
        if self.api_pending_port != desired {
            self.api_pending_port = desired;
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
                self.api_allow_slave_id_handle(),
                self.api_status_handle(),
                self.api_bind_handle(),
            )));
        }
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
}
