use crate::config::Registers;

pub mod home_assistant;
pub mod mbmap;

pub struct Device {
    pub name: String,
    pub registers: Registers,
    pub replaced: usize,
}

impl From<Registers> for Device {
    fn from(registers: Registers) -> Self {
        Self {
            name: String::new(),
            registers,
            replaced: 0,
        }
    }
}
