use super::*;

impl App {
    pub fn open_slave(&mut self) {
        let current = self.config.device.slave_id as u16;
        self.read_mut().popup = Some(Popup::Slave(current));
    }

    pub async fn commit_slave(&mut self) {
        let id = match &self.read().popup {
            Some(Popup::Slave(value)) => Some((*value).min(u8::MAX as u16) as u8),
            _ => None,
        };
        if let Some(id) = id {
            if let Some(device) = &self.device {
                device.set_slave(id).await;
            }
            self.config.device.slave_id = id;
            self.refresh_writes_log_state();
            log::info!("Slave id set to {id}");
            self.read_mut().popup = None;
            self.refresh().await;
        }
    }

    pub async fn open_device_id(&mut self) {
        self.read_mut().popup = Some(Popup::DeviceId(DeviceIdParams {
            access: DeviceIdAccess::Basic,
            ..Default::default()
        }));
        self.device_id_refresh().await;
    }

    fn device_id_mut(&mut self) -> Option<&mut DeviceIdParams> {
        match &mut self.state {
            State::Read(p) => match &mut p.popup {
                Some(Popup::DeviceId(d)) => Some(d),
                _ => None,
            },
            _ => None,
        }
    }

    pub async fn device_id_cycle(&mut self, forward: bool) {
        let Some(params) = self.device_id_mut() else {
            return;
        };
        params.access = cycle(&DeviceIdAccess::ALL, params.access, forward);
        self.device_id_refresh().await;
    }

    pub async fn device_id_refresh(&mut self) {
        let access = match self.device_id_mut() {
            Some(params) => {
                params.loading = true;
                params.status = Some(StatusMessage::info("Reading\u{2026}"));
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

        let result = device.device_identity(access).await;

        let Some(params) = self.device_id_mut() else {
            return;
        };
        params.loading = false;
        match result {
            Ok(objects) => {
                log::info!(
                    "Device identification ({}) \u{b7} {} object(s)",
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
                log::error!("Device identification failed \u{b7} {e}");
                params.objects.clear();
                params.status = Some(StatusMessage::err(format!("Read failed: {e}")));
            }
        }
    }

    pub fn open_raw(&mut self) {
        self.read_mut().popup = Some(Popup::Raw(RawParams::default()));
    }

    fn raw_mut(&mut self) -> Option<&mut RawParams> {
        match &mut self.state {
            State::Read(p) => match &mut p.popup {
                Some(Popup::Raw(r)) => Some(r),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn raw_move(&mut self, down: bool) {
        if let Some(p) = self.raw_mut() {
            let n = RawField::ALL.len() as u16;
            p.selected = wrap_index(p.selected, n, down);
        }
    }

    pub fn raw_input(&mut self, c: char) {
        if let Some(p) = self.raw_mut() {
            match p.current_field() {
                RawField::Code if c.is_ascii_digit() && p.code.len() < 3 => p.code.push(c),
                RawField::Data if c.is_ascii_hexdigit() || c == ' ' => p.data.push(c),
                _ => {}
            }
        }
    }

    pub fn raw_backspace(&mut self) {
        if let Some(p) = self.raw_mut() {
            match p.current_field() {
                RawField::Code => {
                    p.code.pop();
                }
                RawField::Data => {
                    p.data.pop();
                }
            }
        }
    }

    pub async fn raw_send(&mut self) {
        if self.config.read_only {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::warn(
                    "Read-only mode is on \u{2014} custom calls may write and are disabled",
                ));
            }
            return;
        }

        let (code_str, data_str) = match self.raw_mut() {
            Some(p) => (p.code.trim().to_string(), p.data.clone()),
            None => return,
        };

        let code = match code_str.parse::<u16>() {
            Ok(value) if value <= u8::MAX as u16 => value as u8,
            _ => {
                if let Some(p) = self.raw_mut() {
                    p.status = Some(StatusMessage::err("Function code must be 0\u{2013}255"));
                }
                return;
            }
        };

        let data = match parse_hex_bytes(&data_str) {
            Ok(data) => data,
            Err(e) => {
                if let Some(p) = self.raw_mut() {
                    p.status = Some(StatusMessage::err(e));
                }
                return;
            }
        };

        let Some(device) = self.device.clone() else {
            if let Some(p) = self.raw_mut() {
                p.status = Some(StatusMessage::err("No device connected"));
            }
            return;
        };

        if let Some(p) = self.raw_mut() {
            p.status = Some(StatusMessage::info("Sending\u{2026}"));
        }

        let result = device.custom(code, &data).await;

        // The popup may have been closed while the call was in flight.
        let Some(p) = self.raw_mut() else {
            return;
        };
        match result {
            Ok(bytes) => {
                log::info!(
                    "Raw function {code:#04X} \u{b7} {} byte(s) in, {} byte(s) out",
                    data.len(),
                    bytes.len()
                );
                p.response = Some(if bytes.is_empty() {
                    "(empty)".to_string()
                } else {
                    bytes
                        .iter()
                        .map(|byte| format!("{byte:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                p.status = Some(StatusMessage::ok(format!(
                    "{} byte(s) returned",
                    bytes.len()
                )));
            }
            Err(e) => {
                log::error!("Raw function {code:#04X} failed \u{b7} {e}");
                p.response = None;
                p.status = Some(StatusMessage::err(format!("Failed: {e}")));
            }
        }
    }
}
