use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, Serialize, Deserialize, Default)]
pub enum RegisterType {
    #[default]
    Holding,
    Input,
}

impl RegisterType {
    pub fn toggle(&mut self) {
        if *self == RegisterType::Holding {
            *self = RegisterType::Input;
        } else {
            *self = RegisterType::Holding;
        }
    }
}

pub type RegisterCell = (RegisterType, u16);
pub type RegisterCellValue = (RegisterCell, u16);
