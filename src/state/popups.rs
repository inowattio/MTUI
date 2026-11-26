use super::{StatusMessage, scroll_window};
use crate::register::{RegisterCell, RegisterType};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LabelParams {
    pub position: u16,
    pub register_type: RegisterType,
    pub text: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DumpParams {
    pub result: Option<StatusMessage>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RawField {
    #[default]
    Code,
    Data,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RawParams {
    pub code: String,
    pub data: String,
    pub field: RawField,
    pub response: Option<String>,
    pub status: Option<StatusMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub cell: RegisterCell,
    pub text: String,
    pub labeled: bool,
}

impl SearchMatch {
    pub const fn label(cell: RegisterCell, text: String) -> Self {
        Self {
            cell,
            text,
            labeled: true,
        }
    }

    pub const fn hint(cell: RegisterCell, text: String) -> Self {
        Self {
            cell,
            text,
            labeled: false,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SearchParams {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub selected: u16,
    pub top: u16,
}

impl SearchParams {
    pub fn scroll(&mut self, rows: u16) {
        let len = self.matches.len() as u16;
        scroll_window(&mut self.selected, &mut self.top, rows, len);
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct HelpParams {
    pub query: String,
    pub selected: u16,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ColumnsParams {
    pub query: String,
    pub selected: u16,
}

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum InspectMode {
        #[default]
        Now,
        Min,
        Max,
        Avg,
    }
}

impl InspectMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::Min => "min",
            Self::Max => "max",
            Self::Avg => "avg",
        }
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum StatsTab {
        #[default]
        All,
        Reads,
        Writes,
        Others,
    }
}

impl StatsTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Reads => "Reads",
            Self::Writes => "Writes",
            Self::Others => "Others",
        }
    }
}
