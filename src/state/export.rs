#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Registers,
    Config,
    HomeAssistant,
    Mbmap,
}

impl ExportFormat {
    pub const ALL: [Self; 4] = [
        Self::Registers,
        Self::Config,
        Self::HomeAssistant,
        Self::Mbmap,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Registers => "Registers",
            Self::Config => "Whole config",
            Self::HomeAssistant => "Home Assistant YAML",
            Self::Mbmap => "mbmap",
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Registers | Self::Config => "json",
            Self::HomeAssistant => "yaml",
            Self::Mbmap => "mbmap.json",
        }
    }

    pub const fn file_tag(self) -> &'static str {
        match self {
            Self::Registers => "registers",
            Self::Config => "config",
            Self::HomeAssistant => "home_assistant",
            Self::Mbmap => "",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExportTarget {
    #[default]
    Clipboard,
    File,
}

impl ExportTarget {
    pub const ALL: [Self; 2] = [Self::Clipboard, Self::File];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Clipboard => "Clipboard",
            Self::File => "File",
        }
    }
}
