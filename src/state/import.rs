use crate::import::mbmap::{Firmware, Revision, Status};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImportSource {
    #[default]
    Mtui,
    Mbmap,
    HomeAssistant,
    SunSpec,
    Solarman,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportDevice {
    pub name: String,
    pub pins: usize,
    pub labels: usize,
    pub rules: usize,
    pub replaced: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct FirmwareInput {
    pub text: String,
    pub known: Firmware,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConfigImportAction {
    #[default]
    Registers,
    Load,
    Save,
}

impl ConfigImportAction {
    pub const ALL: [Self; 3] = [Self::Registers, Self::Load, Self::Save];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Registers => "Import registers only",
            Self::Load => "Load entire config",
            Self::Save => "Save as",
        }
    }

    pub const fn confirm(self) -> &'static str {
        match self {
            Self::Registers => "Import",
            Self::Load => "Load",
            Self::Save => "Save",
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ConfigImport {
    pub name: String,
    pub action: ConfigImportAction,
    pub file_name: String,
    pub folder: String,
    pub unsaved: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportParams {
    pub source: ImportSource,
    pub devices: Vec<ImportDevice>,
    pub selected: u16,
    pub firmware: Option<FirmwareInput>,
    pub revision: Option<Revision>,
    pub status: Option<Status>,
    pub config: Option<ConfigImport>,
}

impl ImportParams {
    pub fn device(&self) -> Option<&ImportDevice> {
        self.devices.get(usize::from(self.selected))
    }

    pub fn action(&self) -> ConfigImportAction {
        self.config
            .as_ref()
            .map_or(ConfigImportAction::Registers, |c| c.action)
    }
}
