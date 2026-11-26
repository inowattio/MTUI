use super::{ReadParams, StatusMessage, scroll_window};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LibrarySource {
    Mbmap,
    SunSpec,
    Solarman,
}

impl LibrarySource {
    pub const ALL: [Self; 3] = [Self::Mbmap, Self::SunSpec, Self::Solarman];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Mbmap => "mbmap",
            Self::SunSpec => "SunSpec",
            Self::Solarman => "Solarman",
        }
    }

    pub const fn location(self) -> &'static str {
        match self {
            Self::Mbmap => "github.com/nobrega8/mbmap-library",
            Self::SunSpec => "github.com/sunspec/models",
            Self::Solarman => "github.com/davidrapan/ha-solarman",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Mbmap => {
                "Community device register maps, each checked against the size and hash in the library's index."
            }
            Self::SunSpec => {
                "Information models, pick the start address of the model, prefilled from a scan of the connected device."
            }
            Self::Solarman => "Inverter definitions of the Home Assistant Solarman integration.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    NotDownloaded,
    Loading,
    Downloading,
    Cached,
    Loaded,
    Incomplete,
    OfflineCopy,
    FailedDownload,
    FailedLoad,
}

impl SourceStatus {
    pub const ALL: [Self; 9] = [
        Self::NotDownloaded,
        Self::Loading,
        Self::Downloading,
        Self::Cached,
        Self::Loaded,
        Self::Incomplete,
        Self::OfflineCopy,
        Self::FailedDownload,
        Self::FailedLoad,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::NotDownloaded => "Not downloaded",
            Self::Loading => "Loading",
            Self::Downloading => "Downloading",
            Self::Cached => "Cached",
            Self::Loaded => "Loaded",
            Self::Incomplete => "Incomplete",
            Self::OfflineCopy => "Offline copy",
            Self::FailedDownload => "Failed to download",
            Self::FailedLoad => "Failed to load",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct LibraryViewParams {
    pub source: Option<LibrarySource>,
    pub source_selected: u16,
    pub query: String,
    pub matches: Vec<usize>,
    pub selected: u16,
    pub top: u16,
    pub status: Option<StatusMessage>,
    pub prompt: Option<String>,
    pub pending: Option<LibrarySource>,
    pub previous: ReadParams,
}

impl LibraryViewParams {
    pub const fn new(previous: ReadParams) -> Self {
        Self {
            source: None,
            source_selected: 0,
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            top: 0,
            status: None,
            prompt: None,
            pending: None,
            previous,
        }
    }

    pub fn highlighted_source(&self, sources: &[LibrarySource]) -> Option<LibrarySource> {
        let last = sources.len().checked_sub(1)?;
        sources
            .get(usize::from(self.source_selected).min(last))
            .copied()
    }

    pub fn scroll(&mut self, rows: u16) {
        let len = self.matches.len().min(usize::from(u16::MAX)) as u16;
        scroll_window(&mut self.selected, &mut self.top, rows, len);
    }

    pub fn step(&mut self, delta: i32, rows: u16) {
        let last = self
            .matches
            .len()
            .saturating_sub(1)
            .min(usize::from(u16::MAX));
        let target = i64::from(self.selected) + i64::from(delta);
        self.selected = target.clamp(0, last as i64) as u16;
        self.scroll(rows);
    }

    pub fn selected_entry(&self) -> Option<usize> {
        self.matches.get(usize::from(self.selected)).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::{LibrarySource, LibraryViewParams};
    use crate::state::ReadParams;

    fn library(len: usize) -> LibraryViewParams {
        let mut params = LibraryViewParams::new(ReadParams::default());
        params.matches = (0..len).collect();
        params
    }

    #[test]
    fn the_highlighted_source_comes_from_the_enabled_ones() {
        let mut l = library(0);
        assert_eq!(l.highlighted_source(&[]), None);
        l.source_selected = 5;
        assert_eq!(
            l.highlighted_source(&[LibrarySource::Mbmap, LibrarySource::Solarman]),
            Some(LibrarySource::Solarman)
        );
        l.source_selected = 0;
        assert_eq!(
            l.highlighted_source(&[LibrarySource::SunSpec]),
            Some(LibrarySource::SunSpec)
        );
    }

    #[test]
    fn library_steps_stay_inside_the_matches_and_keep_the_cursor_visible() {
        let mut l = library(50);
        l.step(1, 10);
        assert_eq!((l.selected, l.top, l.selected_entry()), (1, 0, Some(1)));
        l.step(15, 10);
        assert_eq!((l.selected, l.top), (16, 7));
        l.step(i32::MAX, 10);
        assert_eq!((l.selected, l.top), (49, 40));
        l.step(i32::MIN, 10);
        assert_eq!((l.selected, l.top), (0, 0));
        l.step(-1, 10);
        assert_eq!(l.selected, 0);
    }

    #[test]
    fn an_empty_library_has_nothing_selected() {
        let mut l = library(0);
        l.step(1, 10);
        l.step(i32::MAX, 10);
        assert_eq!((l.selected, l.top, l.selected_entry()), (0, 0, None));
    }
}
