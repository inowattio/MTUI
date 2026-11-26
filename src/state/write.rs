use super::StatusMessage;
use crate::app::WriteType;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriteFunc {
    #[default]
    Single,
    Multiple,
    ReadWrite,
}

impl WriteFunc {
    pub const fn code(self, write_type: WriteType) -> u8 {
        match (write_type, self) {
            (WriteType::Coil, _) => 0x05,
            (_, Self::Single) => 0x06,
            (_, Self::Multiple) => 0x10,
            (_, Self::ReadWrite) => 0x17,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Single => "Single register (0x06)",
            Self::Multiple => "Multiple registers (0x10)",
            Self::ReadWrite => "Read/write multiple (0x17)",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<StatusMessage>,
    pub value: Option<i64>,
    pub write_type: WriteType,
    pub bit_cursor: u16,
    pub func: WriteFunc,
    pub raw: bool,
}

impl WriteParams {
    pub fn cycle_mode(&mut self) {
        use WriteFunc::*;
        (self.write_type, self.func) = match (self.write_type, self.func) {
            (WriteType::Word, Single) => (WriteType::Word, Multiple),
            (WriteType::Word, Multiple) => (WriteType::Word, ReadWrite),
            (WriteType::Word, ReadWrite) => (WriteType::DWord, Multiple),
            (WriteType::DWord, ReadWrite) => (WriteType::Word, Single),
            (WriteType::DWord, _) => (WriteType::DWord, ReadWrite),
            (WriteType::Coil, func) => (WriteType::Coil, func),
        };
        self.bit_cursor = self.bit_cursor.min(self.write_type.bits() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::{WriteFunc, WriteParams};
    use crate::app::WriteType;

    #[test]
    fn cycling_walks_every_word_and_dword_function_then_wraps() {
        let mut w = WriteParams {
            bit_cursor: 31,
            ..WriteParams::default()
        };
        let mut seen = vec![(w.write_type, w.func)];
        for _ in 0..5 {
            w.cycle_mode();
            seen.push((w.write_type, w.func));
        }
        assert_eq!(
            seen,
            [
                (WriteType::Word, WriteFunc::Single),
                (WriteType::Word, WriteFunc::Multiple),
                (WriteType::Word, WriteFunc::ReadWrite),
                (WriteType::DWord, WriteFunc::Multiple),
                (WriteType::DWord, WriteFunc::ReadWrite),
                (WriteType::Word, WriteFunc::Single),
            ]
        );
    }

    #[test]
    fn cycling_keeps_the_bit_cursor_inside_the_new_width() {
        let mut w = WriteParams {
            write_type: WriteType::DWord,
            func: WriteFunc::ReadWrite,
            bit_cursor: 31,
            ..WriteParams::default()
        };
        w.cycle_mode();
        assert_eq!(w.write_type, WriteType::Word);
        assert_eq!(w.bit_cursor, 15);
    }

    #[test]
    fn coils_never_change_mode() {
        let mut w = WriteParams {
            write_type: WriteType::Coil,
            ..WriteParams::default()
        };
        w.cycle_mode();
        assert_eq!((w.write_type, w.func), (WriteType::Coil, WriteFunc::Single));
    }
}
