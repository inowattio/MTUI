use crate::app::Interpretations;

#[derive(Debug)]
pub struct Interpretator {
    interpretation: Interpretations,
    header: String,
}

impl Interpretator {
    pub fn new(interpretation: Interpretations) -> Self {
        let mut header = format!("{0: >5}: {1: <5} ", "index", "u16");
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
            header.push_str(&format!("{0: <7} ", "_ascii_"))
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

            let mut row = format!("{0: >5}: {byte: <5} ", index + i);

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
                let mut s = format!("_{}_", String::from_utf8_lossy(&[byte as u8, next as u8]));
                let max_len = 7;
                if s.len() > max_len {
                    s.truncate(max_len);
                }
                row.push_str(&format!("{s:<7} "))
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
