use crate::config::{Column, InterpretorConfig};
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

const INDEX_W: usize = 5;
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
            custom: custom.unwrap_or("--").to_string(),
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
    ColumnSpec { name: "hex32",   width: 9,  enabled: |c| c.hex32,    render: |c, _, o| if c.two() { let _ = write!(o, "{:08X}", c.word); } else { o.push('-'); } },
    ColumnSpec { name: "f16",     width: 10, enabled: |c| c.f16,      render: |c, w, o| float_cell(f16_to_f32(c.value), w, o) },
    ColumnSpec { name: "bcd",     width: 6,  enabled: |c| c.bcd,      render: |c, _, o| match bcd_to_decimal(c.value) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str("--") } },
    ColumnSpec { name: "bcd32",   width: 10, enabled: |c| c.bcd32,    render: |c, _, o| if c.two() { match bcd_to_decimal_u32(c.word) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str("--") } } else { o.push('-'); } },
    ColumnSpec { name: "u32",     width: 10, enabled: |c| c.u32,      render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word); } else { o.push('-'); } },
    ColumnSpec { name: "i32",     width: 11, enabled: |c| c.i32,      render: |c, _, o| if c.two() { let _ = write!(o, "{}", c.word as i32); } else { o.push('-'); } },
    ColumnSpec { name: "u32m10k", width: 11, enabled: |c| c.u32_m10k, render: |c, _, o| if c.two() { match m10k_to_u32(c.word) { Some((h, l)) => { let _ = write!(o, "{h}/{l}"); } None => o.push_str("--") } } else { o.push('-'); } },
    ColumnSpec { name: "i32m10k", width: 14, enabled: |c| c.i32_m10k, render: |c, _, o| if c.two() { match m10k_to_i32(c.word) { Some((h, l)) => { let _ = write!(o, "{h}/{l}"); } None => o.push_str("--") } } else { o.push('-'); } },
    ColumnSpec { name: "u64",     width: 20, enabled: |c| c.u64,      render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword); } else { o.push('-'); } },
    ColumnSpec { name: "i64",     width: 21, enabled: |c| c.i64,      render: |c, _, o| if c.four() { let _ = write!(o, "{}", c.dword as i64); } else { o.push('-'); } },
    ColumnSpec { name: "f32",     width: 10, enabled: |c| c.f32,      render: |c, w, o| if c.two() { float_cell(f32::from_bits(c.word), w, o) } else { o.push('-'); } },
    ColumnSpec { name: "f64",     width: 12, enabled: |c| c.f64,      render: |c, w, o| if c.four() { float_cell(f64::from_bits(c.dword), w, o) } else { o.push('-'); } },
    ColumnSpec { name: "ascii",   width: 5,  enabled: |c| c.ascii,    render: |c, _, o| ascii_cell(c.value, c.next[0].unwrap_or_default(), o) },
    ColumnSpec { name: "bits",    width: 19, enabled: |c| c.bits,     render: |c, _, o| bits_cell(c.value, o) },
    ColumnSpec { name: "custom",  width: 18, enabled: |c| c.custom,   render: |c, _, o| o.push_str(&c.custom) },
];

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
        let _ = write!(header, "{:>w$}: ", "index", w = INDEX_W);

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
        let mut width = (INDEX_W + 2) as u16; // index + ": "
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
            .flat_map(|&(_, v)| [(v >> 8) as u8, (v & 0xFF) as u8])
            .map(|b| {
                let c = b as char;
                if c.is_ascii_graphic() {
                    c
                } else {
                    '·'
                }
            })
            .collect()
    }

    pub fn placeholder(&self, index: u16, label: Option<&str>) -> String {
        let dash = "--";
        let mut row = String::new();

        if self.config.time {
            let _ = write!(row, "{dash:<w$} ", w = TIME_W);
        }
        if self.config.ago {
            let _ = write!(row, "{dash:<w$} ", w = AGO_W);
        }
        if self.config.index_hex {
            let _ = write!(row, "{index:>w$X}: ", w = INDEX_W);
        } else {
            let _ = write!(row, "{index:>w$}: ", w = INDEX_W);
        }

        for col in COLUMNS {
            if (col.enabled)(&self.config) {
                let _ = write!(row, "{dash:<w$} ", w = col.width);
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
        if self.config.index_hex {
            let _ = write!(row, "{address:>w$X}: ", w = INDEX_W);
        } else {
            let _ = write!(row, "{address:>w$}: ", w = INDEX_W);
        }

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

fn ascii_cell(a: u16, b: u16, out: &mut String) {
    for n in [a, b] {
        for byte in [(n >> 8) as u8, (n & 0xFF) as u8] {
            let ch = byte as char;
            out.push(if ch.is_ascii_graphic() { ch } else { '·' });
        }
    }
}

fn bits_cell(value: u16, out: &mut String) {
    let b = format!("{value:016b}");
    let _ = write!(out, "{} {} {} {}", &b[0..4], &b[4..8], &b[8..12], &b[12..16]);
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

fn bcd_to_decimal(value: u16) -> Option<u16> {
    let mut result = 0u16;
    for shift in [12, 8, 4, 0] {
        let nibble = (value >> shift) & 0xF;
        if nibble > 9 {
            return None;
        }
        result = result * 10 + nibble;
    }
    Some(result)
}

fn bcd_to_decimal_u32(value: u32) -> Option<u32> {
    let mut result = 0u32;
    for shift in [28, 24, 20, 16, 12, 8, 4, 0] {
        let nibble = (value >> shift) & 0xF;
        if nibble > 9 {
            return None;
        }
        result = result * 10 + nibble;
    }
    Some(result)
}

fn m10k_to_u32(value: u32) -> Option<(u16, u16)> {
    let high = (value >> 16) as u16;
    let low = (value & 0xFFFF) as u16;

    Some((high, low))
}

fn m10k_to_i32(value: u32) -> Option<(i16, i16)> {
    let high = (value >> 16) as i16;
    let low = (value & 0xFFFF) as i16;

    Some((high, low))
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
