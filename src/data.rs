use std::fs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Register{
    pub address: u16,
    pub name: String
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Data {
    pub input: Vec<Register>,
    pub holding: Vec<Register>,
}

impl Data {
    pub fn from_json_file(name: &str) -> Self {
        let data = fs::read_to_string(name).unwrap();

        serde_json::from_str(&data).unwrap()
    }
}
