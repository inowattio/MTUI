use crate::app::{Interpretations, RegisterCellValue};

#[derive(Debug)]
pub struct Interpretator {
    interpretation: Interpretations,
    header: String,
}

impl Interpretator {
    pub fn new(interpretation: Interpretations) -> Self {
        let mut header = format!("{0: >5}: {1: <5} {2: <6} ", "index", "u16", "i16");

        if interpretation.u32 {
            header.push_str(&format!("{0: <10} ", "u32"))
        }
        if interpretation.i32 {
            header.push_str(&format!("{0: <11} ", "i32"))
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

        Self {
            interpretation,
            header,
        }
    }

    pub fn header(&self) -> String {
        self.header.clone()
    }

    pub fn run(&self, data: Vec<RegisterCellValue>, index: u16, additional: impl Fn(RegisterCellValue) -> Option<String>) -> Vec<String> {
        let mut lines = Vec::with_capacity(data.len());

        for i in 0..data.len() {
            let current = data[i];
            let byte = current.1;
            let next_byte = data.get(i + 1).map(|(_, v)| *v).unwrap_or(0);

            let mut row = format!("{0: >5}: {1: <5} {2: <6} ", index + i as u16, byte, byte as i16);

            let word = (byte as u32) << 16 | (next_byte as u32);
            if self.interpretation.u32 {
                row.push_str(&format!("{word: <10} "))
            }
            if self.interpretation.i32 {
                row.push_str(&format!("{: <11} ", word as i32))
            }
            if self.interpretation.f32 {
                let x = f32::from_bits(word);
                let mut s = format!("{x}");

                let max_len = 10;
                if s.len() > max_len {
                    s.truncate(max_len);
                }
                row.push_str(&format!("{s: <10} "))
            }
            if self.interpretation.ascii {
                let s: String = [byte, next_byte]
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
            if self.interpretation.bits {
                row.push_str(&format!("{byte:<08b} "))
            }
            
            if let Some(t) = additional(current) {
                row.push_str(&format!("{t} "));
            }

            lines.push(row);
        }

        lines
    }
}
