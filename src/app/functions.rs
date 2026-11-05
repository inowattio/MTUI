use super::{App, BackgroundTask, FunctionTaskResult, StatTarget};
use crate::compat;
use crate::config::AddressMode;
use crate::constants::message;
use crate::functions::{DiagnosticSub, FunctionRequest, build_request, device_id_rows};
use crate::modbus::DeviceIdAccess;
use crate::num_ops::cycle;
use crate::state::{
    FunctionField, FunctionParams, FunctionsParams, OtherFunction, Popup, StatusMessage,
};

const NUMBER_DIGITS: usize = 5;

fn accepts(field: FunctionField, text: &str, c: char, mode: AddressMode) -> bool {
    match field {
        FunctionField::SubFunction | FunctionField::Access => false,
        FunctionField::Data => c.is_ascii_hexdigit() || c == ' ',
        FunctionField::File | FunctionField::Record | FunctionField::Length => {
            c.is_ascii_digit() && text.len() < NUMBER_DIGITS
        }
        FunctionField::Address => mode.accepts(text, c),
    }
}

impl App {
    pub fn open_functions(&mut self) {
        self.read_mut().popup = Some(Popup::Functions(FunctionsParams::default()));
    }

    pub fn open_device_id(&mut self) {
        let params = FunctionsParams {
            selected: OtherFunction::ALL
                .iter()
                .position(|&f| f == OtherFunction::DeviceIdentification)
                .unwrap_or_default(),
            ..FunctionsParams::default()
        };
        self.read_mut().popup = Some(Popup::Functions(params));
        self.function_send();
    }

    fn functions_mut(&mut self) -> Option<&mut FunctionsParams> {
        self.popup_as_mut()
    }

    fn function_mut(&mut self) -> Option<&mut FunctionParams> {
        self.functions_mut().map(FunctionsParams::current_mut)
    }

    fn form_mut(&mut self, function: OtherFunction) -> Option<&mut FunctionParams> {
        self.functions_mut()
            .and_then(|params| params.form_mut(function))
    }

    pub fn functions_select(&mut self, forward: bool) {
        if let Some(params) = self.functions_mut() {
            let len = params.forms.len();
            let current = params.selected.min(len - 1);
            params.selected = if forward {
                (current + 1) % len
            } else {
                (current + len - 1) % len
            };
        }
    }

    pub fn functions_focus(&mut self, detail: bool) {
        if let Some(params) = self.functions_mut() {
            params.detail = detail && !params.current().fields().is_empty();
        }
    }

    pub fn functions_enter(&mut self) {
        let has_fields = self
            .popup_as::<FunctionsParams>()
            .is_some_and(|params| !params.current().fields().is_empty());
        if has_fields {
            self.functions_focus(true);
        } else {
            self.function_send();
        }
    }

    pub fn function_move(&mut self, forward: bool) {
        if let Some(params) = self.function_mut() {
            let len = params.fields().len();
            if len > 0 {
                let current = params.field.min(len - 1);
                params.field = if forward {
                    (current + 1) % len
                } else {
                    (current + len - 1) % len
                };
            }
        }
    }

    pub fn function_cycle(&mut self, forward: bool) {
        let Some(params) = self.function_mut() else {
            return;
        };
        match params.current_field() {
            Some(FunctionField::SubFunction) => {
                params.sub = cycle(&DiagnosticSub::ALL, params.sub, forward);
                params.field = 0;
            }
            Some(FunctionField::Access) => {
                params.access = cycle(&DeviceIdAccess::ALL, params.access, forward);
            }
            _ => return,
        }
        params.rows.clear();
        params.scroll = 0;
        params.status = None;
        params.pending = false;
    }

    pub fn function_accepts(&self, c: char) -> bool {
        let mode = self.interpreter.address_mode();
        let Some(params) = self
            .popup_as::<FunctionsParams>()
            .map(FunctionsParams::current)
        else {
            return false;
        };
        let Some(field) = params.current_field() else {
            return false;
        };
        let text = match field {
            FunctionField::SubFunction | FunctionField::Access => return false,
            FunctionField::Data => &params.data,
            FunctionField::File => &params.file,
            FunctionField::Record => &params.record,
            FunctionField::Length => &params.length,
            FunctionField::Address => &params.address,
        };
        accepts(field, text, c, mode)
    }

    pub fn function_input(&mut self, c: char) {
        let mode = self.interpreter.address_mode();
        let Some(params) = self.function_mut() else {
            return;
        };
        let Some(field) = params.current_field() else {
            return;
        };
        if params.pending {
            params.pending = false;
            params.status = None;
        }
        let Some(text) = params.text_mut(field) else {
            return;
        };
        if accepts(field, text, c, mode) {
            text.push(c);
        }
    }

    pub fn function_backspace(&mut self) {
        if let Some(params) = self.function_mut()
            && let Some(field) = params.current_field()
        {
            if params.pending {
                params.pending = false;
                params.status = None;
            }
            if let Some(text) = params.text_mut(field) {
                text.pop();
            }
        }
    }

    pub fn function_scroll(&mut self, down: bool) {
        let max = self.layout.v_max_offset;
        if let Some(params) = self.function_mut() {
            params.scroll = if down {
                params.scroll.saturating_add(1).min(max)
            } else {
                params.scroll.min(max).saturating_sub(1)
            };
        }
    }

    pub fn function_send(&mut self) {
        let Some(function) = self
            .popup_as::<FunctionsParams>()
            .map(|params| params.current().function)
        else {
            return;
        };
        self.function_send_for(function);
    }

    fn function_send_for(&mut self, function: OtherFunction) {
        let mode = self.interpreter.address_mode();
        let request = match self.form_mut(function) {
            Some(form) => build_request(form, mode),
            None => return,
        };
        let request = match request {
            Ok(request) => request,
            Err(e) => {
                if let Some(form) = self.form_mut(function) {
                    form.pending = false;
                    form.loading = false;
                    form.status = Some(StatusMessage::err(e));
                }
                return;
            }
        };

        let Some(device) = self.device.clone() else {
            if let Some(form) = self.form_mut(function) {
                form.pending = false;
                form.loading = false;
                form.status = Some(StatusMessage::err(message::NO_DEVICE));
            }
            return;
        };

        if !self.free_background_slot() {
            if let Some(form) = self.form_mut(function) {
                form.pending = true;
                form.status = Some(StatusMessage::info(message::DEVICE_BUSY));
            }
            return;
        }

        if let Some(form) = self.form_mut(function) {
            form.pending = false;
            form.loading = true;
            form.status = Some(StatusMessage::info(message::SENDING));
        }

        self.background_task = Some(BackgroundTask::Function(compat::spawn(async move {
            let result = match &request {
                FunctionRequest::DeviceIdentification(access) => {
                    let started = compat::Instant::now();
                    device
                        .device_identity(*access)
                        .await
                        .map(|objects| (started.elapsed(), Ok(device_id_rows(&objects))))
                }
                _ => device
                    .function_timed(request.code(), &request.data())
                    .await
                    .map(|(bytes, latency)| (latency, request.parse(&bytes))),
            }
            .map_err(|e| e.to_string());
            FunctionTaskResult { request, result }
        })));
    }

    pub(super) fn function_retry_pending(&mut self) {
        if self.background_task.is_some() {
            return;
        }
        let pending = self.popup_as::<FunctionsParams>().and_then(|params| {
            params
                .forms
                .iter()
                .find(|form| form.pending)
                .map(|form| form.function)
        });
        if let Some(function) = pending {
            self.function_send_for(function);
        }
    }

    pub(super) fn apply_function_result(&mut self, result: Option<FunctionTaskResult>) {
        match &result {
            Some(FunctionTaskResult {
                result: Ok((latency, _)),
                ..
            }) => self.stats().others.record_ok(*latency),
            Some(FunctionTaskResult {
                request,
                result: Err(e),
            }) => self
                .stats()
                .others
                .record_error(e, Some(StatTarget::Function(request.code()))),
            None => self
                .stats()
                .others
                .record_error("function task stopped unexpectedly", None),
        }
        let mode = self.interpreter.address_mode();
        let Some(params) = self.functions_mut() else {
            return;
        };
        let Some(FunctionTaskResult { request, result }) = result else {
            for form in params.forms.iter_mut().filter(|form| form.loading) {
                form.loading = false;
                form.status = Some(StatusMessage::err(message::TASK_STOPPED));
            }
            return;
        };
        let Some(form) = params.form_mut(request.function()) else {
            return;
        };
        if !form.loading {
            return;
        }
        form.loading = false;
        if build_request(form, mode).as_ref() != Ok(&request) {
            if !form.pending {
                form.status = Some(StatusMessage::info(message::FUNCTION_INPUTS_CHANGED));
            }
            return;
        }
        form.pending = false;
        match result.and_then(|(_, rows)| rows) {
            Ok(rows) => {
                log::info!("{} | {} row(s)", form.function.label(), rows.len());
                form.status = Some(StatusMessage::ok(format!("{} ok", form.function.label())));
                form.rows = rows;
                form.scroll = 0;
            }
            Err(e) => {
                log::error!("{} failed | {e}", form.function.label());
                form.rows.clear();
                form.scroll = 0;
                form.status = Some(StatusMessage::err(format!("Failed: {e}")));
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::App;
    use crate::config::Config;
    use crate::functions::device_id_rows;
    use crate::modbus::DeviceIdAccess;
    use crate::state::{FunctionParams, FunctionsParams, OtherFunction};

    fn form(app: &App, function: OtherFunction) -> &FunctionParams {
        app.popup_as::<FunctionsParams>()
            .and_then(|params| params.forms.iter().find(|f| f.function == function))
            .expect("functions popup")
    }

    async fn settle(app: &mut App) {
        crate::app::settle_until(
            app,
            |app| {
                app.background_task.is_none()
                    && app
                        .popup_as::<FunctionsParams>()
                        .is_some_and(|p| p.forms.iter().all(|f| !f.loading && !f.pending))
            },
            "function request",
        )
        .await;
    }

    fn select(app: &mut App, function: OtherFunction) {
        while app
            .popup_as::<FunctionsParams>()
            .is_some_and(|p| p.current().function != function)
        {
            app.functions_select(true);
        }
    }

    #[tokio::test]
    async fn the_device_id_keybind_opens_and_reads_device_identification() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_device_id();
        assert_eq!(
            app.popup_as::<FunctionsParams>()
                .map(|p| p.current().function),
            Some(OtherFunction::DeviceIdentification)
        );
        settle(&mut app).await;

        let device = app.device.clone().expect("mock device");
        let basic = device
            .device_identity(DeviceIdAccess::Basic)
            .await
            .expect("mock answers");
        assert_eq!(
            form(&app, OtherFunction::DeviceIdentification).rows,
            device_id_rows(&basic)
        );
    }

    #[tokio::test]
    async fn device_identification_reads_the_selected_access_level() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_functions();
        select(&mut app, OtherFunction::DeviceIdentification);
        app.functions_enter();
        app.function_cycle(true);
        app.function_send();
        settle(&mut app).await;

        let device = app.device.clone().expect("mock device");
        let regular = device
            .device_identity(DeviceIdAccess::Regular)
            .await
            .expect("mock answers");
        let form = form(&app, OtherFunction::DeviceIdentification);
        assert_eq!(form.access, DeviceIdAccess::Regular);
        assert_eq!(form.rows, device_id_rows(&regular));
    }

    #[tokio::test]
    async fn a_send_blocked_by_a_busy_device_runs_once_it_is_free() {
        let mut app = App::boot(Config::default(), String::new()).await;
        app.open_functions();
        select(&mut app, OtherFunction::ReportServerId);
        app.functions_enter();
        select(&mut app, OtherFunction::CommEventCounter);
        app.functions_enter();
        assert!(form(&app, OtherFunction::CommEventCounter).pending);

        settle(&mut app).await;

        assert!(!form(&app, OtherFunction::ReportServerId).rows.is_empty());
        assert!(!form(&app, OtherFunction::CommEventCounter).rows.is_empty());
    }
}
