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
            let _ = write!(header, "{0: <12} ", "time");
        }
        if self.config.ago {
            let _ = write!(header, "{0: <9} ", "ago");
        }
        let _ = write!(header, "{0: >5}: ", "index");
        if self.config.u16 {
            let _ = write!(header, "{0: <5} ", "u16");
        }
        if self.config.i16 {
            let _ = write!(header, "{0: <6} ", "i16");
        }
        if self.config.u8s {
            let _ = write!(header, "{0: <8} ", "u8s");
        }
        if self.config.i8s {
            let _ = write!(header, "{0: <9} ", "i8s");
        }
        if self.config.hex {
            let _ = write!(header, "{0: <4} ", "hex");
        }
        if self.config.hex32 {
            let _ = write!(header, "{0: <9} ", "hex32");
        }
        if self.config.f16 {
            let _ = write!(header, "{0: <10} ", "f16");
        }
        if self.config.bcd {
            let _ = write!(header, "{0: <6} ", "bcd");
        }
        if self.config.bcd32 {
            let _ = write!(header, "{0: <10} ", "bcd32");
        }
        if self.config.u32 {
            let _ = write!(header, "{0: <10} ", "u32");
        }
        if self.config.i32 {
            let _ = write!(header, "{0: <11} ", "i32");
        }
        if self.config.u32_m10k {
            let _ = write!(header, "{0: <11} ", "u32m10k");
        }
        if self.config.i32_m10k {
            let _ = write!(header, "{0: <14} ", "i32m10k");
        }
        if self.config.u64 {
            let _ = write!(header, "{0: <20} ", "u64");
        }
        if self.config.i64 {
            let _ = write!(header, "{0: <21} ", "i64");
        }
        if self.config.f32 {
            let _ = write!(header, "{0: <10} ", "f32");
        }
        if self.config.f64 {
            let _ = write!(header, "{0: <12} ", "f64");
        }
        if self.config.ascii {
            let _ = write!(header, "{0: <5} ", "ascii");
        }
        if self.config.bits {
            let _ = write!(header, "{0: <19} ", "bits");
        }
        if self.config.custom {
            let _ = write!(header, "{0: <18} ", "custom");
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
        let mut width = 7; // "index" (>5) + ": "
        if self.config.time {
            width += 13; // "{:<12} "
        }
        if self.config.ago {
            width += 10; // "{:<9} "
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
            let _ = write!(row, "{dash: <12} ");
        }
        if self.config.ago {
            let _ = write!(row, "{dash: <9} ");
        }
        if self.config.index_hex {
            let _ = write!(row, "{index: >5X}: ");
        } else {
            let _ = write!(row, "{index: >5}: ");
        }
        if self.config.u16 {
            let _ = write!(row, "{dash: <5} ");
        }
        if self.config.i16 {
            let _ = write!(row, "{dash: <6} ");
        }
        if self.config.u8s {
            let _ = write!(row, "{dash: <8} ");
        }
        if self.config.i8s {
            let _ = write!(row, "{dash: <9} ");
        }
        if self.config.hex {
            let _ = write!(row, "{dash: <4} ");
        }
        if self.config.hex32 {
            let _ = write!(row, "{dash: <9} ");
        }
        if self.config.f16 {
            let _ = write!(row, "{dash: <10} ");
        }
        if self.config.bcd {
            let _ = write!(row, "{dash: <6} ");
        }
        if self.config.bcd32 {
            let _ = write!(row, "{dash: <10} ");
        }
        if self.config.u32 {
            let _ = write!(row, "{dash: <10} ");
        }
        if self.config.i32 {
            let _ = write!(row, "{dash: <11} ");
        }
        if self.config.u32_m10k {
            let _ = write!(row, "{dash: <11} ");
        }
        if self.config.i32_m10k {
            let _ = write!(row, "{dash: <14} ");
        }
        if self.config.u64 {
            let _ = write!(row, "{dash: <20} ");
        }
        if self.config.i64 {
            let _ = write!(row, "{dash: <21} ");
        }
        if self.config.f32 {
            let _ = write!(row, "{dash: <10} ");
        }
        if self.config.f64 {
            let _ = write!(row, "{dash: <12} ");
        }
        if self.config.ascii {
            let _ = write!(row, "{dash: <5} ");
        }
        if self.config.bits {
            let _ = write!(row, "{dash: <19} ");
        }
        if self.config.custom {
            let _ = write!(row, "{dash: <18} ");
        }

        if self.config.label {
            match label {
                Some(text) => row.push_str(text),
                None => row.push_str(""),
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
        let byte = value;
        let [next1, next2, next3] = next;
        let mut row = String::new();
        if self.config.time {
            let formatted = read_at.format("%H:%M:%S.%3f").to_string();
            let _ = write!(row, "{formatted: <12} ");
        }
        if self.config.ago {
            let ago = format_ago(now.signed_duration_since(read_at));
            let _ = write!(row, "{ago: <9} ");
        }
        if self.config.index_hex {
            let _ = write!(row, "{address: >5X}: ");
        } else {
            let _ = write!(row, "{address: >5}: ");
        }
        if self.config.u16 {
            let _ = write!(row, "{byte: <5} ");
        }
        if self.config.i16 {
            let _ = write!(row, "{: <6} ", byte as i16);
        }
        if self.config.u8s {
            let high = (byte >> 8) as u8;
            let low = (byte & 0xFF) as u8;
            let _ = write!(row, "{: <8} ", format!("{high}/{low}"));
        }
        if self.config.i8s {
            let high = (byte >> 8) as u8 as i8;
            let low = (byte & 0xFF) as u8 as i8;
            let _ = write!(row, "{: <9} ", format!("{high}/{low}"));
        }

        let word = self.word_order.make_word(byte, next1.unwrap_or_default());
        let second_word = self
            .word_order
            .make_word(next2.unwrap_or_default(), next3.unwrap_or_default());
        let dword = self.word_order.make_dword(word, second_word);
        if self.config.hex {
            let _ = write!(row, "{byte:<04X} ");
        }
        if self.config.hex32 {
            if next1.is_none() {
                let _ = write!(row, "{: <9} ", "-");
            } else {
                let s = format!("{word:08X}");
                let _ = write!(row, "{s: <9} ");
            }
        }
        if self.config.f16 {
            let x = f16_to_f32(byte);
            let mut s = format!("{x}");
            if s.len() > 10 {
                s = format!("{x:.3e}");
            }
            let _ = write!(row, "{s: <10} ");
        }
        if self.config.bcd {
            let s = bcd_to_decimal(byte).map_or_else(|| "--".to_string(), |n| n.to_string());
            let _ = write!(row, "{s: <6} ");
        }
        if self.config.bcd32 {
            if next1.is_none() {
                let _ = write!(row, "{: <10} ", "-");
            } else {
                let s =
                    bcd_to_decimal_u32(word).map_or_else(|| "--".to_string(), |n| n.to_string());
                let _ = write!(row, "{s: <10} ");
            }
        }
        if self.config.u32 {
            if next1.is_none() {
                let _ = write!(row, "{: <10} ", "-");
            } else {
                let _ = write!(row, "{word: <10} ");
            }
        }
        if self.config.i32 {
            if next1.is_none() {
                let _ = write!(row, "{: <11} ", "-");
            } else {
                let _ = write!(row, "{: <11} ", word as i32);
            }
        }
        if self.config.u32_m10k {
            if next1.is_none() {
                let _ = write!(row, "{: <11} ", "-");
            } else {
                let s =
                    m10k_to_u32(word).map_or_else(|| "--".to_string(), |(h, l)| format!("{h}/{l}"));
                let _ = write!(row, "{s: <11} ");
            }
        }
        if self.config.i32_m10k {
            if next1.is_none() {
                let _ = write!(row, "{: <14} ", "-");
            } else {
                let s =
                    m10k_to_i32(word).map_or_else(|| "--".to_string(), |(h, l)| format!("{h}/{l}"));
                let _ = write!(row, "{s: <14} ");
            }
        }
        if self.config.u64 {
            if next1.is_none() || next2.is_none() || next3.is_none() {
                let _ = write!(row, "{: <20} ", "-");
            } else {
                let _ = write!(row, "{dword: <20} ");
            }
        }
        if self.config.i64 {
            if next1.is_none() || next2.is_none() || next3.is_none() {
                let _ = write!(row, "{: <21} ", "-");
            } else {
                let _ = write!(row, "{: <21} ", dword as i64);
            }
        }
        if self.config.f32 {
            if next1.is_none() {
                let _ = write!(row, "{: <10} ", "-");
            } else {
                let x = f32::from_bits(word);
                let mut s = format!("{x}");
                if s.len() > 10 {
                    s = format!("{x:.3e}");
                }
                let _ = write!(row, "{s: <10} ");
            }
        }
        if self.config.f64 {
            if next1.is_none() || next2.is_none() || next3.is_none() {
                let _ = write!(row, "{: <12} ", "-");
            } else {
                let x = f64::from_bits(dword);
                let mut s = format!("{x}");
                if s.len() > 12 {
                    s = format!("{x:.3e}");
                }
                let _ = write!(row, "{s: <12} ");
            }
        }
        if self.config.ascii {
            let s: String = [byte, next1.unwrap_or_default()]
                .iter()
                .flat_map(|n| [(n >> 8) as u8, (n & 0xFF) as u8])
                .map(|b| {
                    let c = b as char;
                    if c.is_ascii_graphic() {
                        c
                    } else {
                        '·'
                    }
                })
                .collect();

            let _ = write!(row, "{s:<5} ");
        }
        if self.config.bits {
            let b = format!("{byte:016b}");
            let grouped = format!("{} {} {} {}", &b[0..4], &b[4..8], &b[8..12], &b[12..16]);
            let _ = write!(row, "{grouped: <19} ");
        }
        if self.config.custom {
            let s = custom.unwrap_or("--");
            let _ = write!(row, "{s: <18} ");
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
        let [next1, next2, next3] = next;
        let word = self.word_order.make_word(value, next1.unwrap_or_default());
        let second_word = self
            .word_order
            .make_word(next2.unwrap_or_default(), next3.unwrap_or_default());
        let dword = self.word_order.make_dword(word, second_word);
        let two = next1.is_some();
        let four = two && next2.is_some() && next3.is_some();
        let two_or = |s: String| if two { s } else { "-".to_string() };
        let four_or = |s: String| if four { s } else { "-".to_string() };

        let high = (value >> 8) as u8;
        let low = (value & 0xFF) as u8;
        let b = format!("{value:016b}");
        let ascii: String = [value, next1.unwrap_or_default()]
            .iter()
            .flat_map(|n| [(n >> 8) as u8, (n & 0xFF) as u8])
            .map(|b| {
                let c = b as char;
                if c.is_ascii_graphic() {
                    c
                } else {
                    '·'
                }
            })
            .collect();

        vec![
            ("u16", value.to_string()),
            ("i16", (value as i16).to_string()),
            ("u8s", format!("{high}/{low}")),
            ("i8s", format!("{}/{}", high as i8, low as i8)),
            ("hex", format!("{value:04X}")),
            (
                "bits",
                format!("{} {} {} {}", &b[0..4], &b[4..8], &b[8..12], &b[12..16]),
            ),
            ("f16", float_repr(f16_to_f32(value))),
            (
                "bcd",
                bcd_to_decimal(value).map_or_else(|| "--".to_string(), |n| n.to_string()),
            ),
            ("hex32", two_or(format!("{word:08X}"))),
            ("u32", two_or(word.to_string())),
            ("i32", two_or((word as i32).to_string())),
            (
                "u32 m10k",
                two_or(
                    m10k_to_u32(word).map_or_else(|| "--".to_string(), |(h, l)| format!("{h}/{l}")),
                ),
            ),
            (
                "i32 m10k",
                two_or(
                    m10k_to_i32(word).map_or_else(|| "--".to_string(), |(h, l)| format!("{h}/{l}")),
                ),
            ),
            ("f32", two_or(float_repr(f32::from_bits(word)))),
            (
                "bcd32",
                two_or(
                    bcd_to_decimal_u32(word).map_or_else(|| "--".to_string(), |n| n.to_string()),
                ),
            ),
            ("u64", four_or(dword.to_string())),
            ("i64", four_or((dword as i64).to_string())),
            ("f64", four_or(float_repr(f64::from_bits(dword)))),
            ("ascii", ascii),
            ("custom", custom.unwrap_or("--").to_string()),
            ("label", label.unwrap_or("").to_string()),
        ]
    }
}

fn float_repr<T: std::fmt::Display + std::fmt::LowerExp>(x: T) -> String {
    let s = format!("{x}");
    if s.len() > 21 {
        format!("{x:.3e}")
    } else {
        s
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
