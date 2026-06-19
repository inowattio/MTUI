use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, Serialize, Deserialize, Default)]
pub enum RegisterType {
    #[default]
    Holding,
    Input,
    Coil,
    Discrete,
}

impl RegisterType {
    pub const ALL: [RegisterType; 4] = [
        RegisterType::Holding,
        RegisterType::Input,
        RegisterType::Coil,
        RegisterType::Discrete,
    ];

    pub fn toggle(&mut self) {
        *self = match self {
            RegisterType::Holding => RegisterType::Input,
            RegisterType::Input => RegisterType::Coil,
            RegisterType::Coil => RegisterType::Discrete,
            RegisterType::Discrete => RegisterType::Holding,
        };
    }

    pub fn is_bit(self) -> bool {
        matches!(self, RegisterType::Coil | RegisterType::Discrete)
    }

    pub fn is_writable(self) -> bool {
        matches!(self, RegisterType::Holding | RegisterType::Coil)
    }

    pub fn marker(self) -> &'static str {
        match self {
            RegisterType::Holding => "H",
            RegisterType::Input => "I",
            RegisterType::Coil => "C",
            RegisterType::Discrete => "D",
        }
    }

    pub fn access(self) -> &'static str {
        if self.is_writable() {
            "RW"
        } else {
            "RO"
        }
    }
}

pub type RegisterCell = (RegisterType, u16);
pub type RegisterCellValue = (RegisterCell, u16);
