use super::{
    App, BackgroundTask, ConnectTaskResult, FunctionTaskResult, LoadConfigTaskResult, PendingWrite,
    RawTaskResult, RefreshTaskResult, UnitScanTaskResult, WriteOutcome,
};
use crate::compat::TaskPoll;
use crate::modbus::ModbusDevice;
use crate::state::{ConnectionStatus, StatusMessage};
use std::time::Duration;

impl App {
    pub(super) fn free_background_slot(&mut self) -> bool {
        match self.background_task.as_mut() {
            None => true,
            Some(BackgroundTask::Refresh(handle)) => {
                if matches!(handle.poll_result(), TaskPoll::Pending)
                    && let Some(device) = &self.device
                {
                    device.poison();
                }
                self.background_task = None;
                self.read_or_previous_mut().finish_read();
                true
            }
            Some(_) => false,
        }
    }

    pub fn complete_background_task(&mut self) {
        self.poll_library_task();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_network_scan();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_ports_task();

        enum Done {
            Refresh(Option<RefreshTaskResult>),
            Write(Option<WriteOutcome>),
            Reconnect(Option<Result<ModbusDevice, String>>),
            Connect(Option<ConnectTaskResult>),
            Raw(Option<RawTaskResult>),
            Function(Option<FunctionTaskResult>),
            UnitScan(Option<UnitScanTaskResult>),
            LoadConfig(Option<LoadConfigTaskResult>),
            SunSpecScan(Option<super::library::SunSpecScan>),
        }

        macro_rules! poll_task {
            ($handle:expr, $variant:path) => {
                match $handle.poll_result() {
                    TaskPoll::Pending => return,
                    TaskPoll::Finished(value) => $variant(Some(value)),
                    TaskPoll::Gone => $variant(None),
                }
            };
        }

        let done = match self.background_task.as_mut() {
            None => return,
            Some(BackgroundTask::Refresh(handle)) => poll_task!(handle, Done::Refresh),
            Some(BackgroundTask::Write(handle)) => poll_task!(handle, Done::Write),
            Some(BackgroundTask::Reconnect(handle)) => poll_task!(handle, Done::Reconnect),
            Some(BackgroundTask::Connect(handle)) => poll_task!(handle, Done::Connect),
            Some(BackgroundTask::Raw(handle)) => poll_task!(handle, Done::Raw),
            Some(BackgroundTask::Function(handle)) => poll_task!(handle, Done::Function),
            Some(BackgroundTask::UnitScan(handle)) => poll_task!(handle, Done::UnitScan),
            Some(BackgroundTask::LoadConfig(handle)) => poll_task!(handle, Done::LoadConfig),
            Some(BackgroundTask::SunSpecScan(handle)) => poll_task!(handle, Done::SunSpecScan),
        };
        self.background_task = None;

        match done {
            Done::Reconnect(result) => self.apply_reconnect_result(result),
            Done::Connect(result) => self.apply_connect_result(result),
            Done::Raw(result) => self.apply_raw_result(result),
            Done::Function(result) => self.apply_function_result(result),
            Done::UnitScan(result) => self.apply_unit_scan_result(result),
            Done::LoadConfig(result) => self.apply_load_config_result(result),
            Done::SunSpecScan(result) => self.apply_sunspec_scan(result),
            Done::Refresh(Some(result)) => self.apply_refresh_result(result),
            Done::Refresh(None) => {
                let message = "read task stopped unexpectedly".to_string();
                self.stats().reads.record_error(&message, None);
                let params = self.read_or_previous_mut();
                params.read_error = Some(message.clone());
                params.finish_read();
                log::error!("Read task failed | {message}");
                self.connection = ConnectionStatus::Error(message);
                self.reconnect.link_lost = true;
            }
            Done::Write(outcome) => {
                let outcome = outcome.unwrap_or_else(|| WriteOutcome {
                    ok: false,
                    message: "write task stopped unexpectedly".to_string(),
                    latency: Duration::ZERO,
                    read_back: Vec::new(),
                });
                if outcome.ok {
                    self.stats().writes.record_ok(outcome.latency);
                } else {
                    let target = self.pending_write.as_ref().map(PendingWrite::target);
                    self.stats().writes.record_error(&outcome.message, target);
                }
                if outcome.ok && !outcome.read_back.is_empty() {
                    self.store_values(&outcome.read_back);
                    self.sync_auto_widths();
                }
                if let Some(pending) = &self.pending_write {
                    let detail = format!(
                        "@{} = {} (was {})",
                        pending.address,
                        pending.new_value,
                        pending
                            .previous
                            .map_or_else(|| "?".to_string(), |v| v.to_string()),
                    );
                    if outcome.ok {
                        self.log_write();
                        log::info!("Write {detail}");
                    } else {
                        log::error!("Write failed | {detail} | {}", outcome.message);
                    }
                }
                self.pending_write = None;
                if self.is_reading()
                    && let Some(w) = self.write_mut()
                {
                    w.result = Some(if outcome.ok {
                        StatusMessage::ok(outcome.message)
                    } else {
                        StatusMessage::err(outcome.message)
                    });
                }
            }
        }

        if let Some(queued) = self.queued_write.take() {
            self.start_write(queued);
        }
        self.function_retry_pending();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{App, BackgroundTask};
    use crate::config::Config;
    use crate::register::RegisterType;
    use std::assert_matches;
    use std::time::Duration;

    async fn app_with_refresh_in_flight() -> App {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.refresh();
        assert_matches!(
            app.background_task,
            Some(BackgroundTask::Refresh(_)),
            "boot must land in a reading view so refresh() spawns a task"
        );
        app
    }

    #[tokio::test]
    async fn freeing_a_pending_refresh_poisons_the_kept_device() {
        let mut app = app_with_refresh_in_flight().await;
        let device = app.device.clone().expect("mock device");
        assert!(!device.is_poisoned());

        assert!(app.free_background_slot());
        assert!(app.background_task.is_none());
        assert!(
            device.is_poisoned(),
            "an aborted refresh may have left a reply on the line"
        );

        let result = device.read_typed(None, RegisterType::Holding, 0, 1).await;
        assert!(result.is_ok(), "command after poison failed: {result:?}");
        assert!(!device.is_poisoned());
    }

    #[tokio::test]
    async fn freeing_a_finished_refresh_leaves_the_device_clean() {
        let mut app = app_with_refresh_in_flight().await;
        let device = app.device.clone().expect("mock device");

        tokio::time::sleep(Duration::from_millis(400)).await;

        assert!(app.free_background_slot());
        assert!(app.background_task.is_none());
        assert!(
            !device.is_poisoned(),
            "nothing was interrupted, no reconnect needed"
        );
    }
}
