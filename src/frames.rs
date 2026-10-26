use chrono::{DateTime, Local, TimeDelta};
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio_modbus::{ExceptionCode, Request, Response};

pub mod decode;

const CAP: usize = 1000;
const MERGE_GAP: TimeDelta = TimeDelta::milliseconds(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Tx,
    Rx,
}

impl Direction {
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Tx => "TX",
            Self::Rx => "RX",
        }
    }

    pub const fn arrow(self) -> char {
        match self {
            Self::Tx => '>',
            Self::Rx => '<',
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framing {
    Rtu,
    Tcp,
}

impl Framing {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Rtu => "RTU",
            Self::Tcp => "TCP",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub time: DateTime<Local>,
    pub last: DateTime<Local>,
    pub direction: Direction,
    pub framing: Framing,
    pub bytes: Vec<u8>,
}

impl Frame {
    pub fn hex(&self) -> String {
        hex(&self.bytes)
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

static FRAMES: Mutex<VecDeque<Frame>> = Mutex::new(VecDeque::new());

fn push(
    frames: &mut VecDeque<Frame>,
    direction: Direction,
    framing: Framing,
    bytes: &[u8],
    now: DateTime<Local>,
) {
    if bytes.is_empty() {
        return;
    }
    match frames.back_mut() {
        Some(last)
            if last.direction == direction
                && last.framing == framing
                && now - last.last <= MERGE_GAP =>
        {
            last.bytes.extend_from_slice(bytes);
            last.last = now;
        }
        _ => {
            frames.push_back(Frame {
                time: now,
                last: now,
                direction,
                framing,
                bytes: bytes.to_vec(),
            });
            frames.retain_back(CAP);
        }
    }
}

pub fn record(direction: Direction, framing: Framing, bytes: &[u8]) {
    if let Ok(mut frames) = FRAMES.lock() {
        push(&mut frames, direction, framing, bytes, Local::now());
    }
}

pub fn count() -> usize {
    FRAMES.lock().map(|f| f.len()).unwrap_or(0)
}

pub fn snapshot() -> Vec<Frame> {
    FRAMES
        .lock()
        .map(|f| f.iter().cloned().collect())
        .unwrap_or_default()
}

pub fn export(frames: &[Frame]) -> String {
    frames
        .iter()
        .map(|f| {
            format!(
                "{} {} {}\n",
                f.time.format("%Y-%m-%dT%H:%M:%S%.3f"),
                f.direction.tag(),
                f.hex()
            )
        })
        .collect()
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn rtu(unit: u8, pdu: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(pdu.len() + 3);
    frame.push(unit);
    frame.extend_from_slice(pdu);
    frame.extend(crc16(&frame).to_le_bytes());
    frame
}

fn put_words(out: &mut Vec<u8>, words: &[u16]) {
    out.push((words.len() * 2) as u8);
    out.extend(words.iter().flat_map(|w| w.to_be_bytes()));
}

fn put_coils(out: &mut Vec<u8>, coils: &[bool]) {
    let mut packed = vec![0u8; coils.len().div_ceil(8)];
    for (i, &coil) in coils.iter().enumerate() {
        packed[i / 8] |= u8::from(coil) << (i % 8);
    }
    out.push(packed.len() as u8);
    out.extend(packed);
}

const fn coil_word(state: bool) -> u16 {
    if state { 0xFF00 } else { 0x0000 }
}

pub fn request_pdu(request: &Request<'_>) -> Vec<u8> {
    let mut out = vec![request.function_code().value()];
    match request {
        Request::ReadCoils(address, quantity)
        | Request::ReadDiscreteInputs(address, quantity)
        | Request::ReadInputRegisters(address, quantity)
        | Request::ReadHoldingRegisters(address, quantity) => {
            out.extend(address.to_be_bytes());
            out.extend(quantity.to_be_bytes());
        }
        Request::WriteSingleCoil(address, state) => {
            out.extend(address.to_be_bytes());
            out.extend(coil_word(*state).to_be_bytes());
        }
        Request::WriteMultipleCoils(address, coils) => {
            out.extend(address.to_be_bytes());
            out.extend((coils.len() as u16).to_be_bytes());
            put_coils(&mut out, coils);
        }
        Request::WriteSingleRegister(address, word) => {
            out.extend(address.to_be_bytes());
            out.extend(word.to_be_bytes());
        }
        Request::WriteMultipleRegisters(address, words) => {
            out.extend(address.to_be_bytes());
            out.extend((words.len() as u16).to_be_bytes());
            put_words(&mut out, words);
        }
        Request::ReportServerId => {}
        Request::MaskWriteRegister(address, and_mask, or_mask) => {
            out.extend(address.to_be_bytes());
            out.extend(and_mask.to_be_bytes());
            out.extend(or_mask.to_be_bytes());
        }
        Request::ReadWriteMultipleRegisters(read_address, quantity, write_address, words) => {
            out.extend(read_address.to_be_bytes());
            out.extend(quantity.to_be_bytes());
            out.extend(write_address.to_be_bytes());
            out.extend((words.len() as u16).to_be_bytes());
            put_words(&mut out, words);
        }
        Request::ReadDeviceIdentification(read_code, object_id) => {
            out.extend([0x0E, read_code.value(), *object_id]);
        }
        Request::Custom(_, data) => out.extend_from_slice(data),
    }
    out
}

pub fn response_pdu(function: u8, response: &Result<Response, ExceptionCode>) -> Vec<u8> {
    let response = match response {
        Ok(response) => response,
        Err(code) => return vec![function | 0x80, u8::from(*code)],
    };
    let mut out = vec![response.function_code().value()];
    match response {
        Response::ReadCoils(coils) | Response::ReadDiscreteInputs(coils) => {
            put_coils(&mut out, coils);
        }
        Response::ReadInputRegisters(words)
        | Response::ReadHoldingRegisters(words)
        | Response::ReadWriteMultipleRegisters(words) => put_words(&mut out, words),
        Response::WriteSingleCoil(address, state) => {
            out.extend(address.to_be_bytes());
            out.extend(coil_word(*state).to_be_bytes());
        }
        Response::WriteMultipleCoils(address, quantity)
        | Response::WriteMultipleRegisters(address, quantity) => {
            out.extend(address.to_be_bytes());
            out.extend(quantity.to_be_bytes());
        }
        Response::ReportServerId(server_id, running, data) => {
            out.extend([
                2 + data.len() as u8,
                *server_id,
                if *running { 0xFF } else { 0x00 },
            ]);
            out.extend_from_slice(data);
        }
        Response::WriteSingleRegister(address, word) => {
            out.extend(address.to_be_bytes());
            out.extend(word.to_be_bytes());
        }
        Response::MaskWriteRegister(address, and_mask, or_mask) => {
            out.extend(address.to_be_bytes());
            out.extend(and_mask.to_be_bytes());
            out.extend(or_mask.to_be_bytes());
        }
        Response::ReadDeviceIdentification(id) => {
            out.extend([
                0x0E,
                id.read_code.value(),
                id.conformity_level.value(),
                if id.more_follows { 0xFF } else { 0x00 },
                id.next_object_id,
                id.device_id_objects.len() as u8,
            ]);
            for object in &id.device_id_objects {
                out.extend([object.id, object.value.len() as u8]);
                out.extend_from_slice(&object.value);
            }
        }
        Response::Custom(_, data) => out.extend_from_slice(data),
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
pub use tap::Tap;

#[cfg(not(target_arch = "wasm32"))]
mod tap {
    use super::{Direction, Framing, record};
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    #[derive(Debug)]
    pub struct Tap<T>(pub T, pub Framing);

    impl<T: AsyncRead + Unpin> AsyncRead for Tap<T> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            let before = buf.filled().len();
            let poll = Pin::new(&mut self.0).poll_read(cx, buf);
            if matches!(poll, Poll::Ready(Ok(()))) {
                record(Direction::Rx, self.1, &buf.filled()[before..]);
            }
            poll
        }
    }

    impl<T: AsyncWrite + Unpin> AsyncWrite for Tap<T> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            let poll = Pin::new(&mut self.0).poll_write(cx, buf);
            if let Poll::Ready(Ok(written)) = poll {
                record(Direction::Tx, self.1, &buf[..written]);
            }
            poll
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, Frame, Framing, crc16, export, push, request_pdu, response_pdu, rtu};
    use chrono::{Local, TimeDelta, TimeZone};
    use std::collections::VecDeque;
    use tokio_modbus::{ExceptionCode, Request, Response};

    #[test]
    fn chunks_in_the_same_direction_merge_while_they_keep_coming() {
        let start = Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap();
        let mut frames = VecDeque::new();
        push(
            &mut frames,
            Direction::Tx,
            Framing::Rtu,
            &[0x01, 0x03],
            start,
        );
        push(
            &mut frames,
            Direction::Tx,
            Framing::Rtu,
            &[0x00],
            start + TimeDelta::milliseconds(10),
        );
        push(
            &mut frames,
            Direction::Rx,
            Framing::Rtu,
            &[0x01],
            start + TimeDelta::milliseconds(20),
        );
        push(
            &mut frames,
            Direction::Rx,
            Framing::Rtu,
            &[],
            start + TimeDelta::milliseconds(25),
        );
        push(
            &mut frames,
            Direction::Rx,
            Framing::Rtu,
            &[0x83],
            start + TimeDelta::milliseconds(30),
        );
        push(
            &mut frames,
            Direction::Rx,
            Framing::Rtu,
            &[0x02],
            start + TimeDelta::seconds(2),
        );

        let shape: Vec<(Direction, Vec<u8>)> =
            frames.into_iter().map(|f| (f.direction, f.bytes)).collect();
        assert_eq!(
            shape,
            vec![
                (Direction::Tx, vec![0x01, 0x03, 0x00]),
                (Direction::Rx, vec![0x01, 0x83]),
                (Direction::Rx, vec![0x02]),
            ]
        );
    }

    #[test]
    fn exported_lines_carry_the_direction_and_spaced_hex() {
        let time = Local.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap();
        let frame = Frame {
            time,
            last: time,
            direction: Direction::Rx,
            framing: Framing::Rtu,
            bytes: vec![0x01, 0xAB, 0x0F],
        };
        assert_eq!(export(&[frame]), "2026-10-26T12:00:00.000 RX 01 AB 0F\n");
    }

    #[test]
    fn rtu_frames_end_with_the_little_endian_crc() {
        let pdu = request_pdu(&Request::ReadHoldingRegisters(0, 10));
        assert_eq!(
            rtu(1, &pdu),
            vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD]
        );
        assert_eq!(crc16(&[]), 0xFFFF);
    }

    #[test]
    fn write_requests_carry_counts_and_packed_coils() {
        assert_eq!(
            request_pdu(&Request::WriteMultipleRegisters(
                5,
                vec![0x1234, 0xABCD].into()
            )),
            vec![0x10, 0x00, 0x05, 0x00, 0x02, 0x04, 0x12, 0x34, 0xAB, 0xCD]
        );
        assert_eq!(
            request_pdu(&Request::WriteMultipleCoils(
                0,
                vec![true, false, true, true, false, false, false, false, true].into()
            )),
            vec![0x0F, 0x00, 0x00, 0x00, 0x09, 0x02, 0x0D, 0x01]
        );
        assert_eq!(
            request_pdu(&Request::WriteSingleCoil(3, true)),
            vec![0x05, 0x00, 0x03, 0xFF, 0x00]
        );
    }

    #[test]
    fn responses_encode_values_and_exceptions() {
        assert_eq!(
            response_pdu(0x03, &Ok(Response::ReadHoldingRegisters(vec![1, 0x0203]))),
            vec![0x03, 0x04, 0x00, 0x01, 0x02, 0x03]
        );
        assert_eq!(
            response_pdu(0x01, &Ok(Response::ReadCoils(vec![true, true, false]))),
            vec![0x01, 0x01, 0x03]
        );
        assert_eq!(
            response_pdu(0x04, &Err(ExceptionCode::IllegalDataAddress)),
            vec![0x84, 0x02]
        );
    }
}
