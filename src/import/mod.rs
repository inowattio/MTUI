use crate::config::Registers;

pub mod catalog;
pub mod home_assistant;
pub mod mbmap;
pub mod mbmap_index;
pub mod solarman;
pub mod sunspec;

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
