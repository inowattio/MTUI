use super::{Direction, Frame, Framing, crc16, hex};
use crate::config::AddressMode;

const CHUNK: usize = 4;
const PAIR_WINDOW: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Heading,
    Plain,
    Good,
    Bad,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub bytes: Vec<u8>,
    pub label: String,
    pub tone: Tone,
}

impl Field {
    fn new(bytes: &[u8], label: impl Into<String>) -> Self {
        Self {
            bytes: bytes.to_vec(),
            label: label.into(),
            tone: Tone::Plain,
        }
    }

    fn heading(label: impl Into<String>) -> Self {
        Self {
            bytes: Vec::new(),
            label: label.into(),
            tone: Tone::Heading,
        }
    }

    const fn with(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }

    pub fn hex(&self) -> String {
        hex(&self.bytes)
    }
}

pub fn request_for(frames: &[Frame], index: usize) -> Option<&Frame> {
    let frame = frames.get(index)?;
    if frame.direction != Direction::Rx {
        return None;
    }
    frames[..index]
        .iter()
        .rev()
        .take(PAIR_WINDOW)
        .find(|f| f.direction == Direction::Tx && f.framing == frame.framing)
}

pub fn breakdown(frame: &Frame, request: Option<&Frame>, mode: AddressMode) -> Vec<Field> {
    let (adus, rest) = split(&frame.bytes, frame.framing, frame.direction);
    let requests = request.map_or_else(Vec::new, |q| split(&q.bytes, q.framing, q.direction).0);
    let noun = match frame.direction {
        Direction::Tx => "request",
        Direction::Rx => "response",
    };
    let total = adus.len();
    let mut fields = Vec::new();
    for (i, adu) in adus.into_iter().enumerate() {
        let paired = requests
            .iter()
            .rev()
            .find(|q| q.tid == adu.tid && q.unit == adu.unit)
            .and_then(|q| Request::parse(q.pdu));
        let count = if total > 1 {
            format!(" {}/{total}", i + 1)
        } else {
            String::new()
        };
        fields.push(Field::heading(format!(
            "{} {noun}{count}",
            frame.framing.label()
        )));
        fields.extend(adu.head);
        fields.extend(pdu_fields(adu.pdu, frame.direction, paired, mode));
        fields.extend(adu.tail);
    }
    if !rest.is_empty() {
        fields.push(Field::heading("incomplete"));
        raw(&mut fields, rest, "unparsed", Tone::Bad);
    }
    fields
}

pub const fn function_name(code: u8) -> &'static str {
    match code {
        0x01 => "Read Coils",
        0x02 => "Read Discrete Inputs",
        0x03 => "Read Holding Registers",
        0x04 => "Read Input Registers",
        0x05 => "Write Single Coil",
        0x06 => "Write Single Register",
        0x07 => "Read Exception Status",
        0x08 => "Diagnostics",
        0x0B => "Get Comm Event Counter",
        0x0C => "Get Comm Event Log",
        0x0F => "Write Multiple Coils",
        0x10 => "Write Multiple Registers",
        0x11 => "Report Server ID",
        0x14 => "Read File Record",
        0x15 => "Write File Record",
        0x16 => "Mask Write Register",
        0x17 => "Read/Write Registers",
        0x18 => "Read FIFO Queue",
        0x2B => "Encapsulated Interface",
        _ => "Unknown function",
    }
}

const fn exception_name(code: u8) -> &'static str {
    match code {
        0x01 => "Illegal Function",
        0x02 => "Illegal Data Address",
        0x03 => "Illegal Data Value",
        0x04 => "Server Device Failure",
        0x05 => "Acknowledge",
        0x06 => "Server Device Busy",
        0x08 => "Memory Parity Error",
        0x0A => "Gateway Path Unavailable",
        0x0B => "Gateway Target No Response",
        _ => "Unknown exception",
    }
}

const fn diagnostic_name(code: u16) -> Option<&'static str> {
    Some(match code {
        0x00 => "Return Query Data",
        0x01 => "Restart Communications",
        0x02 => "Diagnostic Register",
        0x03 => "Change ASCII Delimiter",
        0x04 => "Force Listen Only",
        0x0A => "Clear Counters",
        0x0B => "Bus Message Count",
        0x0C => "Bus Comm Error Count",
        0x0D => "Bus Exception Count",
        0x0E => "Server Message Count",
        0x0F => "Server No Response Count",
        0x10 => "Server NAK Count",
        0x11 => "Server Busy Count",
        0x12 => "Char Overrun Count",
        0x14 => "Clear Overrun",
        _ => return None,
    })
}

fn comm_status(status: u16) -> String {
    match status {
        0xFFFF => "status busy".to_owned(),
        0x0000 => "status ready".to_owned(),
        _ => format!("status 0x{status:04X}"),
    }
}

fn comm_event(event: u8) -> String {
    let kind = if event & 0x80 != 0 {
        "receive"
    } else if event & 0x40 != 0 {
        "send"
    } else if event == 0x04 {
        "listen only"
    } else if event == 0x00 {
        "restart"
    } else {
        "event"
    };
    format!("{kind} {event:08b}")
}

fn raw(fields: &mut Vec<Field>, bytes: &[u8], label: &str, tone: Tone) {
    for (i, chunk) in bytes.chunks(CHUNK).enumerate() {
        let label = if i == 0 { label } else { "" };
        fields.push(Field::new(chunk, label).with(tone));
    }
}

fn be(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
}

struct Adu<'a> {
    tid: Option<u16>,
    unit: u8,
    head: Vec<Field>,
    pdu: &'a [u8],
    tail: Vec<Field>,
}

fn split(bytes: &[u8], framing: Framing, direction: Direction) -> (Vec<Adu<'_>>, &[u8]) {
    let mut adus = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let next = match framing {
            Framing::Rtu => rtu_adu(rest, direction),
            Framing::Tcp => tcp_adu(rest),
        };
        let Some((adu, len)) = next else {
            break;
        };
        adus.push(adu);
        rest = &rest[len..];
    }
    (adus, rest)
}

fn rtu_adu(bytes: &[u8], direction: Direction) -> Option<(Adu<'_>, usize)> {
    let pdu_len = match pdu_len(&bytes[1..], direction) {
        Some(len) => len,
        None => bytes.len().checked_sub(3).filter(|&len| len > 0)?,
    };
    let len = 1 + pdu_len + 2;
    let adu = bytes.get(..len)?;
    let (body, crc) = adu.split_at(len - 2);
    let got = u16::from_le_bytes([crc[0], crc[1]]);
    let want = crc16(body);
    let crc = if got == want {
        Field::new(crc, "CRC ok").with(Tone::Good)
    } else {
        Field::new(crc, format!("CRC bad, want {}", hex(&want.to_le_bytes()))).with(Tone::Bad)
    };
    let adu = Adu {
        tid: None,
        unit: body[0],
        head: vec![Field::new(&body[..1], format!("unit {}", body[0]))],
        pdu: &body[1..],
        tail: vec![crc],
    };
    Some((adu, len))
}

fn tcp_adu(bytes: &[u8]) -> Option<(Adu<'_>, usize)> {
    let tid = be(bytes, 0)?;
    let protocol = be(bytes, 2)?;
    let length = be(bytes, 4)?;
    let len = 6 + usize::from(length);
    let adu = bytes.get(..len).filter(|_| length >= 2)?;
    let unit = adu[6];
    let protocol = if protocol == 0 {
        Field::new(&adu[2..4], "protocol Modbus")
    } else {
        Field::new(&adu[2..4], format!("protocol {protocol}")).with(Tone::Bad)
    };
    let adu = Adu {
        tid: Some(tid),
        unit,
        head: vec![
            Field::new(&adu[..2], format!("transaction {tid}")),
            protocol,
            Field::new(&adu[4..6], format!("length {length}")),
            Field::new(&adu[6..7], format!("unit {unit}")),
        ],
        pdu: &adu[7..],
        tail: Vec::new(),
    };
    Some((adu, len))
}

fn pdu_len(pdu: &[u8], direction: Direction) -> Option<usize> {
    let function = *pdu.first()?;
    let counted = |at: usize| Some(at + 1 + pdu.get(at).map_or(0, |&n| usize::from(n)));
    match (direction, function) {
        (Direction::Rx, f) if f & 0x80 != 0 => Some(2),
        (_, 0x08) if be(pdu, 1) == Some(0x00) => None,
        (Direction::Tx, 0x01..=0x06 | 0x08) => Some(5),
        (Direction::Tx, 0x07 | 0x0B | 0x0C | 0x11) => Some(1),
        (Direction::Tx, 0x0F | 0x10) => counted(5),
        (Direction::Tx, 0x14 | 0x15) => counted(1),
        (Direction::Tx, 0x16) => Some(7),
        (Direction::Tx, 0x17) => counted(9),
        (Direction::Tx, 0x18) => Some(3),
        (Direction::Tx, 0x2B) => Some(4),
        (Direction::Rx, 0x01..=0x04 | 0x0C | 0x11 | 0x14 | 0x15 | 0x17) => counted(1),
        (Direction::Rx, 0x05 | 0x06 | 0x08 | 0x0B | 0x0F | 0x10) => Some(5),
        (Direction::Rx, 0x07) => Some(2),
        (Direction::Rx, 0x16) => Some(7),
        (Direction::Rx, 0x18) => Some(3 + be(pdu, 1).map_or(0, usize::from)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct Request {
    function: u8,
    address: u16,
    quantity: u16,
}

impl Request {
    fn parse(pdu: &[u8]) -> Option<Self> {
        let function = *pdu.first()?;
        if !matches!(function, 0x01..=0x04 | 0x17) {
            return None;
        }
        Some(Self {
            function,
            address: be(pdu, 1)?,
            quantity: be(pdu, 3)?,
        })
    }
}

struct Reader<'a> {
    rest: &'a [u8],
    fields: Vec<Field>,
    mode: AddressMode,
}

impl<'a> Reader<'a> {
    const fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.rest.len() < n {
            return None;
        }
        let (head, tail) = self.rest.split_at(n);
        self.rest = tail;
        Some(head)
    }

    fn byte(&mut self, label: impl FnOnce(u8) -> String) -> Option<u8> {
        let bytes = self.take(1)?;
        self.fields.push(Field::new(bytes, label(bytes[0])));
        Some(bytes[0])
    }

    fn word(&mut self, label: impl FnOnce(u16) -> String) -> Option<u16> {
        let bytes = self.take(2)?;
        let value = u16::from_be_bytes([bytes[0], bytes[1]]);
        self.fields.push(Field::new(bytes, label(value)));
        Some(value)
    }

    fn address(&mut self, name: &str) -> Option<u16> {
        let mode = self.mode;
        self.word(|a| format!("{name} {}", mode.display(a)))
    }

    fn quantity(&mut self, name: &str) -> Option<u16> {
        self.word(|q| format!("{name} {q}"))
    }

    fn byte_count(&mut self) -> Option<u8> {
        self.byte(|n| format!("{n} bytes follow"))
    }

    fn mark(&mut self, tone: Tone) {
        if let Some(last) = self.fields.last_mut() {
            last.tone = tone;
        }
    }

    fn data(&mut self, label: &str) {
        let rest = std::mem::take(&mut self.rest);
        raw(&mut self.fields, rest, label, Tone::Plain);
    }

    fn words(&mut self, start: Option<u16>) {
        let mode = self.mode;
        let mut i = 0u16;
        while self.rest.len() >= 2 {
            self.word(|v| match start {
                Some(start) => format!("{} = {v}", mode.display(start.wrapping_add(i))),
                None => format!("[{i}] = {v}"),
            });
            i += 1;
        }
    }

    fn coils(&mut self, start: Option<u16>, quantity: Option<u16>) {
        let mode = self.mode;
        let mut offset = 0u16;
        while !self.rest.is_empty() {
            let left = quantity.map_or(8, |q| q.saturating_sub(offset).min(8));
            self.byte(|b| {
                let bits: String = (0..left.max(1))
                    .map(|bit| if b >> bit & 1 == 1 { '1' } else { '0' })
                    .collect();
                match start {
                    Some(start) => {
                        format!("from {}: {bits}", mode.display(start.wrapping_add(offset)))
                    }
                    None => format!("bit {offset}: {bits}"),
                }
            });
            offset = offset.saturating_add(8);
        }
    }

    fn echo(&mut self, function: u8) -> Option<()> {
        match function {
            0x05 => {
                self.address("address")?;
                self.word(|v| {
                    match v {
                        0xFF00 => "on",
                        0x0000 => "off",
                        _ => "invalid coil value",
                    }
                    .to_owned()
                })?;
            }
            0x06 => {
                self.address("address")?;
                self.word(|v| format!("value {v}"))?;
            }
            0x16 => {
                self.address("address")?;
                self.word(|m| format!("and {m:016b}"))?;
                self.word(|m| format!("or  {m:016b}"))?;
            }
            _ => self.data("data"),
        }
        Some(())
    }

    fn request(&mut self) -> Option<()> {
        let function = self.byte(|f| function_name(f).to_owned())?;
        match function {
            0x01..=0x04 => {
                self.address("start")?;
                self.quantity("quantity")?;
            }
            0x0F => {
                let start = self.address("start")?;
                let quantity = self.quantity("quantity")?;
                self.byte_count()?;
                self.coils(Some(start), Some(quantity));
            }
            0x10 => {
                let start = self.address("start")?;
                self.quantity("quantity")?;
                self.byte_count()?;
                self.words(Some(start));
            }
            0x17 => {
                self.address("read start")?;
                self.quantity("read quantity")?;
                let start = self.address("write start")?;
                self.quantity("write quantity")?;
                self.byte_count()?;
                self.words(Some(start));
            }
            0x2B => {
                self.byte(|m| format!("MEI type 0x{m:02X}"))?;
                self.byte(|c| format!("read code {c}"))?;
                self.byte(|o| format!("object 0x{o:02X}"))?;
            }
            0x08 => self.diagnostic(false)?,
            0x14 => {
                self.byte_count()?;
                while self.rest.len() >= 7 {
                    self.reference()?;
                    self.quantity("file")?;
                    self.quantity("record")?;
                    self.quantity("length")?;
                }
            }
            0x18 => {
                self.address("FIFO pointer")?;
            }
            0x07 | 0x0B | 0x0C | 0x11 => {}
            _ => self.echo(function)?,
        }
        Some(())
    }

    fn response(&mut self, request: Option<Request>) -> Option<()> {
        let function = self.byte(|f| {
            let name = function_name(f & 0x7F);
            if f & 0x80 == 0 {
                name.to_owned()
            } else {
                format!("{name} failed")
            }
        })?;
        if function & 0x80 != 0 {
            self.mark(Tone::Bad);
            self.byte(|c| exception_name(c).to_owned())?;
            self.mark(Tone::Bad);
            return Some(());
        }
        let request = request.filter(|q| q.function == function);
        match function {
            0x01 | 0x02 => {
                self.byte_count()?;
                self.coils(request.map(|q| q.address), request.map(|q| q.quantity));
            }
            0x03 | 0x04 | 0x17 => {
                self.byte_count()?;
                self.words(request.map(|q| q.address));
            }
            0x0F | 0x10 => {
                self.address("start")?;
                self.quantity("quantity")?;
            }
            0x07 => {
                self.byte(|s| format!("status {s:08b}"))?;
            }
            0x11 => {
                self.byte_count()?;
                self.data("server id");
            }
            0x08 => self.diagnostic(true)?,
            0x0B => {
                self.word(comm_status)?;
                self.quantity("event count")?;
            }
            0x0C => {
                self.byte_count()?;
                self.word(comm_status)?;
                self.quantity("event count")?;
                self.quantity("message count")?;
                while !self.rest.is_empty() {
                    self.byte(comm_event)?;
                }
            }
            0x14 => {
                self.byte_count()?;
                while !self.rest.is_empty() {
                    self.file_response()?;
                }
            }
            0x18 => {
                self.word(|n| format!("{n} bytes follow"))?;
                self.quantity("FIFO count")?;
                self.words(None);
            }
            0x2B => self.device_id()?,
            _ => self.echo(function)?,
        }
        Some(())
    }

    fn diagnostic(&mut self, response: bool) -> Option<()> {
        let sub = self.word(|s| {
            diagnostic_name(s).map_or_else(|| format!("sub-function 0x{s:04X}"), str::to_owned)
        })?;
        match (response, sub, self.rest.len()) {
            (true, 0x02, 2) => {
                self.word(|r| format!("register {r:016b}"))?;
            }
            (true, 0x0B..=0x12, 2) => {
                self.quantity("count")?;
            }
            _ => self.data("data"),
        }
        Some(())
    }

    fn reference(&mut self) -> Option<()> {
        let kind = self.byte(|t| format!("reference type {t}"))?;
        if kind != 6 {
            self.mark(Tone::Bad);
        }
        Some(())
    }

    fn file_response(&mut self) -> Option<()> {
        let len = self.byte(|n| format!("{n} bytes in record"))?;
        let body = self.take(usize::from(len))?;
        let rest = std::mem::replace(&mut self.rest, body);
        let done = self.reference();
        self.words(None);
        let odd = std::mem::replace(&mut self.rest, rest);
        raw(&mut self.fields, odd, "unexpected", Tone::Bad);
        done
    }

    fn device_id(&mut self) -> Option<()> {
        self.byte(|m| format!("MEI type 0x{m:02X}"))?;
        self.byte(|c| format!("read code {c}"))?;
        self.byte(|c| format!("conformity 0x{c:02X}"))?;
        self.byte(|m| {
            if m == 0xFF {
                "more follows"
            } else {
                "last page"
            }
            .to_owned()
        })?;
        self.byte(|o| format!("next object 0x{o:02X}"))?;
        let count = self.byte(|n| format!("{n} objects"))?;
        for _ in 0..count {
            self.byte(|o| format!("object 0x{o:02X}"))?;
            let len = self.byte(|n| format!("length {n}"))?;
            let value = self.take(usize::from(len))?;
            let text = format!("\"{}\"", String::from_utf8_lossy(value));
            raw(&mut self.fields, value, &text, Tone::Plain);
        }
        Some(())
    }

    fn finish(mut self) -> Vec<Field> {
        let rest = std::mem::take(&mut self.rest);
        raw(&mut self.fields, rest, "unexpected", Tone::Bad);
        self.fields
    }
}

fn pdu_fields(
    pdu: &[u8],
    direction: Direction,
    request: Option<Request>,
    mode: AddressMode,
) -> Vec<Field> {
    let mut reader = Reader {
        rest: pdu,
        fields: Vec::new(),
        mode,
    };
    let _ = match direction {
        Direction::Tx => reader.request(),
        Direction::Rx => reader.response(request),
    };
    reader.finish()
}

#[cfg(test)]
mod tests {
    use super::{Field, Tone, breakdown, request_for};
    use crate::config::AddressMode;
    use crate::frames::{Direction, Frame, Framing, request_pdu, response_pdu, rtu};
    use chrono::{Local, TimeZone};
    use tokio_modbus::{ExceptionCode, Request, Response};

    fn frame(direction: Direction, framing: Framing, bytes: Vec<u8>) -> Frame {
        let time = Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap();
        Frame {
            time,
            last: time,
            direction,
            framing,
            bytes,
        }
    }

    fn rows(fields: &[Field]) -> Vec<(String, String)> {
        fields.iter().map(|f| (f.hex(), f.label.clone())).collect()
    }

    fn row(hex: &str, label: &str) -> (String, String) {
        (hex.to_owned(), label.to_owned())
    }

    #[test]
    fn rtu_read_request_names_every_byte() {
        let tx = frame(
            Direction::Tx,
            Framing::Rtu,
            rtu(1, &request_pdu(&Request::ReadHoldingRegisters(100, 2))),
        );
        let fields = breakdown(&tx, None, AddressMode::Dec);
        assert_eq!(
            rows(&fields),
            vec![
                row("", "RTU request"),
                row("01", "unit 1"),
                row("03", "Read Holding Registers"),
                row("00 64", "start 100"),
                row("00 02", "quantity 2"),
                row("85 D4", "CRC ok"),
            ]
        );
        assert_eq!(fields.last().unwrap().tone, Tone::Good);
    }

    #[test]
    fn responses_take_register_addresses_from_their_request() {
        let tx = frame(
            Direction::Tx,
            Framing::Rtu,
            rtu(1, &request_pdu(&Request::ReadHoldingRegisters(100, 2))),
        );
        let rx = frame(
            Direction::Rx,
            Framing::Rtu,
            rtu(
                1,
                &response_pdu(0x03, &Ok(Response::ReadHoldingRegisters(vec![7, 0x0102]))),
            ),
        );
        let frames = [tx, rx];
        let request = request_for(&frames, 1);
        assert_eq!(request, Some(&frames[0]));
        let labels: Vec<String> = breakdown(&frames[1], request, AddressMode::Dec)
            .into_iter()
            .map(|f| f.label)
            .collect();
        assert_eq!(&labels[3..6], ["4 bytes follow", "100 = 7", "101 = 258"]);

        let unpaired: Vec<String> = breakdown(&frames[1], None, AddressMode::Dec)
            .into_iter()
            .map(|f| f.label)
            .collect();
        assert_eq!(&unpaired[4..6], ["[0] = 7", "[1] = 258"]);
    }

    #[test]
    fn coil_bits_stop_at_the_requested_quantity() {
        let tx = frame(
            Direction::Tx,
            Framing::Rtu,
            rtu(1, &request_pdu(&Request::ReadCoils(8, 10))),
        );
        let coils = vec![
            true, false, true, true, false, false, false, false, true, true,
        ];
        let rx = frame(
            Direction::Rx,
            Framing::Rtu,
            rtu(1, &response_pdu(0x01, &Ok(Response::ReadCoils(coils)))),
        );
        let labels: Vec<String> = breakdown(&rx, Some(&tx), AddressMode::Dec)
            .into_iter()
            .map(|f| f.label)
            .collect();
        assert_eq!(&labels[4..6], ["from 8: 10110000", "from 16: 11"]);
    }

    #[test]
    fn exceptions_and_bad_crcs_are_flagged() {
        let mut bytes = rtu(
            1,
            &response_pdu(0x03, &Err(ExceptionCode::IllegalDataAddress)),
        );
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        let rx = frame(Direction::Rx, Framing::Rtu, bytes);
        let fields = breakdown(&rx, None, AddressMode::Dec);
        let tones: Vec<(&str, Tone)> = fields.iter().map(|f| (f.label.as_str(), f.tone)).collect();
        assert_eq!(tones[2], ("Read Holding Registers failed", Tone::Bad));
        assert_eq!(tones[3], ("Illegal Data Address", Tone::Bad));
        assert!(tones[4].0.starts_with("CRC bad"));
        assert_eq!(tones[4].1, Tone::Bad);
    }

    #[test]
    fn merged_frames_split_and_leftovers_are_unparsed() {
        let one = rtu(1, &request_pdu(&Request::ReadInputRegisters(0, 1)));
        let two = rtu(2, &request_pdu(&Request::WriteSingleCoil(3, true)));
        let bytes = [one.as_slice(), two.as_slice(), &[0x01, 0x03]].concat();
        let tx = frame(Direction::Tx, Framing::Rtu, bytes);
        let fields = breakdown(&tx, None, AddressMode::Dec);
        let headings: Vec<&str> = fields
            .iter()
            .filter(|f| f.tone == Tone::Heading)
            .map(|f| f.label.as_str())
            .collect();
        assert_eq!(
            headings,
            ["RTU request 1/2", "RTU request 2/2", "incomplete"]
        );
        assert!(fields.iter().any(|f| f.label == "on"));
        let last = fields.last().unwrap();
        assert_eq!((last.hex(), last.tone), ("01 03".to_owned(), Tone::Bad));
    }

    #[test]
    fn tcp_frames_decode_the_mbap_header() {
        let mut bytes = vec![0x00, 0x07, 0x00, 0x00, 0x00, 0x06, 0x01];
        bytes.extend(request_pdu(&Request::ReadHoldingRegisters(0x10, 1)));
        let tx = frame(Direction::Tx, Framing::Tcp, bytes);
        let fields = breakdown(&tx, None, AddressMode::Hex);
        assert_eq!(
            rows(&fields)[..6],
            [
                row("", "TCP request"),
                row("00 07", "transaction 7"),
                row("00 00", "protocol Modbus"),
                row("00 06", "length 6"),
                row("01", "unit 1"),
                row("03", "Read Holding Registers"),
            ]
        );
        assert_eq!(
            fields[6].label,
            format!("start {}", AddressMode::Hex.display(0x10))
        );
    }

    fn labels(direction: Direction, pdu: &[u8]) -> Vec<(String, String)> {
        let frame = frame(direction, Framing::Rtu, rtu(1, pdu));
        let fields = breakdown(&frame, None, AddressMode::Dec);
        let rows = rows(&fields);
        rows[2..rows.len() - 1].to_vec()
    }

    #[test]
    fn diagnostics_name_the_sub_function_and_counters() {
        assert_eq!(
            labels(Direction::Tx, &[0x08, 0x00, 0x0E, 0x00, 0x00]),
            vec![
                row("08", "Diagnostics"),
                row("00 0E", "Server Message Count"),
                row("00 00", "data"),
            ]
        );
        assert_eq!(
            labels(Direction::Rx, &[0x08, 0x00, 0x0E, 0x01, 0x02]),
            vec![
                row("08", "Diagnostics"),
                row("00 0E", "Server Message Count"),
                row("01 02", "count 258"),
            ]
        );
        assert_eq!(
            labels(Direction::Rx, &[0x08, 0x00, 0x02, 0x80, 0x01])[2],
            row("80 01", "register 1000000000000001")
        );
        assert_eq!(
            labels(Direction::Tx, &[0x08, 0x00, 0x99, 0x00, 0x00])[1],
            row("00 99", "sub-function 0x0099")
        );
    }

    #[test]
    fn return_query_data_keeps_its_whole_payload() {
        let pdu = [0x08, 0x00, 0x00, 1, 2, 3, 4, 5, 6];
        let tx = frame(Direction::Tx, Framing::Rtu, rtu(1, &pdu));
        let fields = breakdown(&tx, None, AddressMode::Dec);
        assert_eq!(
            rows(&fields)[3..6],
            [
                row("00 00", "Return Query Data"),
                row("01 02 03 04", "data"),
                row("05 06", ""),
            ]
        );
        assert_eq!(fields.last().unwrap().label, "CRC ok");
    }

    #[test]
    fn comm_event_counter_and_log_responses() {
        assert_eq!(
            labels(Direction::Tx, &[0x0B]),
            vec![row("0B", "Get Comm Event Counter")]
        );
        assert_eq!(
            labels(Direction::Rx, &[0x0B, 0xFF, 0xFF, 0x01, 0x08]),
            vec![
                row("0B", "Get Comm Event Counter"),
                row("FF FF", "status busy"),
                row("01 08", "event count 264"),
            ]
        );
        assert_eq!(
            labels(
                Direction::Rx,
                &[0x0C, 0x08, 0x00, 0x00, 0x01, 0x08, 0x01, 0x21, 0x20, 0x00]
            ),
            vec![
                row("0C", "Get Comm Event Log"),
                row("08", "8 bytes follow"),
                row("00 00", "status ready"),
                row("01 08", "event count 264"),
                row("01 21", "message count 289"),
                row("20", "event 00100000"),
                row("00", "restart 00000000"),
            ]
        );
    }

    #[test]
    fn read_file_record_requests_and_responses() {
        assert_eq!(
            labels(
                Direction::Tx,
                &[0x14, 0x07, 0x06, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02]
            ),
            vec![
                row("14", "Read File Record"),
                row("07", "7 bytes follow"),
                row("06", "reference type 6"),
                row("00 04", "file 4"),
                row("00 01", "record 1"),
                row("00 02", "length 2"),
            ]
        );
        assert_eq!(
            labels(
                Direction::Rx,
                &[0x14, 0x06, 0x05, 0x06, 0x0D, 0xFE, 0x00, 0x20]
            ),
            vec![
                row("14", "Read File Record"),
                row("06", "6 bytes follow"),
                row("05", "5 bytes in record"),
                row("06", "reference type 6"),
                row("0D FE", "[0] = 3582"),
                row("00 20", "[1] = 32"),
            ]
        );
        let tx = frame(
            Direction::Tx,
            Framing::Rtu,
            rtu(1, &[0x14, 0x07, 0x05, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02]),
        );
        assert_eq!(breakdown(&tx, None, AddressMode::Dec)[4].tone, Tone::Bad);
    }

    #[test]
    fn read_fifo_queue_requests_and_responses() {
        assert_eq!(
            labels(Direction::Tx, &[0x18, 0x04, 0xDE]),
            vec![
                row("18", "Read FIFO Queue"),
                row("04 DE", "FIFO pointer 1246")
            ]
        );
        assert_eq!(
            labels(
                Direction::Rx,
                &[0x18, 0x00, 0x06, 0x00, 0x02, 0x01, 0xB8, 0x12, 0x84]
            ),
            vec![
                row("18", "Read FIFO Queue"),
                row("00 06", "6 bytes follow"),
                row("00 02", "FIFO count 2"),
                row("01 B8", "[0] = 440"),
                row("12 84", "[1] = 4740"),
            ]
        );
    }

    #[test]
    fn truncated_new_function_frames_degrade_without_panicking() {
        let pdus: [&[u8]; 6] = [
            &[0x08, 0x00, 0x0B, 0x00, 0x07],
            &[0x0B, 0x00, 0x00, 0x00, 0x03],
            &[0x0C, 0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x02, 0x40],
            &[0x14, 0x06, 0x05, 0x06, 0x0D, 0xFE, 0x00, 0x20],
            &[0x14, 0x07, 0x06, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02],
            &[0x18, 0x00, 0x04, 0x00, 0x01, 0x00, 0x09],
        ];
        for pdu in pdus {
            for len in 1..=pdu.len() {
                for direction in [Direction::Tx, Direction::Rx] {
                    for framing in [Framing::Rtu, Framing::Tcp] {
                        let bytes = match framing {
                            Framing::Rtu => rtu(1, &pdu[..len]),
                            Framing::Tcp => {
                                let length = u16::try_from(len + 1).unwrap().to_be_bytes();
                                [&[0, 1, 0, 0, length[0], length[1], 1], &pdu[..len]].concat()
                            }
                        };
                        let fields =
                            breakdown(&frame(direction, framing, bytes), None, AddressMode::Dec);
                        assert!(!fields.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn short_file_record_sub_responses_are_flagged() {
        let fields = labels(Direction::Rx, &[0x14, 0x04, 0x05, 0x06, 0x0D, 0xFE]);
        assert_eq!(fields[2], row("05", "5 bytes in record"));
        assert_eq!(fields[3], row("06 0D FE", "unexpected"));
    }
}
