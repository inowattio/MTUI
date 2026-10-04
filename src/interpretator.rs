use crate::config::{AddressMode, Column, InterpretorConfig, TimeMode};
use crate::constants::{ELLIPSIS, NO_VALUE, UNINTERPRETABLE};
use crate::custom::CustomRepr;
use crate::modbus::WordOrder;
use std::fmt::Write as _;

#[derive(Debug, Clone)]
pub struct Interpretor {
    config: InterpretorConfig,
    word_order: WordOrder,
    header: String,
    order: Vec<Column>,
    enabled: Vec<EnabledColumn>,
    lookahead: usize,
    label_auto: usize,
    custom_auto: usize,
}

#[derive(Debug, Clone, Copy)]
struct EnabledColumn {
    spec: &'static ColumnSpec,
    width: usize,
}

const ADDRESS_W: usize = 7;
const TIME_W: usize = 12;
const INSPECT_W: usize = 21;
const CONFIGURED: usize = 0;
pub const WIDTH_MAX: usize = 64;
pub const LOOKAHEAD: usize = 7;
pub type Lookahead = [Option<u16>; LOOKAHEAD];

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

    pub const fn end(self) -> usize {
        self.start.saturating_add(self.width)
    }
}

#[derive(Debug)]
struct ColumnSpec {
    column: Column,
    width: usize,
    render: fn(&RowCtx, usize, &mut String),
}

struct RowCtx<'a> {
    value: u16,
    next: Lookahead,
    word: u32,
    dword: u64,
    qword: u128,
    custom: &'a str,
    time: &'a str,
    elapsed: Option<chrono::Duration>,
    label: &'a str,
    address: u16,
    time_mode: TimeMode,
    address_mode: AddressMode,
}

struct RowData<'a> {
    address: u16,
    value: u16,
    next: Lookahead,
    custom: Option<&'a str>,
    time: &'a str,
    elapsed: Option<chrono::Duration>,
    label: Option<&'a str>,
}

impl<'a> RowCtx<'a> {
    fn new(config: &InterpretorConfig, order: WordOrder, data: RowData<'a>) -> Self {
        let [b, c, d, e, f, g, h] = data.next.map(Option::unwrap_or_default);
        Self {
            value: data.value,
            next: data.next,
            word: order.make_word(data.value, b),
            dword: order.make_dword([data.value, b, c, d]),
            qword: order.make_qword([data.value, b, c, d, e, f, g, h]),
            custom: data.custom.unwrap_or(NO_VALUE),
            time: data.time,
            elapsed: data.elapsed,
            label: data.label.unwrap_or(""),
            address: data.address,
            time_mode: config.time_mode,
            address_mode: config.address_mode,
        }
    }

    fn has_next(&self, n: usize) -> bool {
        self.next[..n].iter().all(Option::is_some)
    }
}

#[rustfmt::skip]
const COLUMNS: &[ColumnSpec] = &[
    ColumnSpec { column: Column::Address, width: ADDRESS_W, render: |c, w, o| address_cell(c.address, c.address_mode, w, o) },
    ColumnSpec { column: Column::Time,    width: TIME_W, render: |c, w, o| time_cell(c, w, o) },
    ColumnSpec { column: Column::U16,     width: 5,  render: |c, _, o| { let _ = write!(o, "{}", c.value); } },
    ColumnSpec { column: Column::I16,     width: 6,  render: |c, _, o| { let _ = write!(o, "{}", c.value as i16); } },
    ColumnSpec { column: Column::U8,     width: 8,  render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8, (c.value & 0xFF) as u8); } },
    ColumnSpec { column: Column::I8,     width: 9,  render: |c, _, o| { let _ = write!(o, "{}/{}", (c.value >> 8) as u8 as i8, (c.value & 0xFF) as u8 as i8); } },
    ColumnSpec { column: Column::Hex,     width: 4,  render: |c, _, o| { let _ = write!(o, "{:04X}", c.value); } },
    ColumnSpec { column: Column::Hex32,   width: 9,  render: |c, _, o| if c.has_next(1) { let _ = write!(o, "{:08X}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F16,     width: 10, render: |c, w, o| float_cell(f16_to_f32(c.value), w, o) },
    ColumnSpec { column: Column::Bcd,     width: 6,  render: |c, _, o| match bcd_to_decimal(c.value) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } },
    ColumnSpec { column: Column::Bcd32,   width: 10, render: |c, _, o| if c.has_next(1) { match bcd_to_decimal(c.word) { Some(n) => { let _ = write!(o, "{n}"); } None => o.push_str(UNINTERPRETABLE) } } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U32,     width: 10, render: |c, _, o| if c.has_next(1) { let _ = write!(o, "{}", c.word); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I32,     width: 11, render: |c, _, o| if c.has_next(1) { let _ = write!(o, "{}", c.word as i32); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U32M10K, width: 11, render: |c, _, o| if c.has_next(1) { let (h, l) = m10k_to_u32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I32M10K, width: 14, render: |c, _, o| if c.has_next(1) { let (h, l) = m10k_to_i32(c.word); let _ = write!(o, "{h}/{l}"); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U64,     width: 20, render: |c, _, o| if c.has_next(3) { let _ = write!(o, "{}", c.dword); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I64,     width: 21, render: |c, _, o| if c.has_next(3) { let _ = write!(o, "{}", c.dword as i64); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::U128,    width: 39, render: |c, _, o| if c.has_next(7) { let _ = write!(o, "{}", c.qword); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::I128,    width: 40, render: |c, _, o| if c.has_next(7) { let _ = write!(o, "{}", c.qword as i128); } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F32,     width: 10, render: |c, w, o| if c.has_next(1) { float_cell(f32::from_bits(c.word), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F64,     width: 12, render: |c, w, o| if c.has_next(3) { float_cell(f64::from_bits(c.dword), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::F128,    width: 12, render: |c, w, o| if c.has_next(7) { float_cell(f128_to_f64(c.qword), w, o) } else { o.push_str(UNINTERPRETABLE); } },
    ColumnSpec { column: Column::Ascii,   width: 5,  render: |c, _, o| ascii_cell(c.value, c.next[0].unwrap_or_default(), o) },
    ColumnSpec { column: Column::Bits,    width: 19, render: |c, _, o| bits_cell(c.value, o) },
    ColumnSpec { column: Column::Custom,  width: CONFIGURED, render: |c, w, o| clipped_cell(c.custom, w, o) },
    ColumnSpec { column: Column::Label,   width: CONFIGURED, render: |c, w, o| clipped_cell(c.label, w, o) },
];

impl Column {
    const fn is_meta(self) -> bool {
        matches!(self, Self::Address | Self::Time | Self::Label)
    }

    pub const fn words(self) -> usize {
        match self.custom_repr() {
            Some(repr) => repr.register_count(),
            None => match self {
                Self::Hex32 | Self::Bcd32 | Self::U32M10K | Self::I32M10K | Self::Ascii => 2,
                _ => 1,
            },
        }
    }

    pub fn graph_width(self) -> Option<usize> {
        (self.custom_repr().is_some() || matches!(self, Self::Bcd | Self::Bcd32))
            .then(|| self.words())
    }

    pub fn is_graphable(self) -> bool {
        self.graph_width().is_some()
    }

    pub const fn graph_is_float(self) -> bool {
        matches!(self, Self::F16 | Self::F32 | Self::F64 | Self::F128)
    }

    pub const fn custom_repr(self) -> Option<CustomRepr> {
        Some(match self {
            Self::U16 => CustomRepr::U16,
            Self::I16 => CustomRepr::I16,
            Self::F16 => CustomRepr::F16,
            Self::U32 => CustomRepr::U32,
            Self::I32 => CustomRepr::I32,
            Self::F32 => CustomRepr::F32,
            Self::U64 => CustomRepr::U64,
            Self::I64 => CustomRepr::I64,
            Self::F64 => CustomRepr::F64,
            Self::U128 => CustomRepr::U128,
            Self::I128 => CustomRepr::I128,
            Self::F128 => CustomRepr::F128,
            _ => return None,
        })
    }
}

pub fn following(address: u16, count: usize, at: impl Fn(u16) -> Option<u16>) -> Lookahead {
    std::array::from_fn(|i| {
        (i < count)
            .then(|| at(address.saturating_add(i as u16 + 1)))
            .flatten()
    })
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
            order: Vec::new(),
            enabled: Vec::new(),
            lookahead: 0,
            label_auto: 0,
            custom_auto: 0,
        };
        interpretor.rebuild();
        interpretor
    }

    pub fn ordered_columns(&self) -> &[Column] {
        &self.order
    }

    pub fn move_column(&mut self, column: Column, right: bool) -> bool {
        let Some(from) = self.config.visible.iter().position(|&c| c == column) else {
            return false;
        };
        let to = if right {
            from + 1
        } else {
            from.wrapping_sub(1)
        };
        if to >= self.config.visible.len() {
            return false;
        }
        self.config.visible.swap(from, to);
        self.rebuild();
        true
    }

    fn effective_width(&self, spec: &ColumnSpec) -> usize {
        let (setting, auto) = match spec.column {
            Column::Label => (self.config.label_width, self.label_auto),
            Column::Custom => (self.config.custom_width, self.custom_auto),
            _ => return spec.width,
        };
        let min = spec.column.name().chars().count();
        let width = match setting {
            0 => auto,
            n => n as usize,
        };
        width.clamp(min, WIDTH_MAX)
    }

    fn rebuild(&mut self) {
        self.order.clear();
        let visible = self.config.visible.iter().copied();
        let built_in = COLUMNS.iter().map(|spec| spec.column);
        for column in visible.chain(built_in) {
            if !self.order.contains(&column) {
                self.order.push(column);
            }
        }
        self.enabled = self
            .config
            .visible
            .iter()
            .filter_map(|&column| COLUMNS.iter().find(|spec| spec.column == column))
            .map(|spec| EnabledColumn {
                spec,
                width: self.effective_width(spec),
            })
            .collect();
        self.lookahead = self
            .enabled
            .iter()
            .map(|col| col.spec.column.words() - 1)
            .max()
            .unwrap_or(0);
        self.rebuild_header();
    }

    pub const fn lookahead(&self) -> usize {
        self.lookahead
    }

    pub const fn label_width(&self) -> u16 {
        self.config.label_width
    }

    pub fn set_label_width(&mut self, width: u16) {
        self.config.label_width = width;
        self.rebuild();
    }

    pub fn set_label_auto(&mut self, longest: usize) {
        if self.label_auto != longest {
            self.label_auto = longest;
            self.rebuild();
        }
    }

    pub const fn custom_width(&self) -> u16 {
        self.config.custom_width
    }

    pub fn set_custom_width(&mut self, width: u16) {
        self.config.custom_width = width;
        self.rebuild();
    }

    pub fn set_custom_auto(&mut self, longest: usize) {
        if self.custom_auto != longest {
            self.custom_auto = longest;
            self.rebuild();
        }
    }

    fn rebuild_header(&mut self) {
        let mut header = String::new();

        for col in self.enabled_columns() {
            let _ = write!(header, "{:<w$} ", col.spec.column.name(), w = col.width);
        }
        self.header = header;
    }

    fn enabled_columns(&self) -> impl Iterator<Item = EnabledColumn> + '_ {
        self.enabled.iter().copied()
    }

    pub fn toggle(&mut self, column: Column) {
        self.config.toggle(column);
        self.rebuild();
    }

    pub fn is_enabled(&self, column: Column) -> bool {
        self.config.is_visible(column)
    }

    pub fn config(&self) -> InterpretorConfig {
        self.config.clone()
    }

    pub const fn set_word_order(&mut self, word_order: WordOrder) {
        self.word_order = word_order;
    }

    pub fn header(&self) -> &str {
        &self.header
    }

    pub fn prefix_width(&self) -> u16 {
        match self.enabled.first() {
            Some(col) if col.spec.column == Column::Address => (col.width + 1) as u16,
            _ => 0,
        }
    }

    pub const fn time_mode(&self) -> TimeMode {
        self.config.time_mode
    }

    pub const fn address_mode(&self) -> AddressMode {
        self.config.address_mode
    }

    pub const fn set_time_mode(&mut self, mode: TimeMode) {
        self.config.time_mode = mode;
    }

    pub const fn set_address_mode(&mut self, mode: AddressMode) {
        self.config.address_mode = mode;
    }

    pub fn row_segments(&self) -> Vec<RowSegment> {
        let mut segments = Vec::new();
        let mut start = 0usize;
        for col in self.enabled_columns() {
            segments.push(RowSegment::new(col.spec.column.name(), start, col.width));
            start += col.width + 1;
        }
        segments
    }

    pub fn placeholder(&self, address: u16, label: Option<&str>) -> String {
        let mut row = String::with_capacity(self.header.len() + 32);
        let ctx = RowCtx::new(
            &self.config,
            self.word_order,
            RowData {
                address,
                value: 0,
                next: [None; LOOKAHEAD],
                custom: None,
                time: NO_VALUE,
                elapsed: None,
                label,
            },
        );

        for col in self.enabled_columns() {
            let mark = row.len();
            if col.spec.column.is_meta() {
                (col.spec.render)(&ctx, col.width, &mut row);
            } else {
                row.push_str(NO_VALUE);
            }
            pad_to(&mut row, mark, col.width);
        }

        row
    }

    #[allow(clippy::too_many_arguments)]
    pub fn format_row(
        &self,
        address: u16,
        value: u16,
        next: Lookahead,
        time_text: &str,
        elapsed: Option<chrono::Duration>,
        custom: Option<&str>,
        label: Option<&str>,
    ) -> String {
        let mut row = String::with_capacity(self.header.len() + 32);
        let ctx = RowCtx::new(
            &self.config,
            self.word_order,
            RowData {
                address,
                value,
                next,
                custom,
                time: time_text,
                elapsed,
                label,
            },
        );
        for col in self.enabled_columns() {
            let mark = row.len();
            (col.spec.render)(&ctx, col.width, &mut row);
            pad_to(&mut row, mark, col.width);
        }

        row
    }

    #[allow(clippy::too_many_arguments)]
    pub fn interpret_all(
        &self,
        address: u16,
        value: u16,
        next: Lookahead,
        time: &str,
        elapsed: chrono::Duration,
        custom: Option<&str>,
        label: Option<&str>,
    ) -> Vec<(&'static str, String)> {
        let ctx = RowCtx::new(
            &self.config,
            self.word_order,
            RowData {
                address,
                value,
                next,
                custom,
                time,
                elapsed: Some(elapsed),
                label,
            },
        );
        COLUMNS
            .iter()
            .map(|col| {
                let mut cell = String::new();
                (col.render)(&ctx, INSPECT_W, &mut cell);
                (col.column.name(), cell.trim().to_string())
            })
            .collect()
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

const fn glyph(b: u8) -> char {
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

const AGO_NOW: &str = "now";
const AGO_SUFFIX: &str = " ago";
const AGO_OVER_AN_HOUR: &str = ">1h ago";

fn write_ago(out: &mut String, elapsed: chrono::Duration) {
    let secs = elapsed.num_seconds();
    if secs <= 0 {
        out.push_str(AGO_NOW);
    } else if secs < 60 {
        let _ = write!(out, "{secs}s{AGO_SUFFIX}");
    } else if secs < 3600 {
        let _ = write!(out, "{}m{AGO_SUFFIX}", secs / 60);
    } else {
        out.push_str(AGO_OVER_AN_HOUR);
    }
}

pub fn is_age(text: &str) -> bool {
    text == AGO_NOW
        || text == AGO_OVER_AN_HOUR
        || text
            .strip_suffix(AGO_SUFFIX)
            .and_then(|rest| rest.strip_suffix(['s', 'm']))
            .is_some_and(|count| !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit()))
}

fn address_cell(address: u16, mode: AddressMode, width: usize, out: &mut String) {
    match mode {
        AddressMode::Dec => {
            let _ = write!(out, "{address:>width$}");
        }
        AddressMode::Hex => {
            let _ = write!(out, "{address:>width$X}");
        }
    }
}

fn time_cell(ctx: &RowCtx, width: usize, out: &mut String) {
    match (ctx.time_mode, ctx.elapsed) {
        (TimeMode::Ago, Some(elapsed)) => write_ago(out, elapsed),
        _ => clipped_cell(ctx.time, width, out),
    }
}

fn clipped_cell(text: &str, width: usize, out: &mut String) {
    if text.chars().count() <= width {
        out.push_str(text);
        return;
    }
    let keep = width.saturating_sub(ELLIPSIS.chars().count());
    out.extend(text.chars().take(keep));
    out.push_str(ELLIPSIS);
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

const fn m10k_to_u32(value: u32) -> (u16, u16) {
    let high = (value >> 16) as u16;
    let low = (value & 0xFFFF) as u16;

    (high, low)
}

const fn m10k_to_i32(value: u32) -> (i16, i16) {
    let high = (value >> 16) as i16;
    let low = (value & 0xFFFF) as i16;

    (high, low)
}

pub(crate) fn f128_to_f64(bits: u128) -> f64 {
    const MANTISSA_BITS: u32 = 112;
    const BIAS: i32 = 16383;
    let negative = bits >> 127 != 0;
    let exponent = ((bits >> MANTISSA_BITS) & 0x7FFF) as i32;
    let mantissa = bits & ((1u128 << MANTISSA_BITS) - 1);

    let magnitude = match exponent {
        0 => 0.0,
        0x7FFF => {
            if mantissa == 0 {
                f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => {
            let shift = MANTISSA_BITS - 52;
            let significand = (1u128 << MANTISSA_BITS) | mantissa;
            let mut top = (significand >> shift) as u64;
            let rest = significand & ((1u128 << shift) - 1);
            let half = 1u128 << (shift - 1);
            if rest > half || (rest == half && top & 1 == 1) {
                top += 1;
            }
            scale_pow2(top as f64, exponent - BIAS - 52)
        }
    };

    if negative { -magnitude } else { magnitude }
}

fn scale_pow2(x: f64, exponent: i32) -> f64 {
    match exponent {
        e if e > 1023 => x * f64::INFINITY,
        e if e < -1022 => x * 2f64.powi(-1022) * 2f64.powi((e + 1022).max(-1022)),
        e => x * 2f64.powi(e),
    }
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

    fn visible(visible: Vec<Column>) -> Interpretor {
        let config = InterpretorConfig {
            visible,
            ..InterpretorConfig::default()
        };
        Interpretor::new(config, WordOrder::ABCD)
    }

    fn without(hidden: &[Column]) -> Interpretor {
        let mut config = InterpretorConfig::default();
        config.visible.retain(|c| !hidden.contains(c));
        Interpretor::new(config, WordOrder::ABCD)
    }

    fn segment_names(interpretor: &Interpretor) -> Vec<&'static str> {
        interpretor
            .row_segments()
            .into_iter()
            .map(|segment| segment.name)
            .collect()
    }

    #[test]
    fn the_visible_list_is_the_column_order() {
        let interpretor = visible(vec![Column::Hex, Column::I16, Column::Address, Column::U16]);
        assert_eq!(
            segment_names(&interpretor),
            ["hex", "i16", "address", "u16"]
        );
    }

    #[test]
    fn the_defaults_follow_the_built_in_order() {
        let interpretor = Interpretor::new(InterpretorConfig::default(), WordOrder::ABCD);
        assert_eq!(
            segment_names(&interpretor),
            [
                "address", "time", "u16", "i16", "hex", "ascii", "bits", "custom", "label"
            ]
        );
    }

    #[test]
    fn hidden_columns_trail_the_visible_ones_in_the_full_order() {
        let interpretor = visible(vec![Column::Hex, Column::Address]);
        let order = interpretor.ordered_columns();
        assert_eq!(&order[..2], [Column::Hex, Column::Address]);
        assert_eq!(order.len(), Column::ALL.len());
        assert_eq!(segment_names(&interpretor), ["hex", "address"]);
    }

    #[test]
    fn moving_a_column_stops_at_the_edges_and_skips_hidden_ones() {
        let mut interpretor = visible(vec![Column::Address, Column::U16, Column::Hex]);

        assert!(
            !interpretor.move_column(Column::Address, false),
            "already leftmost"
        );
        assert!(!interpretor.move_column(Column::Hex, true), "already last");
        assert!(!interpretor.move_column(Column::Label, true), "hidden");

        assert!(interpretor.move_column(Column::Address, true));
        assert_eq!(segment_names(&interpretor), ["u16", "address", "hex"]);
    }

    #[test]
    fn toggling_a_column_back_on_restores_its_built_in_position() {
        let mut interpretor = without(&[Column::Bits, Column::Ascii, Column::Custom]);
        interpretor.toggle(Column::U16);
        assert_eq!(
            segment_names(&interpretor),
            ["address", "time", "i16", "hex", "label"]
        );
        interpretor.toggle(Column::U16);
        assert_eq!(
            segment_names(&interpretor),
            ["address", "time", "u16", "i16", "hex", "label"]
        );
    }

    #[test]
    fn address_time_and_label_can_be_ordered_like_any_column() {
        let interpretor = visible(vec![
            Column::Label,
            Column::Hex,
            Column::Time,
            Column::Address,
            Column::U16,
            Column::I16,
        ]);
        assert_eq!(
            segment_names(&interpretor),
            ["label", "hex", "time", "address", "u16", "i16"]
        );
    }

    #[test]
    fn unknown_and_duplicate_column_keys_are_dropped_when_loading() {
        let config: InterpretorConfig =
            serde_json::from_str(r#"{"visible":["hex","not_a_column","i16","hex"]}"#)
                .expect("loads");
        assert_eq!(config.visible, vec![Column::Hex, Column::I16]);
    }

    #[test]
    fn row_segments_line_up_with_the_header() {
        for (address, time, label) in [
            (true, true, true),
            (false, false, false),
            (true, false, true),
        ] {
            let hidden: Vec<Column> = [
                (address, Column::Address),
                (time, Column::Time),
                (label, Column::Label),
            ]
            .into_iter()
            .filter_map(|(shown, column)| (!shown).then_some(column))
            .collect();
            let interpretor = without(&hidden);
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
            [Some(0); LOOKAHEAD],
            "12:00:00.000",
            Some(chrono::Duration::zero()),
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
        Interpretor::new(InterpretorConfig::default(), WordOrder::ABCD)
    }

    fn row_of(interpretor: &Interpretor) -> String {
        interpretor.format_row(
            5,
            1,
            [None; LOOKAHEAD],
            "12:34:56.789",
            Some(chrono::Duration::seconds(3)),
            None,
            None,
        )
    }

    #[test]
    fn the_address_leads_and_time_reads_the_clock_by_default() {
        let row = row_of(&interpretor());
        assert!(row.starts_with("      5 12:34:56.789 "), "{row:?}");
    }

    #[test]
    fn the_time_mode_switches_the_column_to_elapsed() {
        let mut interpretor = interpretor();
        interpretor.set_time_mode(TimeMode::Ago);
        let row = row_of(&interpretor);
        assert!(row.starts_with("      5 3s ago       "), "{row:?}");
    }

    #[test]
    fn a_row_without_an_age_shows_its_time_text_clipped_in_every_mode() {
        let mut interpretor = interpretor();
        let untimed = |i: &Interpretor, time: &str| {
            i.format_row(5, 1, [None; LOOKAHEAD], time, None, None, None)
        };
        for mode in TimeMode::ALL {
            interpretor.set_time_mode(mode);
            let row = untimed(&interpretor, "14:02:42.850");
            assert!(
                row.starts_with("      5 14:02:42.850 1 "),
                "{mode:?}: {row:?}"
            );
            let row = untimed(&interpretor, "2026-09-27T14:02:42Z");
            assert_eq!(segment(&interpretor, "time").text(&row), "2026-09-2...");
            assert_eq!(
                segment(&interpretor, "u16").text(&row),
                "1",
                "still aligned"
            );
        }
    }

    fn segment(interpretor: &Interpretor, name: &str) -> RowSegment {
        interpretor
            .row_segments()
            .into_iter()
            .find(|s| s.name == name)
            .expect("segment")
    }

    #[test]
    fn inspect_reports_the_real_address_unpadded() {
        let mut interpretor = interpretor();
        let entry = |i: &Interpretor, name: &str| {
            i.interpret_all(
                4660,
                1,
                [None; LOOKAHEAD],
                "12:34:56.789",
                chrono::Duration::seconds(3),
                None,
                None,
            )
            .into_iter()
            .find(|(key, _)| *key == name)
            .expect("entry")
            .1
        };

        assert_eq!(entry(&interpretor, "address"), "4660");
        interpretor.set_address_mode(AddressMode::Hex);
        assert_eq!(entry(&interpretor, "address"), "1234");
    }

    #[test]
    fn the_defaults_are_fixed_widths() {
        let defaults = InterpretorConfig::default();
        assert_ne!(defaults.label_width, 0, "label ships with a fixed width");
        assert_ne!(defaults.custom_width, 0, "custom ships with a fixed width");

        let interpretor = interpretor();
        assert_eq!(
            segment(&interpretor, "label").width,
            defaults.label_width as usize
        );
        assert_eq!(
            segment(&interpretor, "custom").width,
            defaults.custom_width as usize
        );
    }

    #[test]
    fn zero_fits_the_column_to_its_longest_value() {
        let mut interpretor = interpretor();
        interpretor.set_label_width(0);
        interpretor.set_custom_width(0);

        interpretor.set_label_auto(20);
        interpretor.set_custom_auto(12);
        assert_eq!(segment(&interpretor, "label").width, 20);
        assert_eq!(segment(&interpretor, "custom").width, 12);

        interpretor.set_label_auto(2);
        interpretor.set_custom_auto(2);
        assert_eq!(
            segment(&interpretor, "label").width,
            "label".len(),
            "never narrower than its own header"
        );
        assert_eq!(segment(&interpretor, "custom").width, "custom".len());

        interpretor.set_label_auto(500);
        assert_eq!(segment(&interpretor, "label").width, WIDTH_MAX);
    }

    #[test]
    fn an_explicit_label_width_wins_over_auto_and_clips() {
        let mut interpretor = interpretor();
        interpretor.set_label_auto(30);
        interpretor.set_label_width(8);

        let row = interpretor.format_row(
            5,
            1,
            [None; LOOKAHEAD],
            "12:34:56.789",
            Some(chrono::Duration::seconds(3)),
            None,
            Some("Plant.Line2.Pump"),
        );
        let label = segment(&interpretor, "label");
        assert_eq!(label.width, 8);
        assert_eq!(label.text(&row), "Plant...");
        assert_eq!(
            label.text(interpretor.header()),
            "label",
            "the header still lines up"
        );
    }

    #[test]
    fn the_address_mode_switches_the_column_to_hex() {
        let mut interpretor = interpretor();
        interpretor.set_address_mode(AddressMode::Hex);
        let row = interpretor.format_row(
            255,
            1,
            [None; LOOKAHEAD],
            "12:34:56.789",
            Some(chrono::Duration::seconds(3)),
            None,
            None,
        );
        assert!(row.starts_with("     FF "), "{row:?}");
    }

    #[test]
    fn a_placeholder_keeps_the_anchor_and_its_label() {
        let i = interpretor();
        let placeholder = i.placeholder(5, Some("pump"));
        assert!(placeholder.starts_with("      5 "), "{placeholder:?}");
        assert_eq!(i.prefix_width() as usize, "      5 ".len());
        let named = |name: &str| {
            i.row_segments()
                .into_iter()
                .find(|s| s.name == name)
                .expect("segment")
                .text(&placeholder)
        };
        assert_eq!(named("label"), "pump", "the label survives with no read");
        assert_eq!(named("u16"), NO_VALUE, "value columns have nothing to show");
    }

    #[test]
    fn f128_decodes_the_binary128_layout() {
        let one = 0x3FFF_u128 << 112;
        assert_eq!(f128_to_f64(one), 1.0);
        assert_eq!(f128_to_f64(one | 1 << 127), -1.0);
        assert_eq!(f128_to_f64(0), 0.0);
        assert_eq!(f128_to_f64(0x4000_9000_u128 << 96), 3.125);
        assert_eq!(f128_to_f64(0x7FFF_u128 << 112), f64::INFINITY);
        assert_eq!(f128_to_f64(0xFFFF_u128 << 112), f64::NEG_INFINITY);
        assert!(f128_to_f64((0x7FFF_u128 << 112) | 1).is_nan());

        let pi = 0x4000_921F_B544_42D1_8469_898C_C517_01B8_u128;
        assert_eq!(f128_to_f64(pi), std::f64::consts::PI);

        let subnormal = 1_u128 << 100;
        assert_eq!(f128_to_f64(subnormal), 0.0, "below the f64 range");
        let huge = 0x43FF_u128 << 112;
        assert_eq!(f128_to_f64(huge), f64::INFINITY, "2^1024 overflows");
        let max = 0x43FE_u128 << 112 | ((1u128 << 112) - 1);
        assert_eq!(f128_to_f64(max), f64::INFINITY, "rounds up past f64::MAX");
        let tiny = 0x3C01_u128 << 112;
        assert_eq!(f128_to_f64(tiny), f64::MIN_POSITIVE);
        let smaller = 0x3BCD_u128 << 112;
        assert_eq!(f128_to_f64(smaller), 5e-324, "lands in f64 subnormals");
    }

    #[test]
    fn the_lookahead_follows_the_widest_visible_column() {
        assert_eq!(visible(vec![Column::U16, Column::Hex]).lookahead(), 0);
        assert_eq!(interpretor().lookahead(), 1, "ascii reads one word ahead");
        assert_eq!(visible(vec![Column::U16, Column::F64]).lookahead(), 3);
        assert_eq!(visible(vec![Column::I128, Column::F64]).lookahead(), 7);

        let mut interpretor = visible(vec![Column::U16]);
        interpretor.toggle(Column::U64);
        assert_eq!(interpretor.lookahead(), 3);
        interpretor.toggle(Column::U64);
        assert_eq!(interpretor.lookahead(), 0);
    }

    #[test]
    fn following_fills_only_the_requested_slots() {
        let at = |address: u16| Some(address);
        assert_eq!(following(10, 0, at), [None; LOOKAHEAD]);
        assert_eq!(
            following(10, 2, at),
            [Some(11), Some(12), None, None, None, None, None]
        );
        assert_eq!(
            following(u16::MAX, 2, at),
            [Some(u16::MAX), Some(u16::MAX), None, None, None, None, None],
            "saturates at the top of the address space"
        );
        assert!(following(0, LOOKAHEAD, at).iter().all(Option::is_some));
    }

    #[test]
    fn ago_text() {
        assert_eq!(format_ago(chrono::Duration::seconds(0)), "now");
        assert_eq!(format_ago(chrono::Duration::seconds(59)), "59s ago");
        assert_eq!(format_ago(chrono::Duration::seconds(125)), "2m ago");
        assert_eq!(format_ago(chrono::Duration::seconds(3600)), ">1h ago");
    }

    #[test]
    fn every_ago_text_is_recognised_as_an_age() {
        for secs in [-5, 0, 1, 59, 60, 125, 3599, 3600, 90_000] {
            let text = format_ago(chrono::Duration::seconds(secs));
            assert!(is_age(&text), "{text}");
        }
        for text in ["12:00:00.000", "-", "", "s ago", "5h ago", "now!"] {
            assert!(!is_age(text), "{text:?}");
        }
    }
}
