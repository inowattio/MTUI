use crate::app::Interpretations;

#[derive(Debug)]
pub struct Interpretator {
    interpretation: Interpretations,
    header: String,
}

impl Interpretator {
    pub fn new(interpretation: Interpretations) -> Self {
        let mut header = format!("{0: >5}: {1: <5} {2: <5} ", "index", "u16", "i16");

        if interpretation.u32 {
            header.push_str(&format!("{0: <10} ", "u32"))
        }
        if interpretation.i32 {
            header.push_str(&format!("{0: <10} ", "i32"))
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

        header.push('\n');
        Self {
            interpretation,
            header,
        }
    }

    pub fn run(&self, data: Vec<u16>, index: usize) -> String {
        let mut rendered_data = self.header.clone();

        for i in 0..data.len() {
            let byte = *data.get(i).unwrap_or(&0);
            let next = *data.get(i + 1).unwrap_or(&0);

            let mut row = format!("{0: >5}: {1: <5} {2: <5} ", index + i, byte, byte as i16);

            let word = (byte as u32) << 16 | (next as u32);
            if self.interpretation.u32 {
                row.push_str(&format!("{word: <10} "))
            }
            if self.interpretation.i32 {
                row.push_str(&format!("{: <10} ", word as i32))
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
                let s: String = [byte, next]
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

            rendered_data.push_str(&row);
            rendered_data.push('\n');
        }

        rendered_data
    }
}
