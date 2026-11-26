use super::{ExportFormat, ExportTarget, LibrarySource, ReadPanel, ReadParams, StatusMessage};
use crate::num_ops::wrap_index;
use crate::register::RegisterType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Number,
    Text,
    Toggle,
    Action,
    Color,
    CycleType(RegisterType),
    CyclePanel(ReadPanel),
}

macro_rules! settings_fields {
    (
        $( $category:ident {
            $( [ $( $field:ident : $kind:expr => $label:literal, $description:literal ),+ $(,)? ] ),*
            $(,)?
        } )+
    ) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SettingsField { $( $( $( $field, )+ )* )+ }

        impl SettingsField {
            pub const fn label(self) -> &'static str {
                match self { $( $( $( SettingsField::$field => $label, )+ )* )+ }
            }

            pub const fn description(self) -> &'static str {
                match self { $( $( $( SettingsField::$field => $description, )+ )* )+ }
            }

            pub const fn kind(self) -> FieldKind {
                use FieldKind::*;
                match self { $( $( $( SettingsField::$field => $kind, )+ )* )+ }
            }
        }

        impl SettingsCategory {
            pub const fn groups(self) -> &'static [&'static [SettingsField]] {
                match self {
                    $( SettingsCategory::$category => &[ $( &[ $( SettingsField::$field ),+ ] ),* ], )+
                }
            }
        }
    };
}

settings_fields! {
    Data {
        [
            BatchSize: Number => "Batch size", "How many registers each read request fetches around the cursor",
            BatchAnchor: Toggle => "Batch anchor", "Where the cursor sits inside the read batch: start, middle or end",
            ReadFullCustoms: Toggle => "Read full custom values", "Also read every register a custom rule spans, even outside the batch",
            CustomBatchByRegisters: Toggle => "Custom batch by registers", "In the Custom panel, size the batch by registers instead of rules",
        ],
        [
            RefreshInterval: Number => "Auto-refresh (ms)", "Delay between automatic reads, 0 turns auto-refresh off",
            ReconnectOnTimeout: Toggle => "Reconnect on timeout", "Reconnect to the device after a read times out",
            RawExceptions: Toggle => "Raw exception codes", "Show every Modbus exception as its code, Exception 0x8F (143), instead of its name",
            ReadOnly: Toggle => "Read-only", "Refuse all writes from the UI and the API",
            WriteWaitsForRead: Toggle => "Write waits for read", "Queue a write until the read in progress finishes instead of refusing it",
            WriteLogEnabled: Toggle => "Log writes to file", "Append every write to a log file",
            WriteLogDirectory: Text => "Write log folder", "Folder for the write log, empty uses the config file's folder",
        ],
        [
            GraphHistory: Number => "Graph history", "Samples kept per register for the value graph",
            MatrixColumns: Number => "Matrix columns", "Registers per row in the Matrix panel, auto fits as many as the screen shows",
        ],
        [
            CycleHoldings: CycleType(RegisterType::Holding) => "Cycle holdings", "Include holding registers when cycling register types",
            CycleInputs: CycleType(RegisterType::Input) => "Cycle inputs", "Include input registers when cycling register types",
            CycleCoils: CycleType(RegisterType::Coil) => "Cycle coils", "Include coils when cycling register types",
            CycleDiscretes: CycleType(RegisterType::Discrete) => "Cycle discretes", "Include discrete inputs when cycling register types",
        ],
        [
            ShowMockDevice: Toggle => "Show mock device", "Offer the built-in mock device in the Device popup",
            SavePositionOnExit: Toggle => "Save position on exit", "Store the cursor position as the startup position when quitting",
            StartupPanel: Toggle => "Startup panel", "Panel opened on start",
            StartupType: Toggle => "Startup type", "Register type selected on start",
            StartupAddress: Number => "Startup address", "Address the cursor starts on",
        ],
    }
    Api {
        [
            ApiEnabled: Toggle => "Server", "Serve the HTTP API while the app runs",
            ApiHost: Text => "Host", "IP the HTTP API listens on, 0.0.0.0 for all interfaces, 127.0.0.1 for local only",
            ApiPort: Number => "Port", "Port for the HTTP API, 0 tries 26127 then any free port",
        ],
        [
            ApiUnitIdOverride: Toggle => "Unit ID set", "Let API requests target a unit id other than the configured one",
            ApiRawFunction: Toggle => "Raw function", "Let the API send any function code with raw bytes through POST /raw",
        ],
        [
            ApiMcp: Toggle => "MCP", "Serve the Model Context Protocol on /mcp so AI assistants can use the device",
        ],
    }
    Display {
        [
            ShowClock: Toggle => "Show clock", "Show the current time in the bottom bar",
            ClockMillis: Toggle => "Clock milliseconds", "Show milliseconds in the bottom bar clock",
            ShowFrameTime: Toggle => "Show frame time", "Show how long each frame takes to render",
            ShowRam: Toggle => "Show RAM usage", "Show the memory used by the application",
            ShowConnectionLabel: Toggle => "Show connection label", "Show the connection state as a word next to the refresh countdown",
            ShowAsciiStrip: Toggle => "Show ASCII strip", "Show the read registers decoded as an ASCII string",
            ShowInactiveTabs: Toggle => "Show inactive tabs", "Show every panel and register type tab, not just the active one",
            FilterPanelsByType: Toggle => "Filter panels by type", "In Pinned, Labeled and Custom, list only the current register type",
        ],
        [
            CyclePinned: CyclePanel(ReadPanel::Pinned) => "Cycle pinned", "Include the Pinned panel when cycling panels",
            CycleLabeled: CyclePanel(ReadPanel::Labeled) => "Cycle labeled", "Include the Labeled panel when cycling panels",
            CycleCustom: CyclePanel(ReadPanel::Custom) => "Cycle custom", "Include the Custom panel when cycling panels",
            CycleMatrix: CyclePanel(ReadPanel::Matrix) => "Cycle matrix", "Include the Matrix panel when cycling panels",
        ],
        [
            TimeMode: Toggle => "Time format", "Show the read time as a timestamp or as how long ago it was",
            AddressMode: Toggle => "Address format", "Show addresses in decimal or hexadecimal",
            LabelWidth: Number => "Label width", "Characters the label column takes, auto fits the longest label",
            CustomWidth: Number => "Custom width", "Characters the custom column takes, auto fits the longest value",
        ],
        [
            SelectedAddressOnly: Toggle => "Selected address only", "Highlight only the address of the selected row instead of the whole row",
            ShowReadWindow: Toggle => "Show read window", "Highlight the address range covered by the current read batch",
            ShowMatrixContext: Toggle => "Show matrix context", "Show the custom value and label of the selected register under the Matrix panel",
            GraphTimeAxis: Toggle => "Graph time axis", "Plot the graph against time instead of sample count",
            ShowRuleContinuation: Toggle => "Show rule continuation", "Mark the other registers of a multi-register custom rule, pointing to its value",
        ],
        [
            ChangedExpiry: Number => "Changed highlight (ms)", "How long a changed value stays highlighted, 0 never clears it",
            ChangedFade: Toggle => "Fade changed highlight", "Fade a changed value back to the text color over the highlight time",
            ChangedBits: Toggle => "Highlight flipped bits", "In the bits column, highlight only the bits that changed",
            ChangedTrend: Toggle => "Show change direction", "Mark a highlighted value with an up or down arrow after its address",
            ChangedTrendColor: Toggle => "Color change direction", "Color the direction arrow with the rising and falling theme colors",
            ChangedTrendCustom: Toggle => "Direction only per custom", "Show one arrow per custom value on its first register, following the decoded value",
        ],
        [
            PaddingHorizontal: Number => "Horizontal padding", "Empty columns kept on both sides of the interface",
            PaddingVertical: Number => "Vertical padding", "Empty rows kept above and below the interface",
        ],
    }
    Theme {
        [ThemePreset: Toggle => "Preset", "Switch between the built-in color schemes"],
        [
            ThemeBorder: Color => "Frame border", "Color of the frame borders",
            ThemeAccent: Color => "Accent", "Color for titles, keys and highlights",
            ThemeText: Color => "Text", "Main text color",
            ThemeBackground: Color => "Background", "Background color",
            ThemeDim: Color => "Muted", "Color for secondary and muted text",
            ThemeChanged: Color => "Changed value", "Color for values that changed recently",
            ThemeTrendUp: Color => "Rising value", "Color of the direction arrow for a value that went up",
            ThemeTrendDown: Color => "Falling value", "Color of the direction arrow for a value that went down",
            ThemeZebra: Color => "Zebra stripe", "Background of alternating table rows",
            ThemeOk: Color => "Success", "Color for success and connected states",
            ThemeWarning: Color => "Warning", "Color for warnings",
            ThemeError: Color => "Error", "Color for errors",
            ThemeSelectedText: Color => "Selected text", "Text color of the selected row",
            ThemeSelectedBackground: Color => "Selected background", "Background color of the selected row",
            ThemeSeries1: Color => "Graph series 1", "Color of the first held series in the value graph",
            ThemeSeries2: Color => "Graph series 2", "Color of the second held series in the value graph",
            ThemeSeries3: Color => "Graph series 3", "Color of the third held series in the value graph",
        ],
    }
    Keybinds {}
    Library {
        [
            LibraryMbmap: Toggle => "mbmap", "List the community device maps of github.com/nobrega8/mbmap-library in the Library",
            LibrarySunSpec: Toggle => "SunSpec", "List the SunSpec information models of github.com/sunspec/models in the Library",
            LibrarySolarman: Toggle => "Solarman", "List the Solarman inverter definitions of github.com/davidrapan/ha-solarman in the Library",
        ],
        [
            LibraryDisclaimer: Toggle => "Disclaimer", "Whether the notice that library maps come from external, unverified sources was accepted, turn off to show it again",
        ],
        [
            ClearLibraryCache: Action => "Clear cache", "Delete the saved library listings and map files and forget the SunSpec device scan, all downloaded again when needed",
        ],
    }
    Config {
        [
            Name: Text => "Config name", "Name shown in the title bar for this configuration",
            SkipUnsavedWarning: Toggle => "Ignore unsaved warning", "Quit or switch configuration without asking about unsaved changes",
        ],
        [
            ClearPins: Action => "Clear pinned registers", "Remove every pinned register",
            ClearLabels: Action => "Clear labels", "Remove every label",
            ClearCustom: Action => "Clear custom rules", "Remove every custom rule",
        ],
        [
            Save: Action => "Save configuration", "Write the current settings to the configuration file",
            LoadConfig: Text => "Load configuration", "Path of a configuration file to load now",
            NextConfig: Text => "Next configuration", "Configuration file loaded by the cycle config key",
        ],
        [
            ExportFormat: Toggle => "Export format", "Registers: pins, labels and custom rules as MTUI JSON, Whole config: as Save would write it, Home Assistant: modbus YAML, mbmap: .mbmap.json device map",
            ExportTarget: Toggle => "Export to", "Copy to the clipboard or write a file in the directory MTUI was started from",
            Export: Action => "Export", "Export the chosen format to the chosen destination",
        ],
    }
    Changes {}
    Search {}
}

impl SettingsField {
    pub fn matches(self, query: &str) -> bool {
        let label = self.label().to_lowercase();
        let description = self.description().to_lowercase();
        query.split_whitespace().all(|word| {
            label.contains(&word.to_lowercase()) || description.contains(&word.to_lowercase())
        })
    }

    pub fn is_text_input(self) -> bool {
        self.kind() == FieldKind::Text
    }

    pub const fn is_toggle(self) -> bool {
        matches!(
            self.kind(),
            FieldKind::Toggle | FieldKind::CycleType(_) | FieldKind::CyclePanel(_)
        )
    }

    pub const fn is_startup(self) -> bool {
        matches!(
            self,
            Self::StartupPanel | Self::StartupType | Self::StartupAddress
        )
    }

    pub const fn cycle_register_type(self) -> Option<RegisterType> {
        match self.kind() {
            FieldKind::CycleType(register_type) => Some(register_type),
            _ => None,
        }
    }

    pub const fn cycle_panel(self) -> Option<ReadPanel> {
        match self.kind() {
            FieldKind::CyclePanel(panel) => Some(panel),
            _ => None,
        }
    }

    pub fn is_theme_color(self) -> bool {
        self.kind() == FieldKind::Color
    }

    pub fn is_action(self) -> bool {
        self.kind() == FieldKind::Action
    }

    pub const fn library_source(self) -> Option<LibrarySource> {
        match self {
            Self::LibraryMbmap => Some(LibrarySource::Mbmap),
            Self::LibrarySunSpec => Some(LibrarySource::SunSpec),
            Self::LibrarySolarman => Some(LibrarySource::Solarman),
            _ => None,
        }
    }
}

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SettingsCategory {
        Data,
        Api,
        Display,
        Theme,
        Keybinds,
        Library,
        Config,
        Changes,
        Search,
    }
}

impl SettingsCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Data => "Data",
            Self::Api => "API",
            Self::Display => "Display",
            Self::Theme => "Theme",
            Self::Keybinds => "Keybinds",
            Self::Library => "Library",
            Self::Config => "Config",
            Self::Changes => "Changes",
            Self::Search => "Search",
        }
    }

    pub fn search(query: &str) -> Vec<(Option<Self>, Vec<SettingsField>)> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        Self::ALL
            .into_iter()
            .filter(|c| c.is_searchable())
            .map(|c| {
                let fields: Vec<SettingsField> = c
                    .fields()
                    .into_iter()
                    .filter(|f| f.matches(query))
                    .collect();
                (Some(c), fields)
            })
            .filter(|(_, fields)| !fields.is_empty())
            .collect()
    }

    pub fn fields(self) -> Vec<SettingsField> {
        self.groups()
            .iter()
            .flat_map(|g| g.iter().copied())
            .collect()
    }

    pub const fn is_keybinds(self) -> bool {
        matches!(self, Self::Keybinds)
    }

    pub const fn is_changes(self) -> bool {
        matches!(self, Self::Changes)
    }

    pub const fn is_search(self) -> bool {
        matches!(self, Self::Search)
    }

    pub const fn is_searchable(self) -> bool {
        !matches!(
            self,
            Self::Keybinds | Self::Theme | Self::Changes | Self::Search
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Categories,
    Fields,
}

#[derive(Debug, Default, PartialEq)]
pub struct SettingsParams {
    pub category: u16,
    pub field: u16,
    pub focus: SettingsFocus,
    pub status: Option<StatusMessage>,
    pub load_path: String,
    pub previous: ReadParams,
    pub kb_selected: u16,
    pub kb_capturing: bool,
    pub query: String,
    pub api_port_draft: Option<u16>,
    pub api_host_draft: Option<String>,
    pub export_format: ExportFormat,
    pub export_target: ExportTarget,
}

impl SettingsParams {
    pub const KB_PAGE: u16 = 10;

    pub const fn current_category(&self) -> SettingsCategory {
        SettingsCategory::ALL[self.category as usize]
    }

    pub fn current_groups(&self) -> Vec<(Option<SettingsCategory>, Vec<SettingsField>)> {
        let category = self.current_category();
        if category.is_search() {
            return SettingsCategory::search(&self.query);
        }
        category
            .groups()
            .iter()
            .map(|group| (None, group.to_vec()))
            .collect()
    }

    pub fn current_fields(&self) -> Vec<SettingsField> {
        self.current_groups()
            .into_iter()
            .flat_map(|(_, fields)| fields)
            .collect()
    }

    pub fn query_push(&mut self, c: char) {
        self.query.push(c);
        self.field = 0;
    }

    pub fn query_pop(&mut self) {
        self.query.pop();
        self.field = 0;
    }

    pub fn current_field(&self) -> Option<SettingsField> {
        self.current_fields().get(self.field as usize).copied()
    }

    pub fn drop_api_drafts_if_left(&mut self) {
        let editing = (self.focus == SettingsFocus::Fields)
            .then(|| self.current_field())
            .flatten();
        if editing != Some(SettingsField::ApiPort) {
            self.api_port_draft = None;
        }
        if editing != Some(SettingsField::ApiHost) {
            self.api_host_draft = None;
        }
    }

    pub fn enter_category(&mut self) {
        let category = self.current_category();
        let list = category.is_keybinds() || category.is_changes();
        if !list && self.current_fields().is_empty() {
            return;
        }
        self.focus = SettingsFocus::Fields;
        self.field = 0;
        if list {
            self.kb_selected = 0;
            self.kb_capturing = false;
        }
    }

    pub fn cycle_category(&mut self) {
        self.category = wrap_index(self.category, SettingsCategory::ALL.len() as u16, true);
        self.field = 0;
        self.kb_selected = 0;
        self.kb_capturing = false;
    }

    pub fn kb_move(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = wrap_index(self.kb_selected, count, !up);
    }

    pub fn kb_page(&mut self, up: bool, count: u16) {
        if count == 0 {
            return;
        }
        self.kb_selected = if up {
            self.kb_selected.saturating_sub(Self::KB_PAGE)
        } else {
            self.kb_selected
                .saturating_add(Self::KB_PAGE)
                .min(count - 1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{LibrarySource, SettingsCategory, SettingsField, SettingsFocus, SettingsParams};

    #[test]
    fn cycling_categories_wraps_and_keeps_focus() {
        let last = SettingsCategory::ALL.len() as u16 - 1;
        let mut s = SettingsParams {
            category: last,
            field: 3,
            focus: SettingsFocus::Fields,
            ..SettingsParams::default()
        };
        s.cycle_category();
        assert_eq!(
            (s.category, s.field, s.focus),
            (0, 0, SettingsFocus::Fields)
        );
    }

    #[test]
    fn search_is_the_last_category_and_lists_no_static_fields() {
        assert_eq!(
            *SettingsCategory::ALL.last().unwrap(),
            SettingsCategory::Search
        );
        assert!(SettingsCategory::Search.fields().is_empty());
    }

    #[test]
    fn searching_matches_labels_and_descriptions_grouped_by_category() {
        let groups = SettingsCategory::search("clock");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, Some(SettingsCategory::Display));
        assert_eq!(
            groups[0].1,
            vec![SettingsField::ShowClock, SettingsField::ClockMillis]
        );

        let by_description = SettingsCategory::search("auto-refresh");
        assert_eq!(by_description[0].1, vec![SettingsField::RefreshInterval]);

        let words = SettingsCategory::search("CYCLE panel");
        let fields: Vec<_> = words.iter().flat_map(|(_, f)| f.iter().copied()).collect();
        assert!(fields.contains(&SettingsField::CyclePinned));
        assert!(!fields.contains(&SettingsField::CycleHoldings));

        let colors = SettingsCategory::search("color");
        assert!(
            colors
                .iter()
                .all(|(c, _)| *c != Some(SettingsCategory::Theme))
        );

        assert!(SettingsCategory::search("   ").is_empty());
        assert!(SettingsCategory::search("no such setting").is_empty());
    }

    #[test]
    fn typing_a_query_resets_the_field_cursor() {
        let search = SettingsCategory::ALL.len() as u16 - 1;
        let mut s = SettingsParams {
            category: search,
            field: 4,
            ..SettingsParams::default()
        };
        for c in "memory".chars() {
            s.query_push(c);
        }
        assert_eq!(s.field, 0);
        assert_eq!(s.current_fields(), vec![SettingsField::ShowRam]);
        s.query_pop();
        assert_eq!(s.query, "memor");
    }

    #[test]
    fn the_libraries_settings_sit_before_config_and_map_to_their_sources() {
        let categories = SettingsCategory::ALL;
        let position = |c| categories.iter().position(|&x| x == c).unwrap();
        assert_eq!(
            position(SettingsCategory::Library) + 1,
            position(SettingsCategory::Config)
        );
        let sources: Vec<_> = SettingsCategory::Library
            .fields()
            .into_iter()
            .filter_map(SettingsField::library_source)
            .collect();
        assert_eq!(sources, LibrarySource::ALL);
        assert!(
            SettingsCategory::Library
                .fields()
                .contains(&SettingsField::ClearLibraryCache)
        );
        let found = SettingsCategory::search("sunspec");
        assert_eq!(found[0].0, Some(SettingsCategory::Library));
    }

    #[test]
    fn the_save_position_toggle_sits_above_the_startup_fields() {
        let fields = SettingsCategory::Data.fields();
        let position = |field| fields.iter().position(|&f| f == field).unwrap();
        assert!(
            position(SettingsField::SavePositionOnExit) < position(SettingsField::StartupPanel)
        );
        assert_eq!(fields.iter().filter(|f| f.is_startup()).count(), 3);
    }
}
