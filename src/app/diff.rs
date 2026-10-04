use super::App;
#[cfg(not(target_arch = "wasm32"))]
use super::dropped::dropped_dump;
use crate::config::Column;
use crate::constants::{NO_VALUE, message};
use crate::interpretator::{RowSegment, following};
use crate::num_ops::step_hscroll;
use crate::register::RegisterCell;
use crate::snapshot::{
    self, Diff, DiffLine, DiffMark, DiffRow, Snapshot, SnapshotEntry, SnapshotError,
};
use crate::state::{DiffViewParams, State, StatusMessage};
use chrono::{DateTime, Utc};

pub(super) enum PastedDump {
    Snapshot(Snapshot),
    Invalid(String),
    Other,
}

impl App {
    pub const fn diff_view(&self) -> Option<&DiffViewParams> {
        match &self.state {
            State::Diff(d) => Some(d),
            _ => None,
        }
    }

    pub const fn diff_view_mut(&mut self) -> Option<&mut DiffViewParams> {
        match &mut self.state {
            State::Diff(d) => Some(d),
            _ => None,
        }
    }

    pub fn paste_text(&mut self, text: &str) {
        if self.paste_import(text) {
            return;
        }
        match self.pasted_dump(text) {
            PastedDump::Snapshot(snapshot) => self.open_diff_view(snapshot),
            PastedDump::Invalid(reason) => self.set_read_status(StatusMessage::warn(reason)),
            PastedDump::Other => {
                self.set_read_status(StatusMessage::warn(message::PASTE_NOT_REGISTERS));
            }
        }
    }

    pub fn paste_into_diff_view(&mut self, text: &str) {
        match self.pasted_dump(text) {
            PastedDump::Snapshot(snapshot) => {
                log::info!(
                    "Replaced the diff snapshot | {} register(s)",
                    snapshot.len()
                );
                if let Some(d) = self.diff_view_mut() {
                    d.snapshot = snapshot;
                    d.top = 0;
                    d.h_offset = 0;
                }
                self.rediff();
            }
            PastedDump::Invalid(reason) => log::warn!("Paste ignored | {reason}"),
            PastedDump::Other => {}
        }
    }

    fn pasted_dump(&self, text: &str) -> PastedDump {
        let mode = self.interpreter.address_mode();
        match snapshot::parse_dump(text, mode) {
            Ok(snapshot) => PastedDump::Snapshot(snapshot),
            Err(SnapshotError::NotADump) => dropped_dump(text, mode),
            Err(error) => PastedDump::Invalid(error.to_string()),
        }
    }

    pub fn open_diff_view(&mut self, snapshot: Snapshot) {
        log::info!(
            "Opened a diff | {} register(s) in the snapshot",
            snapshot.len()
        );
        let diff = self.diff_of(&snapshot);
        let previous = std::mem::take(self.read_mut());
        self.state = State::Diff(DiffViewParams {
            snapshot,
            diff,
            top: 0,
            h_offset: 0,
            changed_only: false,
            previous,
        });
    }

    pub fn close_diff_view(&mut self) {
        let previous = match &mut self.state {
            State::Diff(d) => std::mem::take(&mut d.previous),
            _ => return,
        };
        self.state = State::Read(previous);
    }

    pub const fn diff_toggle_changed(&mut self) {
        if let Some(d) = self.diff_view_mut() {
            d.changed_only = !d.changed_only;
            d.top = 0;
        }
    }

    pub(super) fn rediff(&mut self) {
        let Some(diff) = self.diff_view().map(|d| self.diff_of(&d.snapshot)) else {
            return;
        };
        if let Some(d) = self.diff_view_mut() {
            d.diff = diff;
        }
    }

    pub fn diff_scroll(&mut self, delta: isize) {
        let visible = self.layout.visible_rows.max(1) as usize;
        if let Some(d) = self.diff_view_mut() {
            let max_top = d.diff.rows(d.changed_only).len().saturating_sub(visible);
            d.top = d.top.min(max_top).saturating_add_signed(delta).min(max_top);
        }
    }

    pub fn diff_hscroll(&mut self, right: bool) {
        let max = self.layout.h_max_offset;
        if let Some(d) = self.diff_view_mut() {
            d.h_offset = step_hscroll(d.h_offset, max, right);
        }
    }

    fn diff_of(&self, snapshot: &Snapshot) -> Diff {
        Diff::new(snapshot, |cell| self.cell_value(cell))
    }

    pub fn diff_lines(
        &self,
        snapshot: &Snapshot,
        rows: &[DiffRow],
        now: DateTime<Utc>,
    ) -> Vec<DiffLine> {
        rows.iter()
            .map(|&row| self.diff_line(snapshot, row, now))
            .collect()
    }

    fn diff_line(&self, snapshot: &Snapshot, row: DiffRow, now: DateTime<Utc>) -> DiffLine {
        let DiffRow { cell, mark } = row;
        let current = || self.cell_row(cell, now).map(|(text, _)| text);
        let recorded = || {
            snapshot
                .get(&cell)
                .map(|entry| self.snapshot_row(snapshot, cell, entry))
        };
        let placeholder = || self.interpreter.placeholder(cell.1, self.label(cell));
        let (text, other) = match mark {
            DiffMark::Same => (current(), None),
            DiffMark::Unread => (None, None),
            DiffMark::Before => (recorded(), current()),
            DiffMark::After => (current(), recorded()),
        };
        let text = text.unwrap_or_else(placeholder);
        let emphasis = other
            .map(|other| self.differing_segments(&text, &other))
            .unwrap_or_default();
        DiffLine {
            mark,
            cell,
            text,
            emphasis,
        }
    }

    fn snapshot_row(
        &self,
        snapshot: &Snapshot,
        cell: RegisterCell,
        entry: &SnapshotEntry,
    ) -> String {
        let (kind, addr) = cell;
        let at = |address: u16| snapshot.get(&(kind, address)).map(|e| e.value);
        let custom = self.custom_value(cell, entry.value, &at);
        self.interpreter.format_row(
            addr,
            entry.value,
            following(addr, self.interpreter.lookahead(), at),
            entry.time.as_deref().unwrap_or(NO_VALUE),
            None,
            custom.as_deref(),
            self.label(cell),
        )
    }

    fn differing_segments(&self, text: &str, other: &str) -> Vec<RowSegment> {
        self.interpreter
            .row_segments()
            .into_iter()
            .filter(|segment| {
                segment.name != Column::Time.name() && segment.text(text) != segment.text(other)
            })
            .collect()
    }
}

#[cfg(target_arch = "wasm32")]
const fn dropped_dump(_: &str, _: crate::config::AddressMode) -> PastedDump {
    PastedDump::Other
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use crate::app::{App, settle_until};
    use crate::config::{Column, Config, TimeMode};
    use crate::constants::NO_VALUE;
    use crate::register::{RegisterCell, RegisterType};
    use crate::snapshot::{DiffLine, DiffMark, DiffRow, Snapshot, SnapshotEntry, parse_dump};
    use crate::state::ConnectionStatus;
    use chrono::{DateTime, Utc};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Modifier;

    const VOLTAGE: RegisterCell = (RegisterType::Input, 0);
    const CURRENT: RegisterCell = (RegisterType::Input, 3);
    const POWER: RegisterCell = (RegisterType::Input, 8);
    const POWER_LOW: RegisterCell = (RegisterType::Input, 9);
    const UNREAD: RegisterCell = (RegisterType::Input, 300);
    const EARLIER: &str = "09:00:00.000";

    async fn read_app() -> App {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.read_mut().register_type = RegisterType::Input;
        app.refresh();
        settle_until(&mut app, |app| app.background_task.is_none(), "read").await;
        app
    }

    fn dumped(app: &App) -> Snapshot {
        parse_dump(&app.dump_csv(Utc::now()), app.interpreter.address_mode()).unwrap()
    }

    fn snapshot_with_changes(app: &App) -> Snapshot {
        let mut snapshot = dumped(app);
        snapshot.get_mut(&VOLTAGE).unwrap().value = 1234;
        snapshot.insert(
            UNREAD,
            SnapshotEntry {
                value: 7,
                time: None,
            },
        );
        snapshot
    }

    fn lines_of(
        app: &App,
        snapshot: &Snapshot,
        cells: &[RegisterCell],
        now: DateTime<Utc>,
    ) -> Vec<DiffLine> {
        let rows: Vec<DiffRow> = app
            .diff_of(snapshot)
            .rows(false)
            .iter()
            .copied()
            .filter(|row| cells.contains(&row.cell))
            .collect();
        app.diff_lines(snapshot, &rows, now)
    }

    fn named(app: &App, name: &str, text: &str) -> String {
        app.interpreter
            .row_segments()
            .into_iter()
            .find(|segment| segment.name == name)
            .expect("segment")
            .text(text)
    }

    fn marks(lines: &[DiffLine]) -> Vec<DiffMark> {
        lines.iter().map(|line| line.mark).collect()
    }

    #[tokio::test]
    async fn a_changed_cell_shows_the_snapshot_then_the_current_row() {
        let app = read_app().await;
        let mut snapshot = snapshot_with_changes(&app);
        snapshot.get_mut(&VOLTAGE).unwrap().time = Some(EARLIER.to_string());
        let now = Utc::now();
        let lines = lines_of(&app, &snapshot, &[VOLTAGE], now);
        assert_eq!(marks(&lines), vec![DiffMark::Before, DiffMark::After]);

        let (before, after) = (&lines[0], &lines[1]);
        assert_eq!(named(&app, "u16", &before.text), "1234");
        assert_eq!(
            named(&app, "custom", &before.text),
            "123.4 V",
            "custom from the snapshot"
        );
        assert_eq!(named(&app, "label", &before.text), "voltage L1");
        assert_eq!(after.text, app.cell_row(VOLTAGE, now).unwrap().0);

        let time = |text: &str| named(&app, Column::Time.name(), text);
        assert_eq!(time(&before.text), EARLIER);
        assert_ne!(time(&after.text), EARLIER, "the times differ");

        let emphasised: Vec<&str> = before.emphasis.iter().map(|segment| segment.name).collect();
        assert!(
            emphasised.contains(&"u16") && emphasised.contains(&"hex"),
            "{emphasised:?}"
        );
        assert!(
            !emphasised.contains(&Column::Time.name()),
            "the time never counts"
        );
        assert!(!emphasised.contains(&"label"));
        assert_eq!(before.emphasis, after.emphasis);
    }

    #[tokio::test]
    async fn the_snapshot_line_takes_its_neighbours_and_custom_words_from_the_snapshot() {
        let app = read_app().await;
        let mut snapshot = dumped(&app);
        snapshot.get_mut(&POWER).unwrap().value = 0x4500;
        snapshot.get_mut(&POWER_LOW).unwrap().value = 0x4142;
        let now = Utc::now();
        let lines = lines_of(&app, &snapshot, &[POWER], now);
        assert_eq!(marks(&lines), vec![DiffMark::Before, DiffMark::After]);

        let (before, after) = (&lines[0], &lines[1]);
        assert_eq!(named(&app, "ascii", &before.text), "E.AB");
        assert_eq!(named(&app, "custom", &before.text), "2052.08 kW");
        assert_eq!(after.text, app.cell_row(POWER, now).unwrap().0);
        assert_ne!(named(&app, "ascii", &after.text), "E.AB");
        let emphasised: Vec<&str> = before.emphasis.iter().map(|segment| segment.name).collect();
        assert!(
            emphasised.contains(&"ascii") && emphasised.contains(&"custom"),
            "{emphasised:?}"
        );
    }

    #[tokio::test]
    async fn same_and_unread_cells_take_one_line_each() {
        let app = read_app().await;
        let snapshot = snapshot_with_changes(&app);
        let now = Utc::now();
        let lines = lines_of(&app, &snapshot, &[CURRENT, UNREAD], now);
        assert_eq!(lines.len(), 2);

        assert_eq!(lines[0].mark, DiffMark::Same);
        assert_eq!(lines[0].text, app.cell_row(CURRENT, now).unwrap().0);
        assert!(lines[0].emphasis.is_empty());

        assert_eq!(lines[1].mark, DiffMark::Unread);
        assert_eq!(lines[1].text, app.interpreter.placeholder(UNREAD.1, None));

        let diff = app.diff_of(&snapshot);
        let summary = diff.summary;
        assert_eq!((summary.changed, summary.unread), (1, 1));
        assert_eq!(summary.same, app.read_count() - 1);
        assert_eq!(diff.rows(true).len(), 2, "one pair");
        assert_eq!(diff.rows(false).len(), snapshot.len() + 1);
    }

    #[tokio::test]
    async fn a_dump_saved_in_the_ago_mode_claims_no_age() {
        let mut app = read_app().await;
        app.interpreter.set_time_mode(TimeMode::Ago);
        let dump = app.dump_csv(Utc::now());
        assert!(dump.contains(",now,") || dump.contains(" ago,"), "{dump}");

        let mut snapshot = snapshot_with_changes(&app);
        assert!(
            snapshot.values().all(|entry| entry.time.is_none()),
            "ages are not kept"
        );
        let rows = app.diff_of(&snapshot).rows(true).to_vec();
        let lines = app.diff_lines(&snapshot, &rows, Utc::now());
        assert_eq!(named(&app, "time", &lines[0].text), NO_VALUE);
        let current = named(&app, "time", &lines[1].text);
        assert!(current == "now" || current.ends_with(" ago"), "{current}");

        snapshot.get_mut(&VOLTAGE).unwrap().time = Some(EARLIER.to_string());
        let lines = app.diff_lines(&snapshot, &rows, Utc::now());
        assert_eq!(named(&app, "time", &lines[0].text), EARLIER);
    }

    #[tokio::test]
    async fn scrolling_stops_once_the_last_line_reaches_the_bottom() {
        let mut app = read_app().await;
        let snapshot = snapshot_with_changes(&app);
        let len = app.diff_of(&snapshot).rows(false).len();
        app.open_diff_view(snapshot);
        app.layout.visible_rows = 4;

        app.diff_scroll(-3);
        assert_eq!(app.diff_view().unwrap().top, 0);
        app.diff_scroll(isize::MAX);
        assert_eq!(app.diff_view().unwrap().top, len - 4);
        app.diff_scroll(-1);
        assert_eq!(app.diff_view().unwrap().top, len - 5);

        app.diff_toggle_changed();
        let d = app.diff_view().unwrap();
        assert!(d.changed_only);
        assert_eq!(d.top, 0, "switching tabs starts from the top");
    }

    #[tokio::test]
    async fn a_read_finishing_while_the_diff_is_open_is_not_left_hanging() {
        let mut app = App::boot(Config::demo(), String::new()).await;
        app.paused = true;
        app.refresh();
        assert!(app.read().loading);
        app.open_diff_view(snapshot_with_changes(&read_app().await));
        assert_eq!(app.connection, ConnectionStatus::Reading);

        settle_until(&mut app, |app| app.background_task.is_none(), "read").await;
        assert_eq!(app.connection, ConnectionStatus::Connected);
        app.close_diff_view();
        assert!(!app.read().loading, "the read view is not stuck loading");
        assert!(app.read_log.is_empty(), "values read meanwhile are dropped");
    }

    fn screen(app: &mut App) -> String {
        crate::tui::test_util::draw_rows(140, 30, |frame| crate::tui::render(app, frame)).join("\n")
    }

    fn render_buffer(app: &mut App) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(140, 30)).unwrap();
        terminal
            .draw(|frame| crate::tui::render(app, frame))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn line_start(buffer: &Buffer, lead: &str) -> (u16, u16) {
        crate::tui::test_util::buffer_rows(buffer)
            .iter()
            .enumerate()
            .find_map(|(y, row)| {
                let at = row.find(lead)?;
                Some((row[..at].chars().count() as u16, y as u16))
            })
            .unwrap_or_else(|| panic!("no line starts with {lead:?}"))
    }

    fn lead(mark: char, address: u16) -> String {
        format!("{mark} I {address:>7} ")
    }

    #[tokio::test]
    async fn the_diff_screen_draws_both_tabs() {
        let mut app = read_app().await;
        let snapshot = snapshot_with_changes(&app);
        app.open_diff_view(snapshot);

        let all = screen(&mut app);
        assert!(all.contains("1 changed, 9 same, 1 unread"), "{all}");
        assert!(all.contains("All | Changed"), "{all}");
        for (mark, address) in [('-', 0), ('+', 0), ('=', 3), ('?', 300)] {
            assert!(
                all.contains(&lead(mark, address)),
                "{mark} {address}: {all}"
            );
        }
        assert!(all.contains("  T address"), "{all}");

        app.diff_toggle_changed();
        let changed = screen(&mut app);
        assert!(changed.contains(&lead('-', 0)), "{changed}");
        assert!(!changed.contains(&lead('=', 3)), "{changed}");

        let mut same = snapshot_with_changes(&app);
        same.retain(|&cell, _| cell == CURRENT);
        app.diff_view_mut().unwrap().snapshot = same;
        app.rediff();
        assert!(screen(&mut app).contains("no differences"));

        let unread = SnapshotEntry {
            value: 1,
            time: None,
        };
        app.diff_view_mut().unwrap().snapshot = [(UNREAD, unread)].into();
        app.rediff();
        let changed = screen(&mut app);
        assert!(
            changed.contains("not been read in this session"),
            "{changed}"
        );
        assert!(!changed.contains("no differences"), "{changed}");
        app.diff_toggle_changed();
        let all = screen(&mut app);
        assert!(all.contains(&lead('?', 300)), "{all}");
        assert!(all.contains("not been read in this session"), "{all}");
    }

    #[tokio::test]
    async fn differing_cells_are_reversed_on_screen_but_the_time_never_is() {
        let mut app = read_app().await;
        let mut snapshot = snapshot_with_changes(&app);
        snapshot.get_mut(&VOLTAGE).unwrap().time = Some(EARLIER.to_string());
        app.open_diff_view(snapshot);
        let buffer = render_buffer(&mut app);

        let segment = |name: &str| {
            app.interpreter
                .row_segments()
                .into_iter()
                .find(|segment| segment.name == name)
                .expect("segment")
        };
        let reversed = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::REVERSED);
        for mark in ['-', '+'] {
            let (x, y) = line_start(&buffer, &lead(mark, 0));
            let at = |name: &str| x + 4 + segment(name).start as u16;
            assert!(reversed(at("u16"), y), "{mark} u16");
            assert!(reversed(at("hex"), y), "{mark} hex");
            assert!(!reversed(at("time"), y), "{mark} time");
            assert!(!reversed(at("label"), y), "{mark} label");
            assert!(!reversed(x, y), "{mark} marker");
        }
        let (x, y) = line_start(&buffer, &lead('=', 3));
        assert!(!reversed(x + 4 + segment("u16").start as u16, y));
    }

    #[tokio::test]
    async fn same_lines_follow_the_zebra_striping_of_the_other_tables() {
        let mut app = read_app().await;
        let snapshot = snapshot_with_changes(&app);
        app.open_diff_view(snapshot);
        let theme = app.config.theme;
        let buffer = render_buffer(&mut app);

        let background = |mark: char, address: u16| {
            let (x, y) = line_start(&buffer, &lead(mark, address));
            buffer[(x + 4, y)].bg
        };
        assert_ne!(background('=', 1), theme.zebra, "the third line is even");
        assert_eq!(background('=', 2), theme.zebra, "the fourth line is odd");
        assert_ne!(background('=', 3), theme.zebra);
        assert_eq!(background('=', 4), theme.zebra);
        let (x, y) = line_start(&buffer, &lead('-', 0));
        assert_eq!(buffer[(x, y)].fg, theme.error);
    }

    #[tokio::test]
    async fn horizontal_scrolling_keeps_the_marker_type_and_address_frozen() {
        let mut app = read_app().await;
        app.interpreter.set_label_width(60);
        let snapshot = snapshot_with_changes(&app);
        app.open_diff_view(snapshot);
        let narrow = |app: &mut App| {
            crate::tui::test_util::draw_rows(120, 20, |frame| crate::tui::render(app, frame))
        };

        narrow(&mut app);
        assert!(
            app.layout.h_max_offset > 0,
            "the rows are wider than the screen"
        );
        app.diff_hscroll(true);
        assert_eq!(app.diff_view().unwrap().h_offset, 8);

        let rows = narrow(&mut app);
        let header = rows
            .iter()
            .find(|row| row.starts_with("  T address"))
            .expect("the header keeps its lead");
        assert!(!header.contains("time"), "{header}");
        let before = rows
            .iter()
            .find(|row| row.starts_with(&lead('-', 0)))
            .expect("the before line keeps its lead");
        assert!(before.contains("1234"), "{before}");
        assert!(rows[0].contains("< cols >"), "{}", rows[0]);
    }
}
