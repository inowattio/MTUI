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

#[derive(Debug)]
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
    next: [Option<u16>; 3],
    custom: Option<&'a str>,
    time: &'a str,
    elapsed: Option<chrono::Duration>,
    label: Option<&'a str>,
}

impl<'a> RowCtx<'a> {
    fn new(config: &InterpretorConfig, order: WordOrder, data: RowData<'a>) -> Self {
        let [b, c, d] = data.next.map(Option::unwrap_or_default);
        Self {
            value: data.value,
            next: data.next,
            word: order.make_word(data.value, b),
            dword: order.make_dword([data.value, b, c, d]),
            custom: data.custom.unwrap_or(NO_VALUE),
            time: data.time,
            elapsed: data.elapsed,
            label: data.label.unwrap_or(""),
            address: data.address,
            time_mode: config.time_mode,
            address_mode: config.address_mode,
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
    ColumnSpec { column: Column::Address, width: ADDRESS_W, render: |c, w, o| address_cell(c.address, c.address_mode, w, o) },
    ColumnSpec { column: Column::Time,    width: TIME_W, render: |c, _, o| time_cell(c, o) },
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
    ColumnSpec { column: Column::Custom,  width: CONFIGURED, render: |c, w, o| clipped_cell(c.custom, w, o) },
    ColumnSpec { column: Column::Label,   width: CONFIGURED, render: |c, w, o| clipped_cell(c.label, w, o) },
];

impl Column {
    fn is_meta(self) -> bool {
        matches!(self, Column::Address | Column::Time | Column::Label)
    }

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
            order: Vec::new(),
            enabled: Vec::new(),
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
        let Some(from) = self.order.iter().position(|&c| c == column) else {
            return false;
        };
        let to = if right {
            from + 1
        } else {
            from.wrapping_sub(1)
        };
        if to >= self.order.len() {
            return false;
        }
        let mut order = self.order.clone();
        order.swap(from, to);
        self.config.order = order;
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
        let configured = self.config.order.iter().copied();
        let built_in = COLUMNS.iter().map(|spec| spec.column);
        for column in configured.chain(built_in) {
            if !self.order.contains(&column) {
                self.order.push(column);
            }
        }
        self.enabled = self
            .order
            .iter()
            .filter_map(|&column| COLUMNS.iter().find(|spec| spec.column == column))
            .filter(|spec| self.config.get(spec.column))
            .map(|spec| EnabledColumn {
                spec,
                width: self.effective_width(spec),
            })
            .collect();
        self.rebuild_header();
    }

    pub fn label_width(&self) -> u16 {
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

    pub fn custom_width(&self) -> u16 {
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
        match self.enabled.first() {
            Some(col) if col.spec.column == Column::Address => (col.width + 1) as u16,
            _ => 0,
        }
    }

    pub fn time_mode(&self) -> TimeMode {
        self.config.time_mode
    }

    pub fn address_mode(&self) -> AddressMode {
        self.config.address_mode
    }

    pub fn set_time_mode(&mut self, mode: TimeMode) {
        self.config.time_mode = mode;
    }

    pub fn set_address_mode(&mut self, mode: AddressMode) {
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
                next: [None; 3],
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
        next: [Option<u16>; 3],
        time_text: &str,
        elapsed: chrono::Duration,
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
                elapsed: Some(elapsed),
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
        next: [Option<u16>; 3],
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

fn time_cell(ctx: &RowCtx, out: &mut String) {
    match (ctx.time_mode, ctx.elapsed) {
        (TimeMode::ReadAt, _) => out.push_str(ctx.time),
        (TimeMode::Ago, Some(elapsed)) => write_ago(out, elapsed),
        (TimeMode::Ago, None) => out.push_str(NO_VALUE),
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

    fn ordered(order: Vec<Column>) -> Interpretor {
        let config = InterpretorConfig {
            bits: false,
            ascii: false,
            custom: false,
            time: false,
            label: false,
            order,
            ..InterpretorConfig::default()
        };
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
    fn the_configured_order_leads_and_the_rest_follows() {
        let interpretor = ordered(vec![Column::Hex, Column::I16]);
        assert_eq!(
            segment_names(&interpretor),
            ["hex", "i16", "address", "u16"]
        );
    }

    #[test]
    fn an_empty_order_keeps_the_built_in_one() {
        let interpretor = ordered(Vec::new());
        assert_eq!(
            segment_names(&interpretor),
            ["address", "u16", "i16", "hex"]
        );
    }

    #[test]
    fn duplicates_in_the_order_are_ignored() {
        let interpretor = ordered(vec![Column::Hex, Column::Hex, Column::I16]);
        assert_eq!(
            segment_names(&interpretor),
            ["hex", "i16", "address", "u16"]
        );
    }

    #[test]
    fn moving_a_column_stops_at_the_edges() {
        let mut interpretor = ordered(Vec::new());
        let first = interpretor.ordered_columns()[0];
        assert_eq!(
            first,
            Column::Address,
            "the address leads the built-in order"
        );

        assert!(!interpretor.move_column(first, false), "already leftmost");
        assert!(
            !interpretor.move_column(Column::Label, true),
            "already last"
        );

        assert!(interpretor.move_column(first, true));
        assert_eq!(interpretor.ordered_columns()[1], first);
    }

    #[test]
    fn address_time_and_label_can_be_ordered_like_any_column() {
        let config = InterpretorConfig {
            bits: false,
            ascii: false,
            custom: false,
            time: true,
            label: true,
            order: vec![Column::Label, Column::Hex, Column::Time],
            ..InterpretorConfig::default()
        };
        let interpretor = Interpretor::new(config, WordOrder::ABCD);
        assert_eq!(
            segment_names(&interpretor),
            ["label", "hex", "time", "address", "u16", "i16"]
        );
    }

    #[test]
    fn unknown_column_keys_are_dropped_when_loading() {
        let config: InterpretorConfig =
            serde_json::from_str(r#"{"order":["hex","not_a_column","i16"]}"#).expect("loads");
        assert_eq!(config.order, vec![Column::Hex, Column::I16]);
    }

    #[test]
    fn row_segments_line_up_with_the_header() {
        for (address, time, label) in [
            (true, true, true),
            (false, false, false),
            (true, false, true),
        ] {
            let config = InterpretorConfig {
                address,
                time,
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
            ..InterpretorConfig::default()
        };
        Interpretor::new(config, WordOrder::ABCD)
    }

    fn row_of(interpretor: &Interpretor) -> String {
        interpretor.format_row(
            5,
            1,
            [None; 3],
            "12:34:56.789",
            chrono::Duration::seconds(3),
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
                [None; 3],
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
            [None; 3],
            "12:34:56.789",
            chrono::Duration::seconds(3),
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
            [None; 3],
            "12:34:56.789",
            chrono::Duration::seconds(3),
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
    fn ago_text() {
        assert_eq!(format_ago(chrono::Duration::seconds(0)), "now");
        assert_eq!(format_ago(chrono::Duration::seconds(59)), "59s ago");
        assert_eq!(format_ago(chrono::Duration::seconds(125)), "2m ago");
        assert_eq!(format_ago(chrono::Duration::seconds(3600)), ">1h ago");
    }
}
