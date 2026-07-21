use crate::config::{Column, InterpretorConfig};
use crate::constants::{NO_VALUE, UNINTERPRETABLE};
use crate::modbus::WordOrder;
use crate::register::RegisterCellValue;
use chrono::{DateTime, Local};
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

struct ColumnSpec {
    name: &'static str,
    width: usize,
    enabled: fn(&InterpretorConfig) -> bool,
    render: fn(&RowCtx, usize, &mut String),
}

struct RowCtx {
    value: u16,
    next: [Option<u16>; 3],
    word: u32,
    dword: u64,
    custom: String,
}

impl RowCtx {
    fn new(order: WordOrder, value: u16, next: [Option<u16>; 3], custom: Option<&str>) -> Self {
        let word = order.make_word(value, next[0].unwrap_or_default());
        let second = order.make_word(next[1].unwrap_or_default(), next[2].unwrap_or_default());
        Self {
            value,
            next,
            word,
            dword: order.make_dword(word, second),
            custom: custom.unwrap_or(NO_VALUE).to_string(),
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
    ColumnSpec { name: "u16",     width: 5,  enabled: |c| c.u16,      render: |c, _, o| { let _ = write!(o, "{}", c.value); } },
    ColumnSpec { name: "i16",     width: 6,  enabled: |c| c.i16,      render: |c, _, o| { let _ = write!(o, "{}", c.value as i16); } },
    ColumnSpec { name: "u8s",     width: 8,  enabled: |c| c.u8s,      render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8, (c.value & 0xFF) as u8); } },
    ColumnSpec { name: "i8s",     width: 9,  enabled: |c| c.i8s,      render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8 as i8, (c.value & 0xFF) as u8 as i8); } },
    ColumnSpec { name: "hex",     width: 4,  enabled: |c| c.hex,      render: |c, _, o| { let _ = write!(o, "{:04X}", c.value); } },
    ColumnSpec { name: "hex32",   width: 9,  enabled: |c| c.hex32,    render: |c, _, o| if c.two() { let _ = write!(o, "{:08X}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "f16",     width: 10, enabled: |c| c.f16,      render: |c, w, o| float_cell(f16_to_f32(c.value), w, o) },
    ColumnSpec { name: "bcd",     width: 6,  enabled: |c| c.bcd,      render: |c, _, o| match bcd_to_decimal(c.value) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } },
    ColumnSpec { name: "bcd32",   width: 10, enabled: |c| c.bcd32,    render: |c, _, o| if c.two() { match bcd_to_decimal(c.word) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "u32",     width: 10, enabled: |c| c.u32,      render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "i32",     width: 11, enabled: |c| c.i32,      render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word as i32); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "u32m10k", width: 11, enabled: |c| c.u32_m10k, render: |c, _, o| if c.two() { let (h, l) = m10k_to_u32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "i32m10k", width: 14, enabled: |c| c.i32_m10k, render: |c, _, o| if c.two() { let (h, l) = m10k_to_i32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "u64",     width: 20, enabled: |c| c.u64,      render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "i64",     width: 21, enabled: |c| c.i64,      render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword as i64); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "f32",     width: 10, enabled: |c| c.f32,      render: |c, w, o| if c.two() { float_cell(f32::from_bits(c.word), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "f64",     width: 12, enabled: |c| c.f64,      render: |c, w, o| if c.four() { float_cell(f64::from_bits(c.dword), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { name: "ascii",   width: 5,  enabled: |c| c.ascii,    render: |c, _, o| ascii_cell(c.value, c.next[0].unwrap_or_default(), o) },
    ColumnSpec { name: "bits",    width: 19, enabled: |c| c.bits,     render: |c, _, o| bits_cell(c.value, o) },
    ColumnSpec { name: "custom",  width: 18, enabled: |c| c.custom,   render: |c, _, o| o.push_str(&c.custom) },
];

impl Column {
    pub fn graph_width(self) -> Option<usize> {
        Some(match self {
            Column::U16 | Column::I16 | Column::F16 | Column::Bcd => 1,
            Column::U32 | Column::I32 | Column::F32 | Column::Bcd32 => 2,
            Column::U64 | Column::I64 | Column::F64 => 4,
            _ => return None,
        })
    }

    pub fn is_graphable(self) -> bool {
        self.graph_width().is_some()
    }

    pub fn graph_is_float(self) -> bool {
        matches!(self, Column::F16 | Column::F32 | Column::F64)
    }
}

pub fn graph_value(column: Column, order: WordOrder, regs: &[u16]) -> Option<f64> {
    let word = |a: usize| order.make_word(regs[a], regs[a + 1]);
    Some(match column {
        Column::U16 => regs[0] as f64,
        Column::I16 => regs[0] as i16 as f64,
        Column::F16 => f16_to_f32(regs[0]) as f64,
        Column::Bcd => bcd_to_decimal(regs[0])? as f64,
        Column::U32 => word(0) as f64,
        Column::I32 => word(0) as i32 as f64,
        Column::F32 => f32::from_bits(word(0)) as f64,
        Column::Bcd32 => bcd_to_decimal(word(0))? as f64,
        Column::U64 => order.make_dword(word(0), word(2)) as f64,
        Column::I64 => order.make_dword(word(0), word(2)) as i64 as f64,
        Column::F64 => f64::from_bits(order.make_dword(word(0), word(2))),
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

        if self.config.time {
            let _ = write!(header, "{:<w$} ", "time", w = TIME_W);
        }
        if self.config.ago {
            let _ = write!(header, "{:<w$} ", "ago", w = AGO_W);
        }
        let _ = write!(header, "{:>w$}: ", "address", w = ADDRESS_W);

        for col in COLUMNS {
            if (col.enabled)(&self.config) {
                let _ = write!(header, "{:<w$} ", col.name, w = col.width);
            }
        }
        if self.config.label {
            header.push_str("label");
        }

        self.header = header;
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
        let mut width = (ADDRESS_W + 2) as u16; // address + ": "
        if self.config.time {
            width += (TIME_W + 1) as u16; // value + trailing space
        }
        if self.config.ago {
            width += (AGO_W + 1) as u16;
        }
        width
    }

    pub fn shows_ascii(&self) -> bool {
        self.config.ascii
    }

    pub fn ascii_string(&self, data: &[RegisterCellValue]) -> String {
        data.iter()
            .flat_map(|&(_, v)| v.to_be_bytes())
            .map(glyph)
            .collect()
    }

    fn write_address(&self, out: &mut String, value: u16) {
        if self.config.address_hex {
            let _ = write!(out, "{value:>w$X}: ", w = ADDRESS_W);
        } else {
            let _ = write!(out, "{value:>w$}: ", w = ADDRESS_W);
        }
    }

    pub fn placeholder(&self, address: u16, label: Option<&str>) -> String {
        let mut row = String::new();

        if self.config.time {
            let _ = write!(row, "{NO_VALUE:<w$} ", w = TIME_W);
        }
        if self.config.ago {
            let _ = write!(row, "{NO_VALUE:<w$} ", w = AGO_W);
        }
        self.write_address(&mut row, address);

        for col in COLUMNS {
            if (col.enabled)(&self.config) {
                let _ = write!(row, "{NO_VALUE:<w$} ", w = col.width);
            }
        }

        if self.config.label {
            if let Some(text) = label {
                row.push_str(text);
            }
        }

        row
    }

    #[allow(clippy::too_many_arguments)]
    pub fn format_row(
        &self,
        address: u16,
        value: u16,
        next: [Option<u16>; 3],
        read_at: DateTime<Local>,
        now: DateTime<Local>,
        custom: Option<&str>,
        label: Option<&str>,
    ) -> String {
        let mut row = String::new();
        if self.config.time {
            let formatted = read_at.format("%H:%M:%S.%3f").to_string();
            let _ = write!(row, "{formatted:<w$} ", w = TIME_W);
        }
        if self.config.ago {
            let ago = format_ago(now.signed_duration_since(read_at));
            let _ = write!(row, "{ago:<w$} ", w = AGO_W);
        }
        self.write_address(&mut row, address);

        let ctx = RowCtx::new(self.word_order, value, next, custom);
        for col in COLUMNS {
            if (col.enabled)(&self.config) {
                let mark = row.len();
                (col.render)(&ctx, col.width, &mut row);
                let written = row[mark..].chars().count();
                for _ in written..col.width {
                    row.push(' ');
                }
                row.push(' ');
            }
        }

        if self.config.label {
            if let Some(t) = label {
                row.push_str(t);
            }
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
                (col.name, cell)
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
    if c.is_ascii_graphic() {
        c
    } else {
        '·'
    }
}

fn ascii_cell(a: u16, b: u16, out: &mut String) {
    for n in [a, b] {
        for byte in n.to_be_bytes() {
            out.push(glyph(byte));
        }
    }
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
    let secs = elapsed.num_seconds();
    if secs <= 0 {
        "now".to_string()
    } else if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        ">1h ago".to_string()
    }
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
