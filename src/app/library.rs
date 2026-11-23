#![cfg_attr(target_arch = "wasm32", allow(clippy::future_not_send))]

use super::{App, BackgroundTask};
use crate::compat::{self, TaskHandle, TaskPoll};
use crate::constants::message;
use crate::import::catalog::{self, Item, ItemKind};
use crate::import::{mbmap_index, solarman, sunspec};
use crate::modbus::ModbusDevice;
use crate::register::RegisterType;
use crate::state::{
    ImportSource, LibrarySource, LibraryViewParams, MessageKind, Popup, SettingsCategory,
    SettingsFocus, SourceStatus, State, StatusMessage,
};
use futures::StreamExt;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const SUNSPEC_FETCHES: usize = 8;
const SUNSPEC_MAX_MODELS: usize = 128;
#[cfg(not(target_arch = "wasm32"))]
const USER_AGENT: &str = concat!("mtui/", env!("CARGO_PKG_VERSION"));

#[derive(Debug)]
pub(super) struct Catalog {
    items: Vec<Item>,
    warning: Option<(Note, String)>,
    saved: BTreeSet<String>,
}

impl Catalog {
    fn new(items: Vec<Item>) -> Self {
        let saved = items
            .iter()
            .filter(|item| cache::has(&item.url))
            .map(|item| item.url.clone())
            .collect();
        Self {
            items,
            warning: None,
            saved,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Note {
    Incomplete,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Disk,
    Cached,
    Fresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Missing,
    Download,
    Load,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    stage: Stage,
    message: String,
}

impl Failure {
    fn missing() -> Self {
        Self {
            stage: Stage::Missing,
            message: "not downloaded yet".to_string(),
        }
    }

    const fn download(message: String) -> Self {
        Self {
            stage: Stage::Download,
            message,
        }
    }

    const fn load(message: String) -> Self {
        Self {
            stage: Stage::Load,
            message,
        }
    }

    fn named(self, name: &str) -> Self {
        Self {
            message: format!("{name}: {}", self.message),
            ..self
        }
    }
}

#[derive(Debug)]
pub(super) struct Fetch {
    mode: Mode,
    network: bool,
    handle: TaskHandle<Result<Catalog, Failure>>,
}

pub(super) type SunSpecScan = (String, Result<Vec<SunSpecModel>, String>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SunSpecModel {
    id: u16,
    start: u16,
    length: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MapFormat {
    Mbmap,
    SunSpec { start: u16, length: Option<u16> },
    Solarman { title: String },
}

impl MapFormat {
    const fn source(&self) -> LibrarySource {
        match self {
            Self::Mbmap => LibrarySource::Mbmap,
            Self::SunSpec { .. } => LibrarySource::SunSpec,
            Self::Solarman { .. } => LibrarySource::Solarman,
        }
    }
}

#[derive(Debug)]
pub(super) struct Download {
    handle: TaskHandle<Result<String, String>>,
    format: MapFormat,
    url: String,
}

#[derive(Debug, Default)]
pub(super) struct LibraryState {
    catalogs: BTreeMap<LibrarySource, Catalog>,
    fetches: BTreeMap<LibrarySource, Fetch>,
    failures: BTreeMap<LibrarySource, Stage>,
    download: Option<Download>,
    sunspec: Option<(String, Result<Vec<SunSpecModel>, String>)>,
}

impl App {
    pub fn library_items(&self) -> &[Item] {
        self.library_view()
            .and_then(|l| l.source)
            .and_then(|source| self.library.catalogs.get(&source))
            .map_or(&[], |catalog| &catalog.items)
    }

    pub fn library_sources(&self) -> Vec<LibrarySource> {
        self.config.libraries.sources()
    }

    pub fn clear_library_cache(&mut self) {
        let cached = self.library.catalogs.len();
        self.library = LibraryState::default();
        match cache::clear() {
            Ok(()) => {
                log::info!("Cleared the library cache | {cached} listing(s) in memory");
                self.set_settings_status(StatusMessage::ok(message::LIBRARY_CACHE_CLEARED));
            }
            Err(error) => {
                log::error!("Library cache not cleared | {error}");
                self.set_settings_status(StatusMessage::err(format!(
                    "Library cache not cleared: {error}"
                )));
            }
        }
    }

    pub fn library_saved(&self, source: LibrarySource) -> Option<usize> {
        self.library
            .catalogs
            .get(&source)
            .map(|catalog| catalog.saved.len())
    }

    pub fn library_count(&self, source: LibrarySource) -> Option<usize> {
        self.library
            .catalogs
            .get(&source)
            .map(|catalog| catalog.items.len())
    }

    pub fn library_fetching(&self, source: LibrarySource) -> bool {
        self.library.fetches.contains_key(&source)
    }

    pub fn library_status(&self, source: LibrarySource) -> SourceStatus {
        if let Some(fetch) = self.library.fetches.get(&source) {
            return if fetch.network {
                SourceStatus::Downloading
            } else {
                SourceStatus::Loading
            };
        }
        match self.library.failures.get(&source) {
            Some(Stage::Download) => return SourceStatus::FailedDownload,
            Some(Stage::Load) => return SourceStatus::FailedLoad,
            Some(Stage::Missing) | None => {}
        }
        match self
            .library
            .catalogs
            .get(&source)
            .map(|c| c.warning.as_ref().map(|w| w.0))
        {
            None => SourceStatus::NotDownloaded,
            Some(Some(Note::Incomplete)) => SourceStatus::Incomplete,
            Some(Some(Note::Offline)) => SourceStatus::OfflineCopy,
            Some(None) if cache::ENABLED => SourceStatus::Cached,
            Some(None) => SourceStatus::Loaded,
        }
    }

    fn device_key(&self) -> String {
        let device = &self.config.device;
        serde_json::to_string(&(&device.interface, device.unit_id)).unwrap_or_default()
    }

    fn sunspec_scan(&self) -> Option<&Result<Vec<SunSpecModel>, String>> {
        self.library
            .sunspec
            .as_ref()
            .filter(|(key, _)| *key == self.device_key())
            .map(|(_, result)| result)
    }

    pub fn sunspec_found(&self, id: u16) -> Option<(u16, u16)> {
        match self.sunspec_scan() {
            Some(Ok(models)) => models
                .iter()
                .find(|model| model.id == id)
                .map(|model| (model.start, model.length)),
            _ => None,
        }
    }

    pub fn sunspec_scan_note(&self) -> Option<String> {
        match self.sunspec_scan() {
            Some(Ok(models)) => Some(format!(
                "{} SunSpec model(s) found on the device",
                models.len()
            )),
            Some(Err(error)) => Some(format!("No SunSpec models read: {error}")),
            None => None,
        }
    }

    pub const fn library_view(&self) -> Option<&LibraryViewParams> {
        match &self.state {
            State::Library(l) => Some(l),
            _ => None,
        }
    }

    const fn library_view_mut(&mut self) -> Option<&mut LibraryViewParams> {
        match &mut self.state {
            State::Library(l) => Some(l),
            _ => None,
        }
    }

    fn library_source(&self) -> Option<LibrarySource> {
        self.library_view().and_then(|l| l.source)
    }

    fn set_library_status(&mut self, status: Option<StatusMessage>) {
        if let Some(l) = self.library_view_mut() {
            l.status = status;
        }
    }

    fn set_source_status(&mut self, source: LibrarySource, status: Option<StatusMessage>) {
        if self.library_source() == Some(source) {
            self.set_library_status(status);
        }
    }

    pub fn open_library(&mut self) {
        let sources = self.library_sources();
        if sources.is_empty() {
            self.read_mut().popup = Some(Popup::NoLibraries);
            return;
        }
        self.library.download = None;
        let previous = std::mem::take(self.read_mut());
        self.state = State::Library(LibraryViewParams::new(previous));
        for &source in &sources {
            let known = self.library.catalogs.contains_key(&source)
                || self.library.failures.contains_key(&source);
            if !known {
                self.fetch_catalog(source, Mode::Disk);
            }
        }
        if sources.len() == 1 {
            self.choose_library_source();
        }
    }

    pub fn open_library_settings(&mut self) {
        self.close_popup();
        self.open_settings();
        let category = SettingsCategory::ALL
            .iter()
            .position(|&category| category == SettingsCategory::Library)
            .unwrap_or_default();
        if let Some(s) = self.settings_mut() {
            s.category = category as u16;
            s.focus = SettingsFocus::Fields;
        }
    }

    pub fn close_library_view(&mut self) {
        let previous = match &mut self.state {
            State::Library(l) => std::mem::take(&mut l.previous),
            _ => return,
        };
        self.library.download = None;
        self.state = State::Read(previous);
    }

    pub fn library_back(&mut self) {
        let single = self.library_sources().len() == 1;
        let Some(l) = self.library_view_mut() else {
            return;
        };
        if l.prompt.take().is_some() {
            l.status = None;
            return;
        }
        if l.source.take().is_some() && !single {
            l.query.clear();
            l.matches.clear();
            l.selected = 0;
            l.top = 0;
            l.status = None;
            self.library.download = None;
            return;
        }
        self.close_library_view();
    }

    fn choose_library_source(&mut self) {
        let sources = self.library_sources();
        let Some(source) = self
            .library_view()
            .and_then(|l| l.highlighted_source(&sources))
        else {
            return;
        };
        if self.library.catalogs.contains_key(&source) {
            self.enter_library_source(source, true);
            return;
        }
        self.fetch_catalog(source, Mode::Cached);
        if let Some(l) = self.library_view_mut() {
            l.pending = Some(source);
            l.status = None;
        }
    }

    fn enter_library_source(&mut self, source: LibrarySource, refresh: bool) {
        if let Some(l) = self.library_view_mut() {
            l.source = Some(source);
            l.pending = None;
            l.status = None;
        }
        self.recompute_library(None);
        let warning = self
            .library
            .catalogs
            .get(&source)
            .and_then(|catalog| catalog.warning.as_ref())
            .map(|(_, warning)| StatusMessage::warn(warning.clone()));
        let retry = self
            .library
            .catalogs
            .get(&source)
            .and_then(|catalog| catalog.warning.as_ref())
            .map(|(note, _)| match note {
                Note::Offline => Mode::Fresh,
                Note::Incomplete => Mode::Cached,
            });
        if refresh && let Some(mode) = retry {
            self.fetch_catalog(source, mode);
        }
        if self.library_fetching(source) {
            self.set_library_status(Some(StatusMessage::info(format!(
                "Updating {}...",
                source.name()
            ))));
        } else {
            self.set_library_status(warning);
        }
        if source == LibrarySource::SunSpec {
            self.start_sunspec_scan();
        }
    }

    fn fetch_catalog(&mut self, source: LibrarySource, mode: Mode) {
        let running = self.library.fetches.get(&source).map(|fetch| fetch.mode);
        if running.is_some_and(|running| running != Mode::Disk || mode == Mode::Disk) {
            return;
        }
        let incomplete = self
            .library
            .catalogs
            .get(&source)
            .and_then(|catalog| catalog.warning.as_ref())
            .is_some_and(|(note, _)| *note == Note::Incomplete);
        let network = match mode {
            Mode::Disk => false,
            Mode::Fresh => true,
            Mode::Cached => incomplete || !cache::has(listing_url(source)),
        };
        log::info!(
            "Loading the {} catalog | {mode:?}, network {network}",
            source.name()
        );
        let handle = match source {
            LibrarySource::Mbmap => compat::spawn(fetch_mbmap(mode)),
            LibrarySource::SunSpec => compat::spawn(fetch_sunspec(mode)),
            LibrarySource::Solarman => compat::spawn(fetch_solarman(mode)),
        };
        self.library.fetches.insert(
            source,
            Fetch {
                mode,
                network,
                handle,
            },
        );
    }

    pub fn library_input(&mut self, c: char) {
        let Some(l) = self.library_view_mut() else {
            return;
        };
        if let Some(prompt) = &mut l.prompt {
            if c.is_ascii_alphanumeric() {
                prompt.push(c);
            }
            return;
        }
        if l.source.is_none() {
            return;
        }
        l.query.push(c);
        self.recompute_library(None);
    }

    pub fn library_paste(&mut self, text: &str) {
        for c in text.lines().next().unwrap_or_default().trim().chars() {
            self.library_input(c);
        }
    }

    pub fn library_backspace(&mut self) {
        let Some(l) = self.library_view_mut() else {
            return;
        };
        if let Some(prompt) = &mut l.prompt {
            prompt.pop();
            return;
        }
        if l.source.is_none() {
            return;
        }
        l.query.pop();
        self.recompute_library(None);
    }

    pub fn library_move(&mut self, delta: i32) {
        let rows = self.layout.visible_rows;
        let Some(l) = self.library_view_mut() else {
            return;
        };
        if l.prompt.is_some() {
            return;
        }
        if l.source.is_some() {
            l.step(delta, rows);
            return;
        }
        let sources = self.config.libraries.sources().len();
        let Some(l) = self.library_view_mut() else {
            return;
        };
        let last = sources.saturating_sub(1) as i64;
        let selected = (i64::from(l.source_selected) + i64::from(delta)).clamp(0, last) as u16;
        if selected != l.source_selected {
            l.pending = None;
        }
        l.source_selected = selected;
    }

    pub fn library_page(&mut self, down: bool) {
        let page = i32::from(self.layout.visible_rows.max(1));
        self.library_move(if down { page } else { -page });
    }

    pub fn library_commit(&mut self) {
        let Some(l) = self.library_view() else {
            return;
        };
        if l.source.is_none() {
            self.choose_library_source();
            return;
        }
        let prompt = l.prompt.clone();
        let Some(item) = l
            .selected_entry()
            .and_then(|i| self.library_items().get(i))
            .cloned()
        else {
            return;
        };
        if self.library.download.is_some() {
            return;
        }
        let source = self.library_source();
        if source.is_some_and(|source| self.library_fetching(source)) {
            return;
        }

        let format = match (&item.kind, prompt) {
            (ItemKind::Mbmap(_), _) => MapFormat::Mbmap,
            (ItemKind::Solarman, _) => MapFormat::Solarman {
                title: item.title(),
            },
            (ItemKind::SunSpec(id), None) => {
                let start = self
                    .sunspec_found(*id)
                    .map_or(sunspec::DEFAULT_START, |f| f.0);
                let text = self.interpreter.address_mode().display(start).to_string();
                if let Some(l) = self.library_view_mut() {
                    l.prompt = Some(text);
                    l.status = None;
                }
                return;
            }
            (ItemKind::SunSpec(id), Some(text)) => {
                let Some(start) = self.interpreter.address_mode().parse(text.trim()) else {
                    self.set_library_status(Some(StatusMessage::err(message::LIBRARY_BAD_START)));
                    return;
                };
                let length = self
                    .sunspec_found(*id)
                    .filter(|&(found, _)| found == start)
                    .map(|(_, length)| length);
                MapFormat::SunSpec { start, length }
            }
        };

        let verb = if cache::has(&item.url) {
            "Loading"
        } else {
            "Downloading"
        };
        log::info!("{verb} {} from {}", item.path, format.source().name());
        if let Some(l) = self.library_view_mut() {
            l.prompt = None;
            l.status = Some(StatusMessage::info(format!("{verb} {}...", item.title())));
        }
        self.library.download = Some(Download {
            url: item.url.clone(),
            handle: compat::spawn(fetch_item(item)),
            format,
        });
    }

    fn selected_library_path(&self) -> Option<String> {
        self.library_view()
            .and_then(LibraryViewParams::selected_entry)
            .and_then(|i| self.library_items().get(i))
            .map(|item| item.path.clone())
    }

    fn recompute_library(&mut self, keep: Option<String>) {
        let Some(query) = self.library_view().map(|l| l.query.clone()) else {
            return;
        };
        let items = self.library_items();
        let mut matches = catalog::search(items, &query);
        if query.trim().is_empty() {
            matches.sort_by_key(|&i| {
                items
                    .get(i)
                    .and_then(Item::sunspec_id)
                    .and_then(|id| self.sunspec_found(id))
                    .is_none()
            });
        }
        let selected = keep
            .and_then(|path| {
                matches
                    .iter()
                    .position(|&i| items.get(i).is_some_and(|item| item.path == path))
            })
            .unwrap_or(0);
        let rows = self.layout.visible_rows;
        if let Some(l) = self.library_view_mut() {
            l.matches = matches;
            l.selected = 0;
            l.top = 0;
            l.step(i32::try_from(selected).unwrap_or(i32::MAX), rows);
        }
    }

    pub(super) fn poll_library_task(&mut self) {
        fn stopped<T>() -> Result<T, String> {
            Err(message::LIBRARY_TASK_STOPPED.to_string())
        }

        let mut finished = Vec::new();
        for (&source, fetch) in &mut self.library.fetches {
            match fetch.handle.poll_result() {
                TaskPoll::Pending => {}
                TaskPoll::Finished(result) => finished.push((source, result)),
                TaskPoll::Gone => finished.push((source, stopped().map_err(Failure::load))),
            }
        }
        for (source, result) in finished {
            self.library.fetches.remove(&source);
            self.finish_catalog(source, result);
        }

        let downloaded = match self.library.download.as_mut() {
            None => None,
            Some(download) => match download.handle.poll_result() {
                TaskPoll::Pending => None,
                TaskPoll::Finished(result) => Some((download.format.clone(), result)),
                TaskPoll::Gone => Some((download.format.clone(), stopped())),
            },
        };
        if let Some((format, result)) = downloaded {
            if let Some(download) = self.library.download.take()
                && result.is_ok()
                && cache::ENABLED
                && let Some(catalog) = self.library.catalogs.get_mut(&format.source())
            {
                catalog.saved.insert(download.url);
            }
            self.finish_download(format, result);
        }
    }

    fn finish_catalog(&mut self, source: LibrarySource, result: Result<Catalog, Failure>) {
        let pending = self.library_view().and_then(|l| l.pending) == Some(source);
        let viewing = self.library_source() == Some(source);
        let kept = viewing.then(|| self.selected_library_path()).flatten();
        let status = match result {
            Ok(catalog) => {
                log::info!(
                    "{} catalog loaded | {} item(s)",
                    source.name(),
                    catalog.items.len()
                );
                let status = catalog.warning.as_ref().map(|(_, warning)| {
                    log::warn!("{} | {warning}", source.name());
                    StatusMessage::warn(warning.clone())
                });
                self.library.failures.remove(&source);
                self.library.catalogs.insert(source, catalog);
                if pending {
                    self.enter_library_source(source, false);
                    return;
                }
                status
            }
            Err(failure) if failure.stage == Stage::Missing => {
                if viewing || pending {
                    self.fetch_catalog(source, Mode::Cached);
                }
                return;
            }
            Err(failure) if self.library.catalogs.contains_key(&source) => {
                log::warn!("{} not refreshed | {}", source.name(), failure.message);
                let warning = format!("Offline, showing the loaded library: {}", failure.message);
                if let Some(catalog) = self.library.catalogs.get_mut(&source) {
                    catalog.warning = Some((Note::Offline, warning.clone()));
                }
                if pending && let Some(l) = self.library_view_mut() {
                    l.pending = None;
                }
                Some(StatusMessage::warn(warning))
            }
            Err(failure) => {
                log::error!("{} unavailable | {}", source.name(), failure.message);
                self.library.failures.insert(source, failure.stage);
                let status = StatusMessage::err(format!(
                    "{} unavailable: {}",
                    source.name(),
                    failure.message
                ));
                if pending && let Some(l) = self.library_view_mut() {
                    l.pending = None;
                    l.status = Some(status);
                    return;
                }
                Some(status)
            }
        };
        if self.library_source() != Some(source) {
            return;
        }
        match status {
            Some(status) => self.set_library_status(Some(status)),
            None => {
                if let Some(l) = self.library_view_mut()
                    && l.status
                        .as_ref()
                        .is_some_and(|s| s.kind == MessageKind::Info)
                {
                    l.status = None;
                }
            }
        }
        self.recompute_library(kept);
    }

    fn finish_download(&mut self, format: MapFormat, result: Result<String, String>) {
        let source = format.source();
        if self.library_source() != Some(source) {
            return;
        }
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                log::error!("Download failed | {error}");
                if source == LibrarySource::Mbmap {
                    self.set_source_status(
                        source,
                        Some(StatusMessage::err(format!(
                            "Download failed: {error}. Refreshing the library, try again."
                        ))),
                    );
                    self.fetch_catalog(source, Mode::Fresh);
                } else {
                    self.set_source_status(
                        source,
                        Some(StatusMessage::err(format!("Download failed: {error}"))),
                    );
                }
                return;
            }
        };

        let devices = match format {
            MapFormat::Mbmap => {
                self.close_library_view();
                if !self.import_text(&text) {
                    log::error!("Downloaded map could not be imported");
                    self.set_read_status(StatusMessage::err(message::LIBRARY_MAP_UNREADABLE));
                }
                return;
            }
            MapFormat::SunSpec { start, length } => sunspec::parse(&text, start, length)
                .map(|device| (vec![device], ImportSource::SunSpec)),
            MapFormat::Solarman { title } => solarman::parse_named(&text, &title)
                .filter(|devices| !devices.is_empty())
                .map(|devices| (devices, ImportSource::Solarman)),
        };
        match devices {
            Some((devices, import_source)) => {
                self.close_library_view();
                self.open_import(devices, import_source);
            }
            None => {
                log::error!("Downloaded {} file could not be imported", source.name());
                self.set_library_status(Some(StatusMessage::err(message::LIBRARY_MAP_UNREADABLE)));
            }
        }
    }

    fn start_sunspec_scan(&mut self) {
        if matches!(self.sunspec_scan(), Some(Ok(_))) {
            return;
        }
        let Some(device) = self.device.clone() else {
            return;
        };
        if !self.free_background_slot() {
            return;
        }
        log::info!("Scanning the device for SunSpec models");
        let key = self.device_key();
        self.background_task = Some(BackgroundTask::SunSpecScan(compat::spawn(async move {
            (key, scan_sunspec(&device).await)
        })));
    }

    pub(super) fn apply_sunspec_scan(&mut self, result: Option<SunSpecScan>) {
        let (key, result) = result.unwrap_or_else(|| {
            (
                self.device_key(),
                Err(message::LIBRARY_TASK_STOPPED.to_string()),
            )
        });
        match &result {
            Ok(models) => log::info!("SunSpec scan found {} model(s)", models.len()),
            Err(error) => log::info!("SunSpec scan found nothing | {error}"),
        }
        let kept = self.selected_library_path();
        self.library.sunspec = Some((key, result));
        if self.library_source() != Some(LibrarySource::SunSpec) {
            return;
        }
        self.recompute_library(kept);
        let found = self
            .library_view()
            .and_then(LibraryViewParams::selected_entry)
            .and_then(|i| self.library_items().get(i))
            .and_then(Item::sunspec_id)
            .and_then(|id| self.sunspec_found(id));
        let mode = self.interpreter.address_mode();
        let default = mode.display(sunspec::DEFAULT_START).to_string();
        if let Some((start, _)) = found
            && let Some(l) = self.library_view_mut()
            && l.prompt.as_deref() == Some(default.as_str())
        {
            l.prompt = Some(mode.display(start).to_string());
        }
    }
}

async fn read_pair(device: &ModbusDevice, address: u16) -> Result<[u16; 2], String> {
    let words = device
        .read_typed(None, RegisterType::Holding, address, 2)
        .await
        .map_err(|error| error.to_string())?;
    match words[..] {
        [first, second] => Ok([first, second]),
        _ => Err(format!("short read at {address}")),
    }
}

async fn scan_sunspec(device: &ModbusDevice) -> Result<Vec<SunSpecModel>, String> {
    walk_sunspec(|address| read_pair(device, address)).await
}

async fn walk_sunspec<F, R>(mut read: F) -> Result<Vec<SunSpecModel>, String>
where
    F: FnMut(u16) -> R,
    R: Future<Output = Result<[u16; 2], String>>,
{
    let mut base = None;
    for address in sunspec::BASE_ADDRESSES {
        if read(address).await.ok() == Some(sunspec::MARKER) {
            base = Some(address);
            break;
        }
    }
    let base = base.ok_or("no SunSpec marker at 40000, 0 or 50000")?;
    let mut models = Vec::new();
    let mut start = base.checked_add(2);
    while let Some(address) = start {
        if models.len() >= SUNSPEC_MAX_MODELS {
            break;
        }
        let [id, length] = read(address).await?;
        if id == sunspec::END_MODEL {
            break;
        }
        models.push(SunSpecModel {
            id,
            start: address,
            length,
        });
        start = address.checked_add(2).and_then(|a| a.checked_add(length));
    }
    Ok(models)
}

fn client() -> reqwest::Client {
    #[cfg(not(target_arch = "wasm32"))]
    {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default()
    }
    #[cfg(target_arch = "wasm32")]
    {
        reqwest::Client::new()
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let request = async {
        let response = client().get(url).send().await?.error_for_status()?;
        response.bytes().await
    };
    match compat::timeout(REQUEST_TIMEOUT, request).await {
        Ok(Ok(bytes)) => Ok(bytes.to_vec()),
        Ok(Err(error)) => Err(error.without_url().to_string()),
        Err(elapsed) => Err(elapsed.to_string()),
    }
}

fn text(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| "the file is not valid UTF-8".to_string())
}

const fn listing_url(source: LibrarySource) -> &'static str {
    match source {
        LibrarySource::Mbmap => mbmap_index::INDEX_URL,
        LibrarySource::SunSpec => catalog::SUNSPEC_LISTING_URL,
        LibrarySource::Solarman => catalog::SOLARMAN_LISTING_URL,
    }
}

fn cached<T>(url: &str, parse: impl Fn(&[u8]) -> Result<T, String>) -> Option<T> {
    parse(&cache::read(url)?).ok()
}

async fn download<T>(url: &str, parse: impl Fn(&[u8]) -> Result<T, String>) -> Result<T, Failure> {
    let bytes = fetch(url).await.map_err(Failure::download)?;
    let value = parse(&bytes).map_err(Failure::load)?;
    cache::write(url, &bytes);
    Ok(value)
}

async fn load<T>(
    url: &str,
    mode: Mode,
    parse: impl Fn(&[u8]) -> Result<T, String>,
) -> Result<T, Failure> {
    if mode != Mode::Fresh
        && let Some(value) = cached(url, &parse)
    {
        return Ok(value);
    }
    if mode == Mode::Disk {
        return Err(Failure::missing());
    }
    download(url, parse).await
}

async fn fetch_mbmap(mode: Mode) -> Result<Catalog, Failure> {
    let items = |entries: Vec<mbmap_index::Entry>| entries.into_iter().map(Item::from).collect();
    match load(mbmap_index::INDEX_URL, mode, mbmap_index::parse_index).await {
        Ok(entries) => Ok(Catalog::new(items(entries))),
        Err(failure) if mode == Mode::Fresh => {
            match cached(mbmap_index::INDEX_URL, mbmap_index::parse_index) {
                Some(entries) => {
                    let mut catalog = Catalog::new(items(entries));
                    catalog.warning = Some((
                        Note::Offline,
                        format!("Offline, showing the cached library: {}", failure.message),
                    ));
                    Ok(catalog)
                }
                None => Err(failure),
            }
        }
        Err(failure) => Err(failure),
    }
}

async fn fetch_sunspec(mode: Mode) -> Result<Catalog, Failure> {
    let files = load(catalog::SUNSPEC_LISTING_URL, mode, |bytes| {
        catalog::listing(bytes, &[".json"])
    })
    .await?;
    let file_mode = if mode == Mode::Disk {
        Mode::Disk
    } else {
        Mode::Cached
    };
    let results: Vec<Result<Option<Item>, Failure>> = futures::stream::iter(
        files
            .into_iter()
            .filter(|(name, _)| catalog::sunspec_file_id(name).is_some()),
    )
    .map(|(name, url)| async move {
        let text = load(&url, file_mode, text)
            .await
            .map_err(|failure| failure.named(&name))?;
        Ok(sunspec::model_info(&text).map(|info| catalog::sunspec_item(name, url, info)))
    })
    .buffer_unordered(SUNSPEC_FETCHES)
    .collect()
    .await;
    let mut items = Vec::new();
    let mut failed = Vec::new();
    for result in results {
        match result {
            Ok(item) => items.extend(item),
            Err(failure) => failed.push(failure),
        }
    }
    if items.is_empty() {
        return Err(failed
            .into_iter()
            .min_by_key(|failure| failure.stage != Stage::Download)
            .unwrap_or_else(|| Failure::load("no SunSpec model could be read".to_string())));
    }
    items.sort_by_key(Item::sunspec_id);
    let mut catalog = Catalog::new(items);
    catalog.warning = partial_warning(&failed).map(|warning| (Note::Incomplete, warning));
    Ok(catalog)
}

fn partial_warning(failed: &[Failure]) -> Option<String> {
    let first = failed.first()?;
    let verb = if failed.iter().all(|failure| failure.stage == Stage::Missing) {
        "are not downloaded yet"
    } else {
        "could not be downloaded"
    };
    Some(format!(
        "{} model file(s) {verb} ({}), retried on the next visit",
        failed.len(),
        first.message
    ))
}

async fn fetch_solarman(mode: Mode) -> Result<Catalog, Failure> {
    let files = load(catalog::SOLARMAN_LISTING_URL, mode, |bytes| {
        catalog::listing(bytes, &[".yaml", ".yml"])
    })
    .await?;
    let mut items: Vec<Item> = files
        .into_iter()
        .map(|(name, url)| catalog::solarman_item(name, url))
        .collect();
    items.sort_by_key(|item| item.title().to_lowercase());
    Ok(Catalog::new(items))
}

async fn fetch_item(item: Item) -> Result<String, String> {
    let loaded = match item.kind {
        ItemKind::Mbmap(entry) => {
            load(&item.url, Mode::Cached, |bytes| {
                mbmap_index::verify_map(&entry, bytes.to_vec())
            })
            .await
        }
        ItemKind::SunSpec(_) | ItemKind::Solarman => load(&item.url, Mode::Cached, text).await,
    };
    loaded.map_err(|failure| failure.message)
}

#[cfg(not(target_arch = "wasm32"))]
mod cache {
    use crate::import::catalog;
    use std::fs;
    use std::path::PathBuf;

    pub const ENABLED: bool = true;

    fn dir() -> Option<PathBuf> {
        directories::ProjectDirs::from("io", "inowattio", "mtui")
            .map(|dirs| dirs.cache_dir().join("library"))
    }

    pub fn has(url: &str) -> bool {
        dir().is_some_and(|dir| dir.join(catalog::cache_file_name(url)).is_file())
    }

    pub fn read(url: &str) -> Option<Vec<u8>> {
        fs::read(dir()?.join(catalog::cache_file_name(url))).ok()
    }

    pub fn write(url: &str, bytes: &[u8]) {
        let Some(dir) = dir() else {
            return;
        };
        let name = catalog::cache_file_name(url);
        let staged = dir.join(format!("{name}.part"));
        let result = fs::create_dir_all(&dir)
            .and_then(|()| fs::write(&staged, bytes))
            .and_then(|()| fs::rename(&staged, dir.join(&name)));
        if let Err(error) = result {
            log::warn!("Library cache not saved | {name} | {error}");
        }
    }

    pub fn clear() -> std::io::Result<()> {
        let Some(dir) = dir() else {
            return Ok(());
        };
        match fs::remove_dir_all(dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod cache {
    pub const ENABLED: bool = false;

    pub const fn has(_url: &str) -> bool {
        false
    }

    pub const fn read(_url: &str) -> Option<Vec<u8>> {
        None
    }

    pub const fn write(_url: &str, _bytes: &[u8]) {}

    pub const fn clear() -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Failure, SUNSPEC_MAX_MODELS, SunSpecModel, walk_sunspec};
    use std::collections::BTreeMap;

    fn walk(words: &[(u16, u16)]) -> Result<Vec<SunSpecModel>, String> {
        let memory: BTreeMap<u16, u16> = words.iter().copied().collect();
        futures::executor::block_on(walk_sunspec(|address| {
            let pair = match (memory.get(&address), address.checked_add(1)) {
                (Some(&first), Some(next)) => Ok([first, memory.get(&next).copied().unwrap_or(0)]),
                _ => Err(format!("no register at {address}")),
            };
            std::future::ready(pair)
        }))
    }

    fn model(id: u16, start: u16, length: u16) -> SunSpecModel {
        SunSpecModel { id, start, length }
    }

    #[test]
    fn the_chain_is_walked_from_the_marker_to_the_end_model() {
        let words = [
            (40000, 0x5375),
            (40001, 0x6E53),
            (40002, 1),
            (40003, 66),
            (40070, 103),
            (40071, 50),
            (40122, 0xFFFF),
            (40123, 0),
        ];
        assert_eq!(
            walk(&words),
            Ok(vec![model(1, 40002, 66), model(103, 40070, 50)])
        );
    }

    #[test]
    fn the_marker_is_also_looked_for_at_0_and_50000() {
        let at_zero = [(0, 0x5375), (1, 0x6E53), (2, 1), (3, 0), (4, 0xFFFF)];
        assert_eq!(walk(&at_zero), Ok(vec![model(1, 2, 0)]));
        let at_50000 = [(50000, 0x5375), (50001, 0x6E53), (50002, 0xFFFF)];
        assert_eq!(walk(&at_50000), Ok(Vec::new()));
    }

    #[test]
    fn a_device_without_the_marker_has_no_models() {
        assert!(walk(&[(40000, 1), (40001, 2)]).is_err());
        assert!(walk(&[]).is_err());
    }

    #[test]
    fn a_read_error_inside_the_chain_fails_the_scan() {
        let words = [(40000, 0x5375), (40001, 0x6E53), (40002, 1), (40003, 66)];
        assert!(walk(&words).is_err());
    }

    #[test]
    fn the_walk_stops_at_the_end_of_the_address_space() {
        let words = [(40000, 0x5375), (40001, 0x6E53), (40002, 7), (40003, 30000)];
        assert_eq!(walk(&words), Ok(vec![model(7, 40002, 30000)]));
    }

    #[test]
    fn partial_sunspec_downloads_warn_with_the_first_error() {
        assert_eq!(super::partial_warning(&[]), None);
        let failed = [
            Failure::download("timed out".to_string()).named("model_103.json"),
            Failure::missing().named("model_1.json"),
        ];
        let warning = super::partial_warning(&failed).expect("warning");
        assert!(warning.starts_with("2 model file(s) could not be downloaded"));
        assert!(warning.contains("model_103.json: timed out"));
        let missing = [Failure::missing().named("model_1.json")];
        assert!(
            super::partial_warning(&missing)
                .expect("warning")
                .starts_with("1 model file(s) are not downloaded yet")
        );
    }

    #[test]
    fn failures_keep_their_stage_when_named() {
        let failure = Failure::load("bad json".to_string()).named("model_7.json");
        assert_eq!(failure.message, "model_7.json: bad json");
        assert_eq!(failure.stage, super::Stage::Load);
    }

    #[test]
    fn a_looping_chain_is_cut_after_the_model_cap() {
        let mut words = vec![(0, 0x5375), (1, 0x6E53)];
        words.extend((2..2000).step_by(2).flat_map(|a| [(a, 9), (a + 1, 0)]));
        assert_eq!(walk(&words).map(|m| m.len()), Ok(SUNSPEC_MAX_MODELS));
    }
}
