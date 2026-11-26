use super::ReadParams;
use crate::snapshot::{Diff, Snapshot};
use crate::writes_log::WriteEntry;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LogMode {
    #[default]
    Events,
    Frames,
    Writes,
}

impl LogMode {
    pub const fn next(self) -> Self {
        match self {
            Self::Events => Self::Frames,
            Self::Frames => Self::Writes,
            Self::Writes => Self::Events,
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Events => "Logs",
            Self::Frames => "Frames",
            Self::Writes => "Writes",
        }
    }

    pub const fn noun(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Frames => "frames",
            Self::Writes => "writes",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct LogViewParams {
    pub top: u16,
    pub selected: u16,
    pub follow: bool,
    pub h_offset: u16,
    pub wrap: bool,
    pub mode: LogMode,
    pub writes: Vec<WriteEntry>,
    pub writes_path: String,
    pub previous: ReadParams,
}

#[derive(Debug, PartialEq)]
pub struct DiffViewParams {
    pub snapshot: Snapshot,
    pub diff: Diff,
    pub top: usize,
    pub h_offset: u16,
    pub changed_only: bool,
    pub previous: ReadParams,
}
