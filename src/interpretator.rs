use crate::config::InterpretorConfig;
use crate::modbus::WordOrder;
use crate::register::RegisterCellValue;

#[derive(Debug, Clone)]
pub struct Interpretor {
    config: InterpretorConfig,
    word_order: WordOrder,
    header: String,
}

impl Interpretor {
    pub fn new(interpretation: InterpretorConfig, word_order: WordOrder) -> Self {
        let mut header = format!("{0: >5}: {1: <5} {2: <6} ", "index", "u16", "i16");

        if interpretation.hex {
            header.push_str(&format!("{0: <4} ", "hex"))
        }
        if interpretation.u32 {
            header.push_str(&format!("{0: <10} ", "u32"))
        }
        if interpretation.i32 {
            header.push_str(&format!("{0: <11} ", "i32"))
        }
        if interpretation.u64 {
            header.push_str(&format!("{0: <20} ", "u64"))
        }
        if interpretation.i64 {
            header.push_str(&format!("{0: <21} ", "i64"))
        }
        if interpretation.f32 {
            header.push_str(&format!("{0: <10} ", "f32"))
        }
        if interpretation.ascii {
            header.push_str(&format!("{0: <5} ", "ascii"))
        }
        if interpretation.bits {
            header.push_str(&format!("{0: <8} ", "bits"))
        }
        if interpretation.label {
            header.push_str("label");
        }

        Self {
            config: interpretation,
            word_order,
            header,
        }
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

    pub fn run(
        &self,
        data: Vec<RegisterCellValue>,
        index: u16,
        label: impl Fn(RegisterCellValue) -> Option<String>,
    ) -> Vec<String> {
        let mut lines = Vec::with_capacity(data.len());

        for i in 0..data.len() {
            let current = data[i];
            let byte = current.1;
            let next_byte_1st = data.get(i + 1).map(|(_, v)| *v).unwrap_or(0);
            let next_byte_2nd = data.get(i + 2).map(|(_, v)| *v).unwrap_or(0);
            let next_byte_3rd = data.get(i + 3).map(|(_, v)| *v).unwrap_or(0);

            let mut row = format!(
                "{0: >5}: {1: <5} {2: <6} ",
                index + i as u16,
                byte,
                byte as i16
            );

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
