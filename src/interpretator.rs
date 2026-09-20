use crate::config::{Column, InterpretorConfig};
use crate::constants::{NO_VALUE, UNINTERPRETABLE};
use crate::custom::CustomRepr;
use crate::modbus::WordOrder;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct Interpretor {
    config: InterpretorConfig,
    word_order: WordOrder,
    header: String,
}

const ADDRESS_W: usize = 7;
const TIME_W: usize = 12;
const AGO_W: usize = 9;
const INSPECT_W: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowSegment {
    pub name: &'static str,
    pub start: usize,
    pub width: usize,
}

impl RowSegment {
    const fn new(name: &'static str, start: usize, width: usize) -> Self {
        Self { name, start, width }
    }

    pub fn text(self, row: &str) -> String {
        row.chars()
            .skip(self.start)
            .take(self.width)
            .collect::<String>()
            .trim()
            .to_string()
    }

    pub fn end(self) -> usize {
        self.start.saturating_add(self.width)
    }
}

struct ColumnSpec {
    column: Column,
    width: usize,
    render: fn(&RowCtx, usize, &mut String),
}

struct RowCtx<'a> {
    value: u16,
    next: [Option<u16>; 3],
    word: u32,
    dword: u64,
    custom: &'a str,
}

impl<'a> RowCtx<'a> {
    fn new(order: WordOrder, value: u16, next: [Option<u16>; 3], custom: Option<&'a str>) -> Self {
        let [b, c, d] = next.map(Option::unwrap_or_default);
        Self {
            value,
            next,
            word: order.make_word(value, b),
            dword: order.make_dword([value, b, c, d]),
            custom: custom.unwrap_or(NO_VALUE),
        }
    }

    fn two(&self) -> bool {
        self.next[0].is_some()
    }

    fn four(&self) -> bool {
        self.next.iter().all(Option::is_some)
    }
}

#[rustfmt::skip]
const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec { column: Column::U16,     width: 5,  render: |c, _, o| { let _ = write!(o, "{}", c.value); } },
    ColumnSpec { column: Column::I16,     width: 6,  render: |c, _, o| { let _ = write!(o, "{}", c.value as i16); } },
    ColumnSpec { column: Column::U8s,     width: 8,  render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8, (c.value & 0xFF) as u8); } },
    ColumnSpec { column: Column::I8s,     width: 9,  render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8 as i8, (c.value & 0xFF) as u8 as i8); } },
    ColumnSpec { column: Column::Hex,     width: 4,  render: |c, _, o| { let _ = write!(o, "{:04X}", c.value); } },
    ColumnSpec { column: Column::Hex32,   width: 9,  render: |c, _, o| if c.two() { let _ = write!(o, "{:08X}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F16,     width: 10, render: |c, w, o| float_cell(f16_to_f32(c.value), w, o) },
    ColumnSpec { column: Column::Bcd,     width: 6,  render: |c, _, o| match bcd_to_decimal(c.value) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } },
    ColumnSpec { column: Column::Bcd32,   width: 10, render: |c, _, o| if c.two() { match bcd_to_decimal(c.word) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U32,     width: 10, render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I32,     width: 11, render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word as i32); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U32M10K, width: 11, render: |c, _, o| if c.two() { let (h, l) = m10k_to_u32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I32M10K, width: 14, render: |c, _, o| if c.two() { let (h, l) = m10k_to_i32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U64,     width: 20, render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I64,     width: 21, render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword as i64); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F32,     width: 10, render: |c, w, o| if c.two() { float_cell(f32::from_bits(c.word), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F64,     width: 12, render: |c, w, o| if c.four() { float_cell(f64::from_bits(c.dword), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::Ascii,   width: 5,  render: |c, _, o| ascii_cell(c.value, c.next[0].unwrap_or_default(), o) },
    ColumnSpec { column: Column::Bits,    width: 19, render: |c, _, o| bits_cell(c.value, o) },
    ColumnSpec { column: Column::Custom,  width: 18, render: |c, _, o| o.push_str(c.custom) },
];

impl Column {
    pub fn graph_width(self) -> Option<usize> {
        self.custom_repr()
            .map(CustomRepr::register_count)
            .or(match self {
                Column::Bcd => Some(1),
                Column::Bcd32 => Some(2),
                _ => None,
            })
    }

    pub fn is_graphable(self) -> bool {
        self.graph_width().is_some()
    }

    pub fn graph_is_float(self) -> bool {
        matches!(self, Column::F16 | Column::F32 | Column::F64)
    }

    pub fn custom_repr(self) -> Option<CustomRepr> {
        Some(match self {
            Column::U16 => CustomRepr::U16,
            Column::I16 => CustomRepr::I16,
            Column::F16 => CustomRepr::F16,
            Column::U32 => CustomRepr::U32,
            Column::I32 => CustomRepr::I32,
            Column::F32 => CustomRepr::F32,
            Column::U64 => CustomRepr::U64,
            Column::I64 => CustomRepr::I64,
            Column::F64 => CustomRepr::F64,
            _ => return None,
        })
    }
}

pub fn graph_value(column: Column, order: WordOrder, regs: &[u16]) -> Option<f64> {
    if let Some(repr) = column.custom_repr() {
        let raw = order.assemble(regs.get(..repr.register_count())?)?;
        return Some(repr.decode(raw));
    }
    Some(match column {
        Column::Bcd => bcd_to_decimal(regs[0])? as f64,
        Column::Bcd32 => bcd_to_decimal(order.make_word(regs[0], regs[1]))? as f64,
        _ => return None,
    })
}

impl Interpretor {
    pub fn new(interpretation: InterpretorConfig, word_order: WordOrder) -> Self {
        let mut interpretor = Self {
            config: interpretation,
            word_order,
            header: String::new(),
        };
        interpretor.rebuild_header();
        interpretor
    }

    fn rebuild_header(&mut self) {
        let mut header = String::new();

        self.write_prefix(&mut header, "time", "ago");
        let _ = write!(header, "{:>w$}: ", "address", w = ADDRESS_W);

        for col in self.enabled_columns() {
            let _ = write!(header, "{:<w$} ", col.column.name(), w = col.width);
        }
        if self.config.label {
            header.push_str("label");
        }

        self.header = header;
    }

    fn enabled_columns(&self) -> impl Iterator<Item = &'static ColumnSpec> + '_ {
        COLUMNS.iter().filter(|col| self.config.get(col.column))
    }

    fn write_prefix(&self, out: &mut String, time: &str, ago: &str) {
        if self.config.time {
            let _ = write!(out, "{time:<w$} ", w = TIME_W);
        }
        if self.config.ago {
            let _ = write!(out, "{ago:<w$} ", w = AGO_W);
        }
    }

    fn write_row_prefix(
        &self,
        out: &mut String,
        address: u16,
        read: Option<(&str, chrono::Duration)>,
    ) {
        if self.config.time {
            let time = read.map_or(NO_VALUE, |(time, _)| time);
            let _ = write!(out, "{time:<w$} ", w = TIME_W);
        }
        if self.config.ago {
            let mark = out.len();
            match read {
                Some((_, elapsed)) => write_ago(out, elapsed),
                None => out.push_str(NO_VALUE),
            }
            pad_to(out, mark, AGO_W);
        }
        self.write_address(out, address);
    }

    pub fn toggle(&mut self, column: Column) {
        self.config.toggle(column);
        self.rebuild_header();
    }

    pub fn is_enabled(&self, column: Column) -> bool {
        self.config.get(column)
    }

    pub fn config(&self) -> InterpretorConfig {
        self.config.clone()
    }

    pub fn set_word_order(&mut self, word_order: WordOrder) {
        self.word_order = word_order;
    }

    pub fn header(&self) -> &str {
        &self.header
    }

    pub fn prefix_width(&self) -> u16 {
        let mut prefix = String::new();
        self.write_row_prefix(&mut prefix, 0, None);
        prefix.chars().count() as u16
    }

    pub fn row_segments(&self) -> Vec<RowSegment> {
        let mut segments = Vec::new();
        let mut start = 0usize;
        if self.config.time {
            segments.push(RowSegment::new("time", start, TIME_W));
            start += TIME_W + 1;
        }
        if self.config.ago {
            segments.push(RowSegment::new("ago", start, AGO_W));
            start += AGO_W + 1;
        }
        segments.push(RowSegment::new("address", start, ADDRESS_W));
        start += ADDRESS_W + 2;
        for col in self.enabled_columns() {
            segments.push(RowSegment::new(col.column.name(), start, col.width));
            start += col.width + 1;
        }
        if self.config.label {
            segments.push(RowSegment::new("label", start, usize::MAX));
        }
        segments
    }

    fn write_address(&self, out: &mut String, value: u16) {
        if self.config.address_hex {
            let _ = write!(out, "{value:>w$X}: ", w = ADDRESS_W);
        } else {
            let _ = write!(out, "{value:>w$}: ", w = ADDRESS_W);
        }
    }

    pub fn placeholder(&self, address: u16, label: Option<&str>) -> String {
        let mut row = String::with_capacity(self.header.len() + 32);
        self.write_row_prefix(&mut row, address, None);

        for col in self.enabled_columns() {
            let mark = row.len();
            row.push_str(NO_VALUE);
            pad_to(&mut row, mark, col.width);
        }

        if self.config.label
            && let Some(text) = label
        {
            row.push_str(text);
        }

        row
    }

    #[allow(clippy::too_many_arguments)]
    pub fn format_row(
        &self,
        address: u16,
        value: u16,
        next: [Option<u16>; 3],
        time_text: &str,
        elapsed: chrono::Duration,
        custom: Option<&str>,
        label: Option<&str>,
    ) -> String {
        let mut row = String::with_capacity(self.header.len() + 32);
        self.write_row_prefix(&mut row, address, Some((time_text, elapsed)));

        let ctx = RowCtx::new(self.word_order, value, next, custom);
        for col in self.enabled_columns() {
            let mark = row.len();
            (col.render)(&ctx, col.width, &mut row);
            pad_to(&mut row, mark, col.width);
        }

        if self.config.label
            && let Some(t) = label
        {
            row.push_str(t);
        }

        row
    }

    pub fn interpret_all(
        &self,
        value: u16,
        next: [Option<u16>; 3],
        custom: Option<&str>,
        label: Option<&str>,
    ) -> Vec<(&'static str, String)> {
        let ctx = RowCtx::new(self.word_order, value, next, custom);
        let mut entries: Vec<(&'static str, String)> = COLUMNS
            .iter()
            .map(|col| {
                let mut cell = String::new();
                (col.render)(&ctx, INSPECT_W, &mut cell);
                (col.column.name(), cell)
            })
            .collect();
        entries.push(("label", label.unwrap_or("").to_string()));
        entries
    }
}

fn float_cell<T: std::fmt::Display + std::fmt::LowerExp>(x: T, width: usize, out: &mut String) {
    let s = format!("{x}");
    if s.len() > width {
        let _ = write!(out, "{x:.3e}");
    } else {
        out.push_str(&s);
    }
}

fn glyph(b: u8) -> char {
    let c = b as char;
    if c.is_ascii_graphic() { c } else { '.' }
}

pub fn ascii_words(words: &[u16]) -> String {
    words
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .map(glyph)
        .collect()
}

fn ascii_cell(a: u16, b: u16, out: &mut String) {
    out.push_str(&ascii_words(&[a, b]));
}

fn bits_cell(value: u16, out: &mut String) {
    let b = format!("{value:016b}");
    let _ = write!(
        out,
        "{} {} {} {}",
        &b[0..4],
        &b[4..8],
        &b[8..12],
        &b[12..16]
    );
}

pub(crate) fn fmt_num(v: f64, is_float: bool) -> String {
    if !is_float {
        return format!("{v:.0}");
    }
    let mag = v.abs();
    if mag != 0.0 && !(1e-3..1e6).contains(&mag) {
        format!("{v:.2e}")
    } else if mag >= 100.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.3}")
    }
}

pub(crate) fn format_ago(elapsed: chrono::Duration) -> String {
    let mut out = String::new();
    write_ago(&mut out, elapsed);
    out
}

fn write_ago(out: &mut String, elapsed: chrono::Duration) {
    let secs = elapsed.num_seconds();
    if secs <= 0 {
        out.push_str("now");
    } else if secs < 60 {
        let _ = write!(out, "{secs}s ago");
    } else if secs < 3600 {
        let _ = write!(out, "{}m ago", secs / 60);
    } else {
        out.push_str(">1h ago");
    }
}

fn pad_to(out: &mut String, mark: usize, width: usize) {
    let written = out[mark..].chars().count();
    for _ in written..width {
        out.push(' ');
    }
    out.push(' ');
}

fn bcd_to_decimal<T: num_traits::PrimInt>(value: T) -> Option<T> {
    let nibbles = std::mem::size_of::<T>() * 2;
    let mut result = T::zero();
    let ten = T::from(10)?;
    let mask = T::from(0xF)?;
    for i in (0..nibbles).rev() {
        let nibble = (value >> (i * 4)) & mask;
        if nibble > T::from(9)? {
            return None;
        }
        result = result * ten + nibble;
    }
    Some(result)
}

fn m10k_to_u32(value: u32) -> (u16, u16) {
    let high = (value >> 16) as u16;
    let low = (value & 0xFFFF) as u16;

    (high, low)
}

fn m10k_to_i32(value: u32) -> (i16, i16) {
    let high = (value >> 16) as i16;
    let low = (value & 0xFFFF) as i16;

    (high, low)
}

pub(crate) fn f16_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = (bits >> 10) & 0x1f;
    let mantissa = bits & 0x3ff;

    let magnitude = match exponent {
        0 => (mantissa as f32) * 2f32.powi(-24),
        0x1f => {
            if mantissa == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + (mantissa as f32) / 1024.0) * 2f32.powi(exponent as i32 - 15),
    };

    sign * magnitude
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_segments_line_up_with_the_header() {
        for (time, ago, label) in [
            (true, true, true),
            (false, false, false),
            (true, false, true),
        ] {
            let config = InterpretorConfig {
                time,
                ago,
                label,
                ..InterpretorConfig::default()
            };
            let interpretor = Interpretor::new(config, WordOrder::ABCD);
            let header = interpretor.header();
            for segment in interpretor.row_segments() {
                assert_eq!(
                    segment.text(header),
                    segment.name,
                    "segment {} is misaligned in {header:?}",
                    segment.name
                );
            }
        }
    }

    #[test]
    fn row_segments_line_up_with_a_rendered_row() {
        let interpretor = interpretor();
        let row = interpretor.format_row(
            42,
            0x1234,
            [Some(0), Some(0), Some(0)],
            "12:00:00.000",
            chrono::Duration::zero(),
            None,
            Some("pump"),
        );
        let named = |name: &str| {
            interpretor
                .row_segments()
                .into_iter()
                .find(|s| s.name == name)
                .expect("segment")
                .text(&row)
        };
        assert_eq!(named("address"), "42");
        assert_eq!(named("u16"), "4660");
        assert_eq!(named("hex"), "1234");
        assert_eq!(named("time"), "12:00:00.000");
    }

    fn interpretor() -> Interpretor {
        let config = InterpretorConfig {
            time: true,
            ago: true,
            address_hex: false,
            ..InterpretorConfig::default()
        };
        Interpretor::new(config, WordOrder::ABCD)
    }

    #[test]
    fn row_prefix_pads_time_ago_and_address() {
        let row = interpretor().format_row(
            5,
            1,
            [None; 3],
            "12:34:56.789",
            chrono::Duration::seconds(3),
            None,
            None,
        );
        assert!(
            row.starts_with("12:34:56.789 3s ago          5: "),
            "{row:?}"
        );
    }

    #[test]
    fn placeholder_prefix_matches_row_prefix_width() {
        let i = interpretor();
        let placeholder = i.placeholder(5, None);
        assert!(
            placeholder.starts_with("-            -               5: "),
            "{placeholder:?}"
        );
        assert_eq!(placeholder.find("5: "), Some(i.prefix_width() as usize - 3));
    }

    #[test]
    fn ago_text() {
        assert_eq!(format_ago(chrono::Duration::seconds(0)), "now");
        assert_eq!(format_ago(chrono::Duration::seconds(59)), "59s ago");
        assert_eq!(format_ago(chrono::Duration::seconds(125)), "2m ago");
        assert_eq!(format_ago(chrono::Duration::seconds(3600)), ">1h ago");
    }
}
