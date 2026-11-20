use crate::app::App;
use crate::import::catalog::Item;
use crate::state::{LibrarySource, LibraryViewParams, ScreenLayout, SourceStatus, scroll_window};
use crate::tui::draw_state::popups::truncate;
use crate::tui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

const VENDOR_HEADER: &str = "Vendor";
const MODEL_HEADER: &str = "Model";
const REGISTERS_HEADER: &str = "Registers";
const STATUS_HEADER: &str = "Status";
const SOURCE_HEADER: &str = "Source";
const LOCATION_HEADER: &str = "Location";
const ITEMS_HEADER: &str = "Items";
const STATUS_W: usize = 11;
const GAP: &str = "  ";
const FIXED_W: usize = 1 + 3 * GAP.len() + REGISTERS_HEADER.len() + STATUS_W;
const VENDOR_CAP: usize = 24;
const LIST_CHROME: u16 = 2;
const DETAIL_H: u16 = 6;
const MIN_LIST_ROWS: u16 = 3;
const NO_MATCHES: &str = " Nothing matches the search.";
const ON_DEVICE: &str = "on device";
const STATE_HEADER: &str = "Status";
const ITEMS_W: usize = 7;

const fn detail_height(height: u16) -> u16 {
    if height >= DETAIL_H + LIST_CHROME + MIN_LIST_ROWS {
        DETAIL_H
    } else {
        0
    }
}

const fn noun(source: LibrarySource) -> &'static str {
    match source {
        LibrarySource::Mbmap => "maps",
        LibrarySource::SunSpec => "models",
        LibrarySource::Solarman => "definitions",
    }
}

const fn detail_title(source: Option<LibrarySource>) -> &'static str {
    match source {
        None => " Source",
        Some(LibrarySource::Mbmap) => " Map",
        Some(LibrarySource::SunSpec) => " Model",
        Some(LibrarySource::Solarman) => " Definition",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Columns {
    vendor: usize,
    model: usize,
}

fn columns(width: usize, vendor_max: usize) -> Columns {
    let available = width.saturating_sub(FIXED_W);
    let vendor = vendor_max
        .max(VENDOR_HEADER.len())
        .min(VENDOR_CAP)
        .min(available / 3);
    Columns {
        vendor,
        model: available - vendor,
    }
}

fn widest_vendor(items: &[Item], matches: &[usize]) -> usize {
    matches
        .iter()
        .filter_map(|&i| items.get(i))
        .map(|item| item.vendor.chars().count())
        .max()
        .unwrap_or(0)
}

fn cell(text: &str, width: usize) -> String {
    let text: String = truncate(text, width).chars().take(width).collect();
    format!("{text:<width$}")
}

fn registers_text(registers: Option<usize>) -> String {
    registers.map_or_else(|| "-".to_string(), |count| count.to_string())
}

fn row_text(item: &Item, status: &str, cols: Columns) -> String {
    format!(
        " {}{GAP}{}{GAP}{:>w$}{GAP}{}",
        cell(&item.vendor, cols.vendor),
        cell(&item.model, cols.model),
        registers_text(item.registers),
        cell(status, STATUS_W),
        w = REGISTERS_HEADER.len(),
    )
}

fn header_text(cols: Columns) -> String {
    format!(
        " {}{GAP}{}{GAP}{REGISTERS_HEADER}{GAP}{}",
        cell(VENDOR_HEADER, cols.vendor),
        cell(MODEL_HEADER, cols.model),
        cell(STATUS_HEADER, STATUS_W),
    )
}

fn state_w() -> usize {
    SourceStatus::ALL
        .iter()
        .map(|status| status.label().len())
        .max()
        .unwrap_or(0)
}

fn source_columns(width: usize) -> (usize, usize) {
    let name = LibrarySource::ALL
        .iter()
        .map(|source| source.name().len())
        .max()
        .unwrap_or(0)
        .max(SOURCE_HEADER.len());
    let fixed = 1 + 3 * GAP.len() + name + state_w() + ITEMS_W;
    (name, width.saturating_sub(fixed))
}

fn source_cells(name: &str, location: &str, state: &str, items: &str, width: usize) -> [String; 3] {
    let (name_w, location_w) = source_columns(width);
    [
        format!(
            " {}{GAP}{}{GAP}",
            cell(name, name_w),
            cell(location, location_w)
        ),
        cell(state, state_w()),
        format!("{GAP}{:>ITEMS_W$}", truncate(items, ITEMS_W)),
    ]
}

fn status_text(status: SourceStatus, saved: Option<usize>) -> String {
    match (status, saved) {
        (SourceStatus::Cached, Some(saved)) => format!("{} ({saved})", status.label()),
        _ => status.label().to_string(),
    }
}

fn source_items(app: &App, source: LibrarySource) -> String {
    app.library_count(source)
        .map_or_else(|| "-".to_string(), |count| count.to_string())
}

fn status_style(theme: &Theme, status: SourceStatus) -> Style {
    match status {
        SourceStatus::NotDownloaded => theme.dim_style(),
        SourceStatus::Loading | SourceStatus::Downloading => theme.accent_style(),
        SourceStatus::Cached | SourceStatus::Loaded => theme.ok_style(),
        SourceStatus::Incomplete | SourceStatus::OfflineCopy => theme.warn_style(),
        SourceStatus::FailedDownload | SourceStatus::FailedLoad => theme.err_style(),
    }
}

fn item_status(app: &App, item: &Item) -> String {
    match item.sunspec_id().and_then(|id| app.sunspec_found(id)) {
        Some(_) => ON_DEVICE.to_string(),
        None => item.status.clone(),
    }
}

pub fn counter(params: &LibraryViewParams, app: &App) -> String {
    match params.source {
        None => format!("{} sources", app.library_sources().len()),
        Some(source) => count_label(
            &params.query,
            params.matches.len(),
            app.library_items().len(),
            noun(source),
        ),
    }
}

fn count_label(query: &str, matches: usize, total: usize, noun: &str) -> String {
    if query.trim().is_empty() {
        format!("{total} {noun}")
    } else {
        format!("{matches} of {total} {noun}")
    }
}

fn top_line(theme: &Theme, params: &LibraryViewParams, app: &App) -> Line<'static> {
    let mut spans = match &params.prompt {
        Some(prompt) => {
            let id = params
                .selected_entry()
                .and_then(|i| app.library_items().get(i))
                .and_then(Item::sunspec_id)
                .map_or_else(String::new, |id| format!(" {id}"));
            vec![
                Span::styled(
                    format!(" Start address of SunSpec model{id}: "),
                    theme.accent_style(),
                ),
                Span::styled(prompt.clone(), theme.base()),
                Span::styled("_", theme.accent_style()),
            ]
        }
        None => vec![
            Span::styled(" > ", theme.accent_style()),
            Span::styled(params.query.clone(), theme.base()),
            Span::styled("_", theme.accent_style()),
        ],
    };
    if let Some(status) = &params.status {
        spans.push(Span::raw("   "));
        spans.extend(theme.status_line(status).spans);
    }
    Line::from(spans)
}

pub fn draw(
    params: &LibraryViewParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    layout: &mut ScreenLayout,
) {
    let [list_area, detail_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(detail_height(area.height)),
    ])
    .areas(area);
    let visible = list_area.height.saturating_sub(LIST_CHROME).max(1);
    layout.visible_rows = visible;
    let width = usize::from(list_area.width);

    let mut lines = Vec::new();
    let detail = match params.source {
        None => {
            let sources = app.library_sources();
            let highlighted = params.highlighted_source(&sources);
            lines.extend(source_lines(theme, &sources, highlighted, app, width));
            let mut detail =
                highlighted.map_or_else(Vec::new, |source| source_detail(theme, source, app));
            if let Some(status) = &params.status {
                detail.insert(0, theme.status_line(status));
            }
            detail
        }
        Some(_) => {
            lines.push(top_line(theme, params, app));
            lines.extend(item_lines(theme, params, app, width, visible));
            let item = params
                .selected_entry()
                .and_then(|i| app.library_items().get(i));
            item_detail(theme, app, item)
        }
    };
    frame.render_widget(Paragraph::new(lines), list_area);

    if detail_area.is_empty() {
        return;
    }
    let block = theme.panel(detail_title(params.source));
    let detail_inner = block.inner(detail_area).inner(Margin::new(1, 0));
    frame.render_widget(block, detail_area);
    frame.render_widget(
        Paragraph::new(detail).wrap(Wrap { trim: true }),
        detail_inner,
    );
}

fn source_lines(
    theme: &Theme,
    sources: &[LibrarySource],
    highlighted: Option<LibrarySource>,
    app: &App,
    width: usize,
) -> Vec<Line<'static>> {
    let header = source_cells(
        SOURCE_HEADER,
        LOCATION_HEADER,
        STATE_HEADER,
        ITEMS_HEADER,
        width,
    );
    let mut lines = vec![Line::styled(header.concat(), theme.header_style())];
    for (i, &source) in sources.iter().enumerate() {
        let status = app.library_status(source);
        let [left, state, items] = source_cells(
            source.name(),
            source.location(),
            &status_text(status, app.library_saved(source)),
            &source_items(app, source),
            width,
        );
        let line = if Some(source) == highlighted {
            Line::styled(format!("{left}{state}{items}"), theme.selected_style())
        } else {
            let style = theme.row_style(i % 2 == 1, None);
            Line::from(vec![
                Span::styled(left, style),
                Span::styled(state, style.patch(status_style(theme, status))),
                Span::styled(items, style),
            ])
        };
        lines.push(line);
    }
    lines
}

fn item_lines(
    theme: &Theme,
    params: &LibraryViewParams,
    app: &App,
    width: usize,
    visible: u16,
) -> Vec<Line<'static>> {
    let items = app.library_items();
    let cols = columns(width, widest_vendor(items, &params.matches));
    let mut lines = vec![Line::styled(header_text(cols), theme.header_style())];

    if params.matches.is_empty() && !items.is_empty() {
        lines.push(Line::styled(NO_MATCHES, theme.dim_style()));
    }

    let len = params.matches.len().min(usize::from(u16::MAX)) as u16;
    let (mut selected, mut top) = (params.selected, params.top);
    scroll_window(&mut selected, &mut top, visible, len);
    let top = usize::from(top);
    let end = (top + usize::from(visible)).min(usize::from(len));
    for (i, &index) in params.matches[top..end].iter().enumerate() {
        let Some(item) = items.get(index) else {
            continue;
        };
        let position = top + i;
        let style = if position == usize::from(selected) {
            theme.selected_style()
        } else {
            theme.row_style(position % 2 == 1, None)
        };
        lines.push(Line::styled(
            row_text(item, &item_status(app, item), cols),
            style,
        ));
    }
    lines
}

fn source_detail(theme: &Theme, source: LibrarySource, app: &App) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            source.name(),
            theme.accent_style().add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("{GAP}{}", source.location()), theme.dim_style()),
    ])];
    lines.push(Line::styled(source.description(), theme.base()));
    if source == LibrarySource::SunSpec
        && let Some(note) = app.sunspec_scan_note()
    {
        lines.push(Line::styled(note, theme.dim_style()));
    }
    lines
}

fn item_detail(theme: &Theme, app: &App, item: Option<&Item>) -> Vec<Line<'static>> {
    let Some(item) = item else {
        return Vec::new();
    };
    let mut title = vec![Span::styled(
        item.title(),
        theme.accent_style().add_modifier(Modifier::BOLD),
    )];
    if let Some(registers) = item.registers {
        title.push(Span::styled(
            format!("{GAP}{registers} registers"),
            theme.dim_style(),
        ));
    }
    if !item.status.is_empty() {
        title.push(Span::styled(
            format!("{GAP}{}", item.status),
            theme.dim_style(),
        ));
    }
    title.push(Span::styled(
        format!("{GAP}{}", item.path),
        theme.dim_style(),
    ));
    let mut lines = vec![Line::from(title)];
    if let Some(id) = item.sunspec_id() {
        let note = match app.sunspec_found(id) {
            Some((start, length)) => Some((
                format!(
                    "On the device at {} with {length} registers after ID and L",
                    app.interpreter.address_mode().display(start)
                ),
                theme.ok_style(),
            )),
            None => app
                .sunspec_scan_note()
                .map(|note| (note, theme.dim_style())),
        };
        if let Some((note, style)) = note {
            lines.push(Line::styled(note, style));
        }
    }
    let aliases: Vec<&str> = item
        .models
        .iter()
        .map(String::as_str)
        .filter(|model| *model != item.model)
        .collect();
    if !aliases.is_empty() {
        lines.push(Line::styled(
            format!("Also: {}", aliases.join(", ")),
            theme.dim_style(),
        ));
    }
    if !item.description.is_empty() {
        lines.push(Line::styled(item.description.clone(), theme.base()));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::{
        Columns, DETAIL_H, FIXED_W, LIST_CHROME, MIN_LIST_ROWS, VENDOR_CAP, columns, detail_height,
        header_text, row_text, source_cells,
    };
    use crate::import::catalog::{Item, ItemKind};
    use crate::state::SourceStatus;

    fn entry() -> Item {
        Item {
            vendor: "ABB".to_string(),
            model: "ACS880".to_string(),
            models: Vec::new(),
            description: "Industrial drive".to_string(),
            status: "official".to_string(),
            registers: Some(1888),
            path: "maps/abb/acs880.mbmap.json".to_string(),
            url: String::new(),
            kind: ItemKind::Solarman,
        }
    }

    #[test]
    fn short_screens_drop_the_detail_pane_before_the_list() {
        let roomy = DETAIL_H + LIST_CHROME + MIN_LIST_ROWS;
        assert_eq!(detail_height(roomy), DETAIL_H);
        assert_eq!(detail_height(roomy - 1), 0);
        assert_eq!(detail_height(0), 0);
    }

    #[test]
    fn the_model_column_fills_the_rest_of_the_width() {
        let cols = columns(120, 3);
        assert_eq!(cols.vendor, "Vendor".len());
        assert_eq!(FIXED_W + cols.vendor + cols.model, 120);
        assert_eq!(row_text(&entry(), "official", cols).chars().count(), 120);
        assert_eq!(header_text(cols).chars().count(), 120);
    }

    #[test]
    fn long_vendors_are_capped() {
        assert_eq!(columns(200, 80).vendor, VENDOR_CAP);
    }

    #[test]
    fn narrow_screens_shrink_the_names() {
        assert_eq!(
            columns(FIXED_W + 30, 40),
            Columns {
                vendor: 10,
                model: 20
            }
        );
        assert_eq!(
            columns(10, 40),
            Columns {
                vendor: 0,
                model: 0
            }
        );
    }

    #[test]
    fn the_counter_shows_matches_only_while_searching() {
        assert_eq!(super::count_label("", 776, 776, "maps"), "776 maps");
        assert_eq!(super::count_label("  ", 776, 776, "maps"), "776 maps");
        assert_eq!(
            super::count_label("abb acs", 4, 776, "maps"),
            "4 of 776 maps"
        );
        assert_eq!(
            super::count_label("inv", 0, 114, "models"),
            "0 of 114 models"
        );
    }

    #[test]
    fn cells_never_overflow_their_column() {
        assert_eq!(super::cell("Vendor", 2), "..");
        assert_eq!(super::cell("AB", 2), "AB");
        assert_eq!(super::cell("A", 2), "A ");
        assert_eq!(super::cell("ABB", 0), "");
        assert_eq!(super::cell("Schneider", 6), "Sch...");
    }

    #[test]
    fn rows_line_up_with_the_header() {
        let cols = columns(100, 3);
        let header = header_text(cols);
        let row = row_text(&entry(), "official", cols);
        assert_eq!(row.find("ACS880"), header.find("Model"));
        assert_eq!(row.find("official"), header.find("Status"));
        assert_eq!(
            row.find("1888").map(|i| i + 4),
            header.find("Registers").map(|i| i + "Registers".len())
        );
    }

    #[test]
    fn rows_end_at_the_status_without_the_description() {
        let cols = columns(200, 3);
        assert!(
            row_text(&entry(), "official", cols)
                .trim_end()
                .ends_with("official")
        );
        assert!(!row_text(&entry(), "official", cols).contains("Industrial"));
        assert!(header_text(cols).trim_end().ends_with("Status"));
    }

    #[test]
    fn the_status_column_fits_every_sunspec_model_id() {
        let row = row_text(&entry(), &format!("model {}", u16::MAX), columns(100, 3));
        assert!(row.trim_end().ends_with("model 65535"));
    }

    #[test]
    fn unknown_register_counts_show_a_dash() {
        let mut item = entry();
        item.registers = None;
        let cols = columns(100, 3);
        let row = row_text(&item, "", cols);
        let header = header_text(cols);
        assert_eq!(
            row.find(" - ").map(|i| i + 2),
            header.find("Registers").map(|i| i + "Registers".len())
        );
    }

    #[test]
    fn rows_show_the_given_status_instead_of_the_items_own() {
        let row = row_text(&entry(), "on device", columns(100, 3));
        assert!(row.trim_end().ends_with("on device"));
    }

    #[test]
    fn source_rows_fill_the_width_and_right_align_the_count() {
        let row =
            source_cells("SunSpec", "github.com/sunspec/models", "Cached", "114", 100).concat();
        assert_eq!(row.chars().count(), 100);
        assert!(row.ends_with(" 114"));
        let narrow = source_cells("x", "y", "Failed to download", "9999999", 20).concat();
        assert!(narrow.contains("Failed to download"));
    }

    #[test]
    fn cached_sources_count_their_saved_items() {
        assert_eq!(
            super::status_text(SourceStatus::Cached, Some(1)),
            "Cached (1)"
        );
        assert_eq!(
            super::status_text(SourceStatus::Cached, Some(0)),
            "Cached (0)"
        );
        assert_eq!(super::status_text(SourceStatus::Cached, None), "Cached");
        assert_eq!(
            super::status_text(SourceStatus::Incomplete, Some(3)),
            "Incomplete"
        );
        let row = source_cells(
            "mbmap",
            "x",
            &super::status_text(SourceStatus::Cached, Some(776)),
            "776",
            100,
        )
        .concat();
        assert!(row.contains("Cached (776)"));
    }

    #[test]
    fn every_status_fits_its_column_and_lines_up_with_the_header() {
        let header = source_cells("Source", "Location", "Status", "Items", 100).concat();
        for status in SourceStatus::ALL {
            let row = source_cells("Solarman", "github.com/x", status.label(), "30", 100).concat();
            assert_eq!(row.find(status.label()), header.find("Status"));
            assert_eq!(row.chars().count(), 100);
        }
    }
}
