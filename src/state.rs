use crate::app::WriteType;
use crate::constants::keybind;
use crate::register::RegisterType;
use std::time::Instant;

#[derive(Debug, Default, PartialEq)]
pub struct WriteParams {
    pub position: u16,
    pub result: Option<String>,
    pub value: Option<i32>,
    pub write_type: WriteType,
}

#[derive(Debug, PartialEq, Default)]
pub struct DumpParams {
    pub started: bool,
    pub total_batches: Option<u16>,
    pub completed_batches: u16,
    pub start_position: u16,
    pub position: u16,
    pub header_written: bool,
    pub error: Option<String>,
    pub register_type: RegisterType,
}

#[derive(Debug, Default, PartialEq)]
pub struct JumpParams {
    pub from: u16,
    pub to: u16,
    pub register_type: RegisterType,
}

#[derive(Debug, PartialEq)]
pub struct ReadParams {
    pub position: u16,
    pub main_data: String,
    pub pinned_data: String,
    pub refresh_timer: Instant,
    pub register_type: RegisterType,
}

impl Default for ReadParams {
    fn default() -> Self {
        Self {
            position: 0,
            main_data: no_data_text(),
            pinned_data: "".to_string(),
            refresh_timer: Instant::now(),
            register_type: Default::default(),
        }
    }
}

pub fn no_data_text() -> String {
    format!("No data, press '{}' to refresh.", keybind::REFRESH)
}

#[derive(Debug, PartialEq)]
pub enum State {
    Read(ReadParams),
    Jump(JumpParams),
    Write(WriteParams),
    Help,
    Dump(DumpParams),
}

pub enum StateTransition {
    Read,
    Jump,
    Write,
    Help,
    Dump,
}
