use serde::{Deserialize, Serialize};

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RegisterType {
    #[default]
    Holding,
    Input,
    Coil,
    Discrete,
}

impl RegisterType {
    pub const ALL: [Self; 4] = [
        Self::Holding,
        Self::Input,
        Self::Coil,
        Self::Discrete,
    ];

    pub const fn toggle(&mut self) {
        *self = match self {
            Self::Holding => Self::Input,
            Self::Input => Self::Coil,
            Self::Coil => Self::Discrete,
            Self::Discrete => Self::Holding,
        };
    }

    pub const fn is_bit(self) -> bool {
        matches!(self, Self::Coil | Self::Discrete)
    }

    pub const fn is_writable(self) -> bool {
        matches!(self, Self::Holding | Self::Coil)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Holding => "Holding",
            Self::Input => "Input",
            Self::Coil => "Coil",
            Self::Discrete => "Discrete",
        }
    }

    pub const fn marker(self) -> &'static str {
        match self {
            Self::Holding => "H",
            Self::Input => "I",
            Self::Coil => "C",
            Self::Discrete => "D",
        }
    }
}

pub type RegisterCell = (RegisterType, u16);
pub type RegisterCellValue = (RegisterCell, u16);
