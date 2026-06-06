use crate::config::{Column, InterpretorConfig};
use crate::modbus::WordOrder;
use crate::register::RegisterCellValue;
use chrono::{DateTime, Local};

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
            header.push_str(&format!("{0: <12} ", "time"))
        }
        header.push_str(&format!("{0: >5}: ", "index"));
        if self.config.u16 {
            header.push_str(&format!("{0: <5} ", "u16"))
        }
        if self.config.i16 {
            header.push_str(&format!("{0: <6} ", "i16"))
        }
        if self.config.hex {
            header.push_str(&format!("{0: <4} ", "hex"))
        }
        if self.config.u32 {
            header.push_str(&format!("{0: <10} ", "u32"))
        }
        if self.config.i32 {
            header.push_str(&format!("{0: <11} ", "i32"))
        }
        if self.config.u64 {
            header.push_str(&format!("{0: <20} ", "u64"))
        }
        if self.config.i64 {
            header.push_str(&format!("{0: <21} ", "i64"))
        }
        if self.config.f32 {
            header.push_str(&format!("{0: <10} ", "f32"))
        }
        if self.config.ascii {
            header.push_str(&format!("{0: <5} ", "ascii"))
        }
        if self.config.bits {
            header.push_str(&format!("{0: <8} ", "bits"))
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

    pub fn set_word_order(&mut self, word_order: WordOrder) {
        self.word_order = word_order;
    }

    pub fn header(&self) -> String {
        self.header.clone()
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
            row.push_str(&format!("{dash: <12} "));
        }
        if self.config.index_hex {
            row.push_str(&format!("{index: >5X}: "));
        } else {
            row.push_str(&format!("{index: >5}: "));
        }
        if self.config.u16 {
            row.push_str(&format!("{dash: <5} "));
        }
        if self.config.i16 {
            row.push_str(&format!("{dash: <6} "));
        }
        if self.config.hex {
            row.push_str(&format!("{dash: <4} "));
        }
        if self.config.u32 {
            row.push_str(&format!("{dash: <10} "));
        }
        if self.config.i32 {
            row.push_str(&format!("{dash: <11} "));
        }
        if self.config.u64 {
            row.push_str(&format!("{dash: <20} "));
        }
        if self.config.i64 {
            row.push_str(&format!("{dash: <21} "));
        }
        if self.config.f32 {
            row.push_str(&format!("{dash: <10} "));
        }
        if self.config.ascii {
            row.push_str(&format!("{dash: <5} "));
        }
        if self.config.bits {
            row.push_str(&format!("{dash: <8} "));
        }

        if self.config.label {
            match label {
                Some(text) => row.push_str(text),
                None => row.push_str(""),
            }
        }

        row
    }

    pub fn run(
        &self,
        data: Vec<RegisterCellValue>,
        index: u16,
        read_at: DateTime<Local>,
        label: impl Fn(RegisterCellValue) -> Option<String>,
    ) -> Vec<String> {
        let mut lines = Vec::with_capacity(data.len());

        for i in 0..data.len() {
            let current = data[i];
            let byte = current.1;
            let next_byte_1st = data.get(i + 1).map(|(_, v)| *v).unwrap_or(0);
            let next_byte_2nd = data.get(i + 2).map(|(_, v)| *v).unwrap_or(0);
            let next_byte_3rd = data.get(i + 3).map(|(_, v)| *v).unwrap_or(0);

            let mut row = String::new();
            if self.config.time {
                let formatted = read_at.format("%H:%M:%S:%3f").to_string();
                row.push_str(&format!("{formatted: <12} "));
            }
            let address = index + i as u16;
            if self.config.index_hex {
                row.push_str(&format!("{address: >5X}: "));
            } else {
                row.push_str(&format!("{address: >5}: "));
            }
            if self.config.u16 {
                row.push_str(&format!("{byte: <5} "))
            }
            if self.config.i16 {
                row.push_str(&format!("{: <6} ", byte as i16))
            }

            let word = self.word_order.make_word(byte, next_byte_1st);
            let second_word = self.word_order.make_word(next_byte_2nd, next_byte_3rd);
            let dword = self.word_order.make_dword(word, second_word);
            if self.config.hex {
                row.push_str(&format!("{byte:<04X} "))
            }
            if self.config.u32 {
                row.push_str(&format!("{word: <10} "))
            }
            if self.config.i32 {
                row.push_str(&format!("{: <11} ", word as i32))
            }
            if self.config.u64 {
                row.push_str(&format!("{dword: <20} "))
            }
            if self.config.i64 {
                row.push_str(&format!("{: <21} ", dword as i64))
            }
            if self.config.f32 {
                let x = f32::from_bits(word);
                let mut s = format!("{x}");

                let max_len = 10;
                if s.len() > max_len {
                    s.truncate(max_len);
                }
                row.push_str(&format!("{s: <10} "))
            }
            if self.config.ascii {
                let s: String = [byte, next_byte_1st]
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

                row.push_str(&format!("{s:<5} "))
            }
            if self.config.bits {
                row.push_str(&format!("{byte:<08b} "))
            }

            if self.config.label {
                if let Some(t) = label(current) {
                    row.push_str(&t);
                }
            }

            lines.push(row);
        }

        lines
    }
}
