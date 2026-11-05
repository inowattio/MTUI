use crate::config::AddressMode;
use crate::modbus::DeviceIdAccess;
use crate::state::{FunctionParams, OtherFunction};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagnosticSub {
    #[default]
    ReturnQueryData,
    DiagnosticRegister,
    BusMessages,
    BusCommErrors,
    BusExceptions,
    ServerMessages,
    ServerNoResponse,
    ServerNak,
    ServerBusy,
    CharOverruns,
}

impl DiagnosticSub {
    pub const ALL: [Self; 10] = [
        Self::ReturnQueryData,
        Self::DiagnosticRegister,
        Self::BusMessages,
        Self::BusCommErrors,
        Self::BusExceptions,
        Self::ServerMessages,
        Self::ServerNoResponse,
        Self::ServerNak,
        Self::ServerBusy,
        Self::CharOverruns,
    ];

    pub const fn code(self) -> u16 {
        match self {
            Self::ReturnQueryData => 0x00,
            Self::DiagnosticRegister => 0x02,
            Self::BusMessages => 0x0B,
            Self::BusCommErrors => 0x0C,
            Self::BusExceptions => 0x0D,
            Self::ServerMessages => 0x0E,
            Self::ServerNoResponse => 0x0F,
            Self::ServerNak => 0x10,
            Self::ServerBusy => 0x11,
            Self::CharOverruns => 0x12,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ReturnQueryData => "Return query data",
            Self::DiagnosticRegister => "Diagnostic register",
            Self::BusMessages => "Bus messages",
            Self::BusCommErrors => "Bus comm errors",
            Self::BusExceptions => "Bus exceptions",
            Self::ServerMessages => "Server messages",
            Self::ServerNoResponse => "Server no response",
            Self::ServerNak => "Server NAK",
            Self::ServerBusy => "Server busy",
            Self::CharOverruns => "Char overruns",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionRequest {
    ReadExceptionStatus,
    Diagnostics(DiagnosticSub, Vec<u8>),
    CommEventCounter,
    CommEventLog,
    ReportServerId,
    ReadFileRecord { file: u16, record: u16, length: u16 },
    ReadFifoQueue(u16),
    DeviceIdentification(DeviceIdAccess),
}

impl FunctionRequest {
    pub const fn code(&self) -> u8 {
        match self {
            Self::ReadExceptionStatus => 0x07,
            Self::Diagnostics(..) => 0x08,
            Self::CommEventCounter => 0x0B,
            Self::CommEventLog => 0x0C,
            Self::ReportServerId => 0x11,
            Self::ReadFileRecord { .. } => 0x14,
            Self::ReadFifoQueue(_) => 0x18,
            Self::DeviceIdentification(_) => 0x2B,
        }
    }

    pub const fn function(&self) -> OtherFunction {
        match self {
            Self::ReadExceptionStatus => OtherFunction::ReadExceptionStatus,
            Self::Diagnostics(..) => OtherFunction::Diagnostics,
            Self::CommEventCounter => OtherFunction::CommEventCounter,
            Self::CommEventLog => OtherFunction::CommEventLog,
            Self::ReportServerId => OtherFunction::ReportServerId,
            Self::ReadFileRecord { .. } => OtherFunction::ReadFileRecord,
            Self::ReadFifoQueue(_) => OtherFunction::ReadFifoQueue,
            Self::DeviceIdentification(_) => OtherFunction::DeviceIdentification,
        }
    }

    pub fn data(&self) -> Vec<u8> {
        match self {
            Self::ReadExceptionStatus
            | Self::CommEventCounter
            | Self::CommEventLog
            | Self::ReportServerId => Vec::new(),
            Self::Diagnostics(sub, bytes) => {
                let mut data = sub.code().to_be_bytes().to_vec();
                data.extend_from_slice(bytes);
                data
            }
            Self::ReadFileRecord {
                file,
                record,
                length,
            } => {
                let mut data = vec![0x07, 0x06];
                data.extend_from_slice(&file.to_be_bytes());
                data.extend_from_slice(&record.to_be_bytes());
                data.extend_from_slice(&length.to_be_bytes());
                data
            }
            Self::ReadFifoQueue(address) => address.to_be_bytes().to_vec(),
            Self::DeviceIdentification(access) => vec![0x0E, read_code(*access), 0x00],
        }
    }

    pub fn parse(&self, response: &[u8]) -> Result<Vec<(String, String)>, String> {
        match self {
            Self::ReadExceptionStatus => parse_exception_status(response),
            Self::Diagnostics(sub, sent) => parse_diagnostics(*sub, sent, response),
            Self::CommEventCounter => parse_event_counter(response),
            Self::CommEventLog => parse_event_log(response),
            Self::ReportServerId => parse_server_id(response),
            Self::ReadFileRecord { record, length, .. } => {
                parse_file_record(*record, *length, response)
            }
            Self::ReadFifoQueue(_) => parse_fifo_queue(response),
            Self::DeviceIdentification(_) => {
                Err("device identification is read object by object".to_string())
            }
        }
    }
}

const fn read_code(access: DeviceIdAccess) -> u8 {
    match access {
        DeviceIdAccess::Basic => 0x01,
        DeviceIdAccess::Regular => 0x02,
        DeviceIdAccess::Extended => 0x03,
        DeviceIdAccess::Specific => 0x04,
    }
}

const fn object_name(id: u8) -> Option<&'static str> {
    match id {
        0x00 => Some("Vendor name"),
        0x01 => Some("Product code"),
        0x02 => Some("Revision"),
        0x03 => Some("Vendor URL"),
        0x04 => Some("Product name"),
        0x05 => Some("Model name"),
        0x06 => Some("Application"),
        _ => None,
    }
}

pub fn device_id_rows(objects: &[(u8, String)]) -> Vec<(String, String)> {
    if objects.is_empty() {
        return vec![row("Objects", "none")];
    }
    objects
        .iter()
        .map(|(id, value)| {
            let label = object_name(*id).map_or_else(
                || format!("0x{id:02X}"),
                |name| format!("0x{id:02X} {name}"),
            );
            row(label, value.clone())
        })
        .collect()
}

type Rows = Vec<(String, String)>;

fn row(label: impl Into<String>, value: impl Into<String>) -> (String, String) {
    (label.into(), value.into())
}

fn word(bytes: &[u8], index: usize) -> Option<u16> {
    let high = *bytes.get(index)?;
    let low = *bytes.get(index + 1)?;
    Some(u16::from_be_bytes([high, low]))
}

fn hex_rows(label: &str, bytes: &[u8]) -> Rows {
    if bytes.is_empty() {
        return vec![row(label, "none")];
    }
    bytes
        .chunks(16)
        .enumerate()
        .map(|(index, chunk)| {
            let value = chunk
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(" ");
            row(if index == 0 { label } else { "" }, value)
        })
        .collect()
}

fn register_value(value: u16) -> String {
    format!("{value}  0x{value:04X}")
}

fn comm_status(status: u16) -> String {
    match status {
        0x0000 => "ready".to_string(),
        0xFFFF => "busy".to_string(),
        other => format!("0x{other:04X}"),
    }
}

fn parse_exception_status(response: &[u8]) -> Result<Rows, String> {
    let [status] = response else {
        return Err(format!("expected 1 byte, got {}", response.len()));
    };
    let bits = (0..8)
        .filter(|bit| status & (1 << bit) != 0)
        .map(|bit| bit.to_string())
        .collect::<Vec<_>>();
    let bits = if bits.is_empty() {
        "none".to_string()
    } else {
        bits.join(", ")
    };
    Ok(vec![
        row("Status", format!("0b{status:08b} (0x{status:02X})")),
        row("Bits set", bits),
    ])
}

fn parse_diagnostics(sub: DiagnosticSub, sent: &[u8], response: &[u8]) -> Result<Rows, String> {
    let echoed =
        word(response, 0).ok_or_else(|| format!("response too short: {} bytes", response.len()))?;
    if echoed != sub.code() {
        return Err(format!(
            "sub-function mismatch: sent 0x{:04X}, got 0x{echoed:04X}",
            sub.code()
        ));
    }
    let data = response.get(2..).unwrap_or_default();
    match sub {
        DiagnosticSub::ReturnQueryData => {
            let mut rows = hex_rows("Echo", data);
            rows.push(row("Match", if data == sent { "yes" } else { "no" }));
            Ok(rows)
        }
        DiagnosticSub::DiagnosticRegister => {
            let value = two_byte_data(data)?;
            Ok(vec![row(
                sub.label(),
                format!("0x{value:04X} (0b{value:016b})"),
            )])
        }
        _ => {
            let value = two_byte_data(data)?;
            Ok(vec![row(sub.label(), value.to_string())])
        }
    }
}

fn two_byte_data(data: &[u8]) -> Result<u16, String> {
    match data {
        [high, low] => Ok(u16::from_be_bytes([*high, *low])),
        _ => Err(format!("expected 2 data bytes, got {}", data.len())),
    }
}

fn parse_event_counter(response: &[u8]) -> Result<Rows, String> {
    let (Some(status), Some(count), 4) = (word(response, 0), word(response, 2), response.len())
    else {
        return Err(format!("expected 4 bytes, got {}", response.len()));
    };
    Ok(vec![
        row("Status", comm_status(status)),
        row("Event count", count.to_string()),
    ])
}

fn event_text(event: u8) -> String {
    let (kind, flags): (&str, &[(u8, &str)]) = match event {
        0x00 => return "communication restart".to_string(),
        0x04 => return "entered listen only mode".to_string(),
        _ if event & 0x80 != 0 => (
            "receive",
            &[
                (1, "comm error"),
                (4, "char overrun"),
                (5, "listen only"),
                (6, "broadcast"),
            ],
        ),
        _ if event & 0x40 != 0 => (
            "send",
            &[
                (0, "read exception"),
                (1, "server abort"),
                (2, "server busy"),
                (3, "program NAK"),
                (4, "write timeout"),
                (5, "listen only"),
            ],
        ),
        _ => return "unknown".to_string(),
    };
    let set = flags
        .iter()
        .filter(|(bit, _)| event & (1 << bit) != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>();
    if set.is_empty() {
        kind.to_string()
    } else {
        format!("{kind}: {}", set.join(", "))
    }
}

fn parse_event_log(response: &[u8]) -> Result<Rows, String> {
    let Some((&count, body)) = response.split_first() else {
        return Err("empty response".to_string());
    };
    if count < 6 {
        return Err(format!("byte count {count} is less than 6"));
    }
    if usize::from(count) != body.len() {
        return Err(format!(
            "byte count {count} does not match {} received bytes",
            body.len()
        ));
    }
    let (Some(status), Some(events), Some(messages)) =
        (word(body, 0), word(body, 2), word(body, 4))
    else {
        return Err("response too short".to_string());
    };
    let mut rows = vec![
        row("Status", comm_status(status)),
        row("Event count", events.to_string()),
        row("Message count", messages.to_string()),
    ];
    let log = body.get(6..).unwrap_or_default();
    if log.is_empty() {
        rows.push(row("Events", "none"));
    }
    rows.extend(log.iter().enumerate().map(|(index, &event)| {
        row(
            format!("Event {index}"),
            format!("0x{event:02X} {}", event_text(event)),
        )
    }));
    Ok(rows)
}

fn parse_server_id(response: &[u8]) -> Result<Rows, String> {
    let Some((&count, body)) = response.split_first() else {
        return Err("empty response".to_string());
    };
    if count < 2 {
        return Err(format!("byte count {count} is less than 2"));
    }
    if usize::from(count) != body.len() {
        return Err(format!(
            "byte count {count} does not match {} received bytes",
            body.len()
        ));
    }
    let [id, run, extra @ ..] = body else {
        return Err("response too short".to_string());
    };
    let run = match run {
        0x00 => "OFF".to_string(),
        0xFF => "ON".to_string(),
        other => format!("0x{other:02X}"),
    };
    let mut rows = vec![
        row("Server ID", format!("0x{id:02X} ({id})")),
        row("Run indicator", run),
    ];
    rows.extend(hex_rows("Data", extra));
    if let Some(text) = printable_text(extra) {
        rows.push(row("Text", text));
    }
    Ok(rows)
}

fn printable_text(bytes: &[u8]) -> Option<String> {
    let end = bytes
        .iter()
        .rposition(|&byte| byte != 0 && byte != b' ')
        .map_or(0, |index| index + 1);
    let text = bytes.get(..end)?;
    if text.is_empty() || !text.iter().all(|&byte| (0x20..=0x7E).contains(&byte)) {
        return None;
    }
    String::from_utf8(text.to_vec())
        .ok()
        .map(|text| text.trim().to_string())
}

fn parse_file_record(record: u16, length: u16, response: &[u8]) -> Result<Rows, String> {
    let Some((&data_length, body)) = response.split_first() else {
        return Err("empty response".to_string());
    };
    if usize::from(data_length) != body.len() {
        return Err(format!(
            "response data length {data_length} does not match {} received bytes",
            body.len()
        ));
    }
    let [file_length, reference, data @ ..] = body else {
        return Err("response too short".to_string());
    };
    if usize::from(*file_length) != data.len() + 1 {
        return Err(format!(
            "file response length {file_length} does not match {} data bytes",
            data.len()
        ));
    }
    if *reference != 0x06 {
        return Err(format!("reference type 0x{reference:02X}, expected 0x06"));
    }
    if data.len() % 2 != 0 {
        return Err(format!("odd number of data bytes: {}", data.len()));
    }
    let registers = data.len() / 2;
    if registers != usize::from(length) {
        return Err(format!("requested {length} registers, got {registers}"));
    }
    Ok(data
        .as_chunks::<2>()
        .0
        .iter()
        .zip(u32::from(record)..)
        .map(|(pair, number)| {
            let value = u16::from_be_bytes(*pair);
            row(number.to_string(), register_value(value))
        })
        .collect())
}

fn parse_fifo_queue(response: &[u8]) -> Result<Rows, String> {
    let (Some(byte_count), Some(count)) = (word(response, 0), word(response, 2)) else {
        return Err(format!("response too short: {} bytes", response.len()));
    };
    let body = response.len() - 2;
    if usize::from(byte_count) != body {
        return Err(format!(
            "byte count {byte_count} does not match {body} received bytes"
        ));
    }
    if count > 31 {
        return Err(format!("FIFO count {count} exceeds 31"));
    }
    if usize::from(byte_count) != 2 + 2 * usize::from(count) {
        return Err(format!(
            "byte count {byte_count} does not match FIFO count {count}"
        ));
    }
    let mut rows = vec![row("FIFO count", count.to_string())];
    rows.extend(
        response
            .get(4..)
            .unwrap_or_default()
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .map(|(index, pair)| {
                row(
                    format!("[{index}]"),
                    register_value(u16::from_be_bytes(*pair)),
                )
            }),
    );
    Ok(rows)
}

fn parse_hex_data(text: &str) -> Result<Vec<u8>, String> {
    let digits = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<_>>();
    if !digits.iter().all(char::is_ascii_hexdigit) {
        return Err("data must be hex bytes".to_string());
    }
    if digits.len() % 2 != 0 {
        return Err("data must have an even number of hex digits".to_string());
    }
    let bytes = digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let text = pair.iter().collect::<String>();
            u8::from_str_radix(&text, 16).map_err(|_| "data must be hex bytes".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if bytes.len() < 2 {
        return Err("data must be at least 2 bytes".to_string());
    }
    if bytes.len() % 2 != 0 {
        return Err("data must be an even number of bytes".to_string());
    }
    if bytes.len() > 250 {
        return Err("data must be at most 250 bytes".to_string());
    }
    Ok(bytes)
}

fn parse_decimal(text: &str, name: &str) -> Result<u32, String> {
    text.trim()
        .parse::<u32>()
        .map_err(|_| format!("invalid {name}"))
}

fn build_file_record(params: &FunctionParams) -> Result<FunctionRequest, String> {
    let file = parse_decimal(&params.file, "file number")?;
    let record = parse_decimal(&params.record, "record number")?;
    let length = parse_decimal(&params.length, "record length")?;
    let file = u16::try_from(file)
        .ok()
        .filter(|&file| file >= 1)
        .ok_or_else(|| "file number must be 1-65535".to_string())?;
    let record = u16::try_from(record)
        .ok()
        .filter(|&record| record <= 9999)
        .ok_or_else(|| "record number must be 0-9999".to_string())?;
    let length = u16::try_from(length)
        .ok()
        .filter(|length| (1..=121).contains(length))
        .ok_or_else(|| "record length must be 1-121".to_string())?;
    if u32::from(record) + u32::from(length) > 10000 {
        return Err("record + length must not exceed 10000".to_string());
    }
    Ok(FunctionRequest::ReadFileRecord {
        file,
        record,
        length,
    })
}

pub fn build_request(
    params: &FunctionParams,
    mode: AddressMode,
) -> Result<FunctionRequest, String> {
    match params.function {
        OtherFunction::ReadExceptionStatus => Ok(FunctionRequest::ReadExceptionStatus),
        OtherFunction::Diagnostics => {
            let data = if params.sub == DiagnosticSub::ReturnQueryData {
                parse_hex_data(&params.data)?
            } else {
                vec![0x00, 0x00]
            };
            Ok(FunctionRequest::Diagnostics(params.sub, data))
        }
        OtherFunction::CommEventCounter => Ok(FunctionRequest::CommEventCounter),
        OtherFunction::CommEventLog => Ok(FunctionRequest::CommEventLog),
        OtherFunction::ReportServerId => Ok(FunctionRequest::ReportServerId),
        OtherFunction::ReadFileRecord => build_file_record(params),
        OtherFunction::ReadFifoQueue => mode
            .parse(&params.address)
            .map(FunctionRequest::ReadFifoQueue)
            .ok_or_else(|| "invalid FIFO address".to_string()),
        OtherFunction::DeviceIdentification => {
            Ok(FunctionRequest::DeviceIdentification(params.access))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DiagnosticSub, FunctionRequest, build_request, device_id_rows};
    use crate::config::AddressMode;
    use crate::modbus::DeviceIdAccess;
    use crate::state::{FunctionParams, OtherFunction};

    fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(label, value)| ((*label).to_string(), (*value).to_string()))
            .collect()
    }

    fn file_params(file: &str, record: &str, length: &str) -> FunctionParams {
        let mut params = FunctionParams::new(OtherFunction::ReadFileRecord);
        params.file = file.to_string();
        params.record = record.to_string();
        params.length = length.to_string();
        params
    }

    fn diagnostics_params(sub: DiagnosticSub, data: &str) -> FunctionParams {
        let mut params = FunctionParams::new(OtherFunction::Diagnostics);
        params.sub = sub;
        params.data = data.to_string();
        params
    }

    #[test]
    fn data_encodes_each_request() {
        assert!(FunctionRequest::ReadExceptionStatus.data().is_empty());
        assert!(FunctionRequest::CommEventCounter.data().is_empty());
        assert!(FunctionRequest::CommEventLog.data().is_empty());
        assert!(FunctionRequest::ReportServerId.data().is_empty());
        assert_eq!(
            FunctionRequest::Diagnostics(DiagnosticSub::ReturnQueryData, vec![0xA5, 0x37]).data(),
            vec![0x00, 0x00, 0xA5, 0x37]
        );
        assert_eq!(
            FunctionRequest::Diagnostics(DiagnosticSub::CharOverruns, vec![0, 0]).data(),
            vec![0x00, 0x12, 0x00, 0x00]
        );
        assert_eq!(
            FunctionRequest::ReadFileRecord {
                file: 4,
                record: 1,
                length: 2
            }
            .data(),
            vec![0x07, 0x06, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02]
        );
        assert_eq!(
            FunctionRequest::ReadFifoQueue(0x04DE).data(),
            vec![0x04, 0xDE]
        );
    }

    #[test]
    fn exception_status_lists_set_bits() {
        assert_eq!(
            FunctionRequest::ReadExceptionStatus.parse(&[0x6D]),
            Ok(rows(&[
                ("Status", "0b01101101 (0x6D)"),
                ("Bits set", "0, 2, 3, 5, 6")
            ]))
        );
        assert_eq!(
            FunctionRequest::ReadExceptionStatus.parse(&[0x00]),
            Ok(rows(&[
                ("Status", "0b00000000 (0x00)"),
                ("Bits set", "none")
            ]))
        );
        assert!(FunctionRequest::ReadExceptionStatus.parse(&[]).is_err());
        assert!(FunctionRequest::ReadExceptionStatus.parse(&[1, 2]).is_err());
    }

    #[test]
    fn diagnostics_echo_compares_with_sent_data() {
        let request =
            FunctionRequest::Diagnostics(DiagnosticSub::ReturnQueryData, vec![0xA5, 0x37]);
        assert_eq!(
            request.parse(&[0x00, 0x00, 0xA5, 0x37]),
            Ok(rows(&[("Echo", "A5 37"), ("Match", "yes")]))
        );
        assert_eq!(
            request.parse(&[0x00, 0x00, 0xA5, 0x38]),
            Ok(rows(&[("Echo", "A5 38"), ("Match", "no")]))
        );
    }

    #[test]
    fn diagnostics_echo_splits_long_data_into_rows() {
        let sent = (0..18).collect::<Vec<u8>>();
        let request = FunctionRequest::Diagnostics(DiagnosticSub::ReturnQueryData, sent.clone());
        let mut response = vec![0x00, 0x00];
        response.extend_from_slice(&sent);
        assert_eq!(
            request.parse(&response),
            Ok(rows(&[
                ("Echo", "00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F"),
                ("", "10 11"),
                ("Match", "yes")
            ]))
        );
    }

    #[test]
    fn diagnostics_register_and_counters() {
        let register = FunctionRequest::Diagnostics(DiagnosticSub::DiagnosticRegister, vec![0, 0]);
        assert_eq!(
            register.parse(&[0x00, 0x02, 0x81, 0x01]),
            Ok(rows(&[(
                "Diagnostic register",
                "0x8101 (0b1000000100000001)"
            )]))
        );
        let counter = FunctionRequest::Diagnostics(DiagnosticSub::BusMessages, vec![0, 0]);
        assert_eq!(
            counter.parse(&[0x00, 0x0B, 0x01, 0x2C]),
            Ok(rows(&[("Bus messages", "300")]))
        );
        assert!(counter.parse(&[0x00, 0x0B, 0x01]).is_err());
        assert!(counter.parse(&[0x00, 0x0B, 0x01, 0x02, 0x03]).is_err());
        assert!(register.parse(&[0x00, 0x02]).is_err());
    }

    #[test]
    fn diagnostics_rejects_short_or_mismatched_echo() {
        let counter = FunctionRequest::Diagnostics(DiagnosticSub::BusMessages, vec![0, 0]);
        assert!(counter.parse(&[]).is_err());
        assert!(counter.parse(&[0x00]).is_err());
        assert_eq!(
            counter.parse(&[0x00, 0x0C, 0x00, 0x01]),
            Err("sub-function mismatch: sent 0x000B, got 0x000C".to_string())
        );
    }

    #[test]
    fn comm_event_counter_decodes_status() {
        let request = FunctionRequest::CommEventCounter;
        assert_eq!(
            request.parse(&[0xFF, 0xFF, 0x01, 0x08]),
            Ok(rows(&[("Status", "busy"), ("Event count", "264")]))
        );
        assert_eq!(
            request.parse(&[0x00, 0x00, 0x00, 0x03]),
            Ok(rows(&[("Status", "ready"), ("Event count", "3")]))
        );
        assert_eq!(
            request.parse(&[0x12, 0x34, 0x00, 0x00]),
            Ok(rows(&[("Status", "0x1234"), ("Event count", "0")]))
        );
        assert!(request.parse(&[0x00, 0x00, 0x00]).is_err());
        assert!(request.parse(&[0x00, 0x00, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn comm_event_log_decodes_events() {
        let response = [
            0x0D, 0x00, 0x00, 0x01, 0x08, 0x01, 0x21, 0x20, 0x00, 0x82, 0x40, 0x04, 0x13, 0xC0,
        ];
        assert_eq!(
            FunctionRequest::CommEventLog.parse(&response),
            Ok(rows(&[
                ("Status", "ready"),
                ("Event count", "264"),
                ("Message count", "289"),
                ("Event 0", "0x20 unknown"),
                ("Event 1", "0x00 communication restart"),
                ("Event 2", "0x82 receive: comm error"),
                ("Event 3", "0x40 send"),
                ("Event 4", "0x04 entered listen only mode"),
                ("Event 5", "0x13 unknown"),
                ("Event 6", "0xC0 receive: broadcast"),
            ]))
        );
    }

    #[test]
    fn comm_event_log_decodes_flags() {
        let response = [0x08, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0xF2, 0x7F];
        assert_eq!(
            FunctionRequest::CommEventLog.parse(&response),
            Ok(rows(&[
                ("Status", "busy"),
                ("Event count", "0"),
                ("Message count", "0"),
                (
                    "Event 0",
                    "0xF2 receive: comm error, char overrun, listen only, broadcast"
                ),
                (
                    "Event 1",
                    "0x7F send: read exception, server abort, server busy, program NAK, write timeout, listen only"
                ),
            ]))
        );
    }

    #[test]
    fn comm_event_log_without_events_and_malformed() {
        let request = FunctionRequest::CommEventLog;
        assert_eq!(
            request.parse(&[0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02]),
            Ok(rows(&[
                ("Status", "ready"),
                ("Event count", "1"),
                ("Message count", "2"),
                ("Events", "none"),
            ]))
        );
        assert!(request.parse(&[]).is_err());
        assert!(
            request
                .parse(&[0x05, 0x00, 0x00, 0x00, 0x00, 0x00])
                .is_err()
        );
        assert!(
            request
                .parse(&[0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02])
                .is_err()
        );
        assert!(
            request
                .parse(&[0x06, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x00])
                .is_err()
        );
    }

    #[test]
    fn report_server_id_shows_data_and_text() {
        let mut response = vec![0x00, 0x11, 0xFF];
        response.extend_from_slice(b"MTUI test device 1\0\0");
        response[0] = u8::try_from(response.len() - 1).unwrap();
        assert_eq!(
            FunctionRequest::ReportServerId.parse(&response),
            Ok(rows(&[
                ("Server ID", "0x11 (17)"),
                ("Run indicator", "ON"),
                ("Data", "4D 54 55 49 20 74 65 73 74 20 64 65 76 69 63 65"),
                ("", "20 31 00 00"),
                ("Text", "MTUI test device 1"),
            ]))
        );
    }

    #[test]
    fn report_server_id_skips_text_for_binary_data() {
        let request = FunctionRequest::ReportServerId;
        assert_eq!(
            request.parse(&[0x04, 0x01, 0x00, 0x41, 0x01]),
            Ok(rows(&[
                ("Server ID", "0x01 (1)"),
                ("Run indicator", "OFF"),
                ("Data", "41 01"),
            ]))
        );
        assert_eq!(
            request.parse(&[0x02, 0x02, 0x05]),
            Ok(rows(&[
                ("Server ID", "0x02 (2)"),
                ("Run indicator", "0x05"),
                ("Data", "none"),
            ]))
        );
        assert_eq!(
            request.parse(&[0x04, 0x02, 0xFF, 0x00, 0x20]),
            Ok(rows(&[
                ("Server ID", "0x02 (2)"),
                ("Run indicator", "ON"),
                ("Data", "00 20"),
            ]))
        );
    }

    #[test]
    fn report_server_id_rejects_malformed() {
        let request = FunctionRequest::ReportServerId;
        assert!(request.parse(&[]).is_err());
        assert!(request.parse(&[0x01, 0x01]).is_err());
        assert!(request.parse(&[0x03, 0x01, 0xFF]).is_err());
        assert!(request.parse(&[0x02, 0x01, 0xFF, 0x00]).is_err());
    }

    #[test]
    fn read_file_record_lists_registers() {
        let request = FunctionRequest::ReadFileRecord {
            file: 4,
            record: 9998,
            length: 2,
        };
        assert_eq!(
            request.parse(&[0x06, 0x05, 0x06, 0x0D, 0xFE, 0x00, 0x20]),
            Ok(rows(&[("9998", "3582  0x0DFE"), ("9999", "32  0x0020")]))
        );
    }

    #[test]
    fn read_file_record_rejects_malformed() {
        let request = FunctionRequest::ReadFileRecord {
            file: 4,
            record: 1,
            length: 2,
        };
        assert!(request.parse(&[]).is_err());
        assert!(request.parse(&[0x00]).is_err());
        assert!(request.parse(&[0x01, 0x01]).is_err());
        assert!(
            request
                .parse(&[0x07, 0x05, 0x06, 0x0D, 0xFE, 0x00, 0x20])
                .is_err()
        );
        assert!(
            request
                .parse(&[0x06, 0x07, 0x06, 0x0D, 0xFE, 0x00, 0x20])
                .is_err()
        );
        assert!(
            request
                .parse(&[0x06, 0x05, 0x07, 0x0D, 0xFE, 0x00, 0x20])
                .is_err()
        );
        assert!(request.parse(&[0x04, 0x03, 0x06, 0x0D, 0xFE]).is_err());
        assert!(
            request
                .parse(&[0x05, 0x04, 0x06, 0x0D, 0xFE, 0x00])
                .is_err()
        );
    }

    #[test]
    fn fifo_queue_lists_values() {
        let request = FunctionRequest::ReadFifoQueue(0x04DE);
        assert_eq!(
            request.parse(&[0x00, 0x06, 0x00, 0x02, 0x01, 0xB8, 0x12, 0x84]),
            Ok(rows(&[
                ("FIFO count", "2"),
                ("[0]", "440  0x01B8"),
                ("[1]", "4740  0x1284"),
            ]))
        );
        assert_eq!(
            request.parse(&[0x00, 0x02, 0x00, 0x00]),
            Ok(rows(&[("FIFO count", "0")]))
        );
    }

    #[test]
    fn fifo_queue_rejects_malformed() {
        let request = FunctionRequest::ReadFifoQueue(0);
        assert!(request.parse(&[]).is_err());
        assert!(request.parse(&[0x00, 0x02, 0x00]).is_err());
        assert!(request.parse(&[0x00, 0x04, 0x00, 0x01, 0x00]).is_err());
        assert!(
            request
                .parse(&[0x00, 0x04, 0x00, 0x02, 0x00, 0x01])
                .is_err()
        );
        let mut too_many = vec![0x00, 0x42, 0x00, 0x20];
        too_many.extend(std::iter::repeat_n(0, 64));
        assert!(request.parse(&too_many).is_err());
    }

    #[test]
    fn parse_never_panics_on_truncated_input() {
        let requests = [
            FunctionRequest::ReadExceptionStatus,
            FunctionRequest::Diagnostics(DiagnosticSub::ReturnQueryData, vec![1, 2]),
            FunctionRequest::Diagnostics(DiagnosticSub::DiagnosticRegister, vec![0, 0]),
            FunctionRequest::CommEventCounter,
            FunctionRequest::CommEventLog,
            FunctionRequest::ReportServerId,
            FunctionRequest::ReadFileRecord {
                file: 1,
                record: 0,
                length: 1,
            },
            FunctionRequest::ReadFifoQueue(0),
        ];
        let bytes = [0xFF_u8; 12];
        for request in &requests {
            for len in 0..=bytes.len() {
                let _ = request.parse(&bytes[..len]);
                let _ = request.parse(&vec![0; len]);
            }
        }
    }

    #[test]
    fn build_request_maps_simple_functions() {
        let mode = AddressMode::Dec;
        assert_eq!(
            build_request(
                &FunctionParams::new(OtherFunction::ReadExceptionStatus),
                mode
            ),
            Ok(FunctionRequest::ReadExceptionStatus)
        );
        assert_eq!(
            build_request(&FunctionParams::new(OtherFunction::CommEventCounter), mode),
            Ok(FunctionRequest::CommEventCounter)
        );
        assert_eq!(
            build_request(&FunctionParams::new(OtherFunction::CommEventLog), mode),
            Ok(FunctionRequest::CommEventLog)
        );
        assert_eq!(
            build_request(&FunctionParams::new(OtherFunction::ReportServerId), mode),
            Ok(FunctionRequest::ReportServerId)
        );
        let mut device_id = FunctionParams::new(OtherFunction::DeviceIdentification);
        device_id.access = DeviceIdAccess::Extended;
        assert_eq!(
            build_request(&device_id, mode),
            Ok(FunctionRequest::DeviceIdentification(
                DeviceIdAccess::Extended
            ))
        );
    }

    #[test]
    fn device_identification_encodes_read_code() {
        assert_eq!(
            FunctionRequest::DeviceIdentification(DeviceIdAccess::Basic).data(),
            vec![0x0E, 0x01, 0x00]
        );
        assert_eq!(
            FunctionRequest::DeviceIdentification(DeviceIdAccess::Regular).data(),
            vec![0x0E, 0x02, 0x00]
        );
        assert_eq!(
            FunctionRequest::DeviceIdentification(DeviceIdAccess::Basic).code(),
            0x2B
        );
    }

    #[test]
    fn device_id_rows_name_known_objects() {
        assert_eq!(
            device_id_rows(&[
                (0x00, "Acme".to_string()),
                (0x04, "Meter".to_string()),
                (0x91, "custom".to_string()),
            ]),
            rows(&[
                ("0x00 Vendor name", "Acme"),
                ("0x04 Product name", "Meter"),
                ("0x91", "custom"),
            ])
        );
        assert_eq!(device_id_rows(&[]), rows(&[("Objects", "none")]));
    }

    #[test]
    fn build_request_validates_diagnostic_data() {
        let mode = AddressMode::Dec;
        let build = |data: &str| {
            build_request(
                &diagnostics_params(DiagnosticSub::ReturnQueryData, data),
                mode,
            )
        };
        assert_eq!(
            build("a5 37"),
            Ok(FunctionRequest::Diagnostics(
                DiagnosticSub::ReturnQueryData,
                vec![0xA5, 0x37]
            ))
        );
        assert_eq!(
            build("12345678"),
            Ok(FunctionRequest::Diagnostics(
                DiagnosticSub::ReturnQueryData,
                vec![0x12, 0x34, 0x56, 0x78]
            ))
        );
        assert!(build("").is_err());
        assert!(build("12").is_err());
        assert!(build("123").is_err());
        assert!(build("123456").is_err());
        assert!(build("12 3g").is_err());
        assert!(build("0x1234").is_err());
        assert!(build(&"00".repeat(250)).is_ok());
        assert!(build(&"00".repeat(252)).is_err());
        assert_eq!(
            build_request(&diagnostics_params(DiagnosticSub::ServerBusy, "zz"), mode),
            Ok(FunctionRequest::Diagnostics(
                DiagnosticSub::ServerBusy,
                vec![0x00, 0x00]
            ))
        );
    }

    #[test]
    fn build_request_validates_file_record_fields() {
        let build = |file: &str, record: &str, length: &str| {
            build_request(&file_params(file, record, length), AddressMode::Dec)
        };
        assert_eq!(
            build("4", " 1 ", "2"),
            Ok(FunctionRequest::ReadFileRecord {
                file: 4,
                record: 1,
                length: 2
            })
        );
        assert!(build("65535", "9879", "121").is_ok());
        assert_eq!(
            build("0", "0", "1"),
            Err("file number must be 1-65535".to_string())
        );
        assert!(build("65536", "0", "1").is_err());
        assert_eq!(build("", "0", "1"), Err("invalid file number".to_string()));
        assert_eq!(
            build("1", "x", "1"),
            Err("invalid record number".to_string())
        );
        assert_eq!(
            build("1", "0", "-1"),
            Err("invalid record length".to_string())
        );
        assert!(build("1", "9999", "1").is_ok());
        assert!(build("1", "10000", "1").is_err());
        assert!(build("1", "0", "0").is_err());
        assert!(build("1", "0", "122").is_err());
        assert!(build("1", "9990", "11").is_err());
        assert!(build("1", "9990", "10").is_ok());
    }

    #[test]
    fn build_request_parses_fifo_address_in_mode() {
        let mut params = FunctionParams::new(OtherFunction::ReadFifoQueue);
        params.address = "1246".to_string();
        assert_eq!(
            build_request(&params, AddressMode::Dec),
            Ok(FunctionRequest::ReadFifoQueue(1246))
        );
        params.address = "4DE".to_string();
        assert_eq!(
            build_request(&params, AddressMode::Hex),
            Ok(FunctionRequest::ReadFifoQueue(0x04DE))
        );
        params.address = "0x10".to_string();
        assert_eq!(
            build_request(&params, AddressMode::Dec),
            Ok(FunctionRequest::ReadFifoQueue(0x10))
        );
        params.address = "70000".to_string();
        assert_eq!(
            build_request(&params, AddressMode::Dec),
            Err("invalid FIFO address".to_string())
        );
        params.address = String::new();
        assert!(build_request(&params, AddressMode::Dec).is_err());
    }
}
