use super::{App, BackgroundTask, DeviceIdTaskResult};
use crate::compat;
use crate::modbus::DeviceIdAccess;
use crate::num_ops::{cycle, step_hscroll};
use crate::state::{DeviceIdParams, Popup, StatusMessage};

impl App {
    pub fn open_device_id(&mut self) {
        self.read_mut().popup = Some(Popup::DeviceId(DeviceIdParams {
            access: DeviceIdAccess::Basic,
            ..Default::default()
        }));
        self.device_id_refresh();
    }

    fn device_id_mut(&mut self) -> Option<&mut DeviceIdParams> {
        self.popup_as_mut()
    }

    pub fn device_id_cycle(&mut self) {
        let Some(params) = self.device_id_mut() else {
            return;
        };
        params.access = cycle(&DeviceIdAccess::ALL, params.access, true);
        params.h_offset = 0;
        if matches!(self.background_task, Some(BackgroundTask::DeviceId(_))) {
            return;
        }
        self.device_id_refresh();
    }

    pub fn device_id_hscroll(&mut self, right: bool) {
        let max = self.h_max_offset.get();
        if let Some(params) = self.device_id_mut() {
            params.h_offset = step_hscroll(params.h_offset, max, right);
        }
    }

    pub fn device_id_refresh(&mut self) {
        if !self.free_background_slot() {
            if let Some(params) = self.device_id_mut() {
                params.status = Some(StatusMessage::info("Device is busy."));
            }
            return;
        }

        let access = match self.device_id_mut() {
            Some(params) => {
                params.loading = true;
                params.status = Some(StatusMessage::info("Reading..."));
                params.access
            }
            None => return,
        };

        let Some(device) = self.device.clone() else {
            if let Some(params) = self.device_id_mut() {
                params.loading = false;
                params.objects.clear();
                params.status = Some(StatusMessage::err("No device connected"));
            }
            return;
        };

        self.background_task = Some(BackgroundTask::DeviceId(compat::spawn(async move {
            let result = device
                .device_identity(access)
                .await
                .map_err(|e| e.to_string());
            DeviceIdTaskResult { access, result }
        })));
    }

    pub(super) fn apply_device_id_result(&mut self, result: Option<DeviceIdTaskResult>) {
        // The popup may have been closed while the read was in flight.
        let Some(current) = self.popup_as::<DeviceIdParams>().map(|p| p.access) else {
            return;
        };
        if result.as_ref().is_some_and(|r| r.access != current) {
            self.device_id_refresh();
            return;
        }
        let Some(params) = self.device_id_mut() else {
            return;
        };
        params.loading = false;
        let Some(DeviceIdTaskResult { access, result }) = result else {
            params.status = Some(StatusMessage::err("Read failed: task stopped unexpectedly"));
            return;
        };
        match result {
            Ok(objects) => {
                log::info!(
                    "Device identification ({}) | {} object(s)",
                    access.label(),
                    objects.len()
                );
                params.status = Some(if objects.is_empty() {
                    StatusMessage::warn("No identification objects returned")
                } else {
                    StatusMessage::ok(format!("Read {} object(s)", objects.len()))
                });
                params.objects = objects;
            }
            Err(e) => {
                log::error!("Device identification failed | {e}");
                params.objects.clear();
                params.status = Some(StatusMessage::err(format!("Read failed: {e}")));
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{App, BackgroundTask};
    use crate::config::Config;
    use crate::modbus::DeviceIdAccess;
    use crate::state::DeviceIdParams;

    fn device_id(app: &App) -> &DeviceIdParams {
        app.popup_as().expect("device id popup")
    }

    async fn settle(app: &mut App) {
        crate::app::settle_until(
            app,
            |app| app.background_task.is_none() && !device_id(app).loading,
            "device id read",
        )
        .await;
    }

    #[tokio::test]
    async fn cycling_during_a_read_shows_the_new_access_level_objects() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_device_id();
        assert!(matches!(
            app.background_task,
            Some(BackgroundTask::DeviceId(_))
        ));

        app.device_id_cycle();
        assert_eq!(device_id(&app).access, DeviceIdAccess::Regular);
        assert!(device_id(&app).loading, "the pending read keeps loading");

        settle(&mut app).await;

        let device = app.device.clone().expect("mock device");
        let expected = device
            .device_identity(DeviceIdAccess::Regular)
            .await
            .expect("mock answers");
        let basic = device
            .device_identity(DeviceIdAccess::Basic)
            .await
            .expect("mock answers");
        assert_ne!(expected, basic, "the test needs distinguishable results");
        assert_eq!(device_id(&app).objects, expected);
        assert_eq!(device_id(&app).access, DeviceIdAccess::Regular);
    }

    #[tokio::test]
    async fn a_result_for_the_selected_access_level_is_applied() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_device_id();
        settle(&mut app).await;

        let device = app.device.clone().expect("mock device");
        let expected = device
            .device_identity(DeviceIdAccess::Basic)
            .await
            .expect("mock answers");
        assert_eq!(device_id(&app).objects, expected);
    }
}
