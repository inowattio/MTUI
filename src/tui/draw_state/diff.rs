use crate::app::App;
use crate::snapshot::{DiffLine, DiffMark};
use crate::state::DiffViewParams;
use crate::tui::draw_state::dim_line;
use crate::tui::rows_table::{RowsTable, TableRow, max_h_offset};
use crate::tui::theme::Theme;
use chrono::Utc;
use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const TABS: [&str; 2] = ["All", "Changed"];
const HEADER_LEAD: &str = "  T ";
const LEAD: usize = HEADER_LEAD.len();
const NO_DIFFERENCES: &str = "no differences";
const NOTHING_READ: &str = "these registers have not been read in this session";

pub fn summary(params: &DiffViewParams) -> String {
    let summary = params.diff.summary;
    format!(
        "{} changed, {} same, {} unread",
        summary.changed, summary.same, summary.unread
    )
}

fn mark_style(theme: &Theme, mark: DiffMark, zebra: bool) -> Style {
    match mark {
        DiffMark::Before => theme.err_style(),
        DiffMark::After => theme.ok_style(),
        DiffMark::Same => theme.row_style(zebra, false),
        DiffMark::Unread => theme.dim_style(),
    }
}

fn legend(theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(" - snapshot", mark_style(theme, DiffMark::Before, false)),
        Span::styled("  + current ", mark_style(theme, DiffMark::After, false)),
    ])
    .right_aligned()
}

fn table_row(line: DiffLine, zebra: bool, theme: &Theme) -> TableRow {
    let style = mark_style(theme, line.mark, zebra);
    let text = format!(
        "{} {:<2}{}",
        line.mark.symbol(),
        line.cell.0.marker(),
        line.text
    );
    if line.emphasis.is_empty() {
        return TableRow::plain(text, style);
    }
    let chars: Vec<char> = text.chars().collect();
    let piece = |from: usize, to: usize| -> String {
        chars[from.min(chars.len())..to.min(chars.len())]
            .iter()
            .collect()
    };
    let mut spans = Vec::with_capacity(line.emphasis.len() * 2 + 1);
    let mut at = 0;
    for segment in &line.emphasis {
        let (start, end) = (LEAD + segment.start, LEAD + segment.end());
        spans.push(Span::styled(piece(at, start), style));
        spans.push(Span::styled(
            piece(start, end),
            style.add_modifier(Modifier::REVERSED),
        ));
        at = end;
    }
    spans.push(Span::styled(piece(at, chars.len()), style));
    TableRow { spans, style }
}

pub fn draw(params: &DiffViewParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let rows = params.diff.rows(params.changed_only);
    let all_unread = params.diff.summary.all_unread();
    let footnote = (all_unread && !rows.is_empty()).then_some(NOTHING_READ);
    let notice = rows.is_empty().then_some(if all_unread {
        NOTHING_READ
    } else {
        NO_DIFFERENCES
    });

    let mut block = theme
        .tabbed_panel(&TABS, usize::from(params.changed_only))
        .title_top(legend(theme));
    if let Some(text) = footnote {
        block = block.title_bottom(dim_line(theme, format!(" {text} ")));
    }

    let visible = area
        .height
        .saturating_sub(2 + u16::from(footnote.is_some()))
        .max(1);
    app.visible_rows.set(visible);

    let max_top = rows.len().saturating_sub(visible as usize);
    let top = params.top.min(max_top);
    let end = (top + visible as usize).min(rows.len());
    let lines = app.diff_lines(&params.snapshot, &rows[top..end], Utc::now());

    let header = format!("{HEADER_LEAD}{}", app.interpreter.header());
    let prefix = LEAD + app.interpreter.prefix_width() as usize;
    let widths = lines
        .iter()
        .map(|line| LEAD + line.text.chars().count())
        .chain(std::iter::once(header.chars().count()));
    let max_offset = max_h_offset(widths, prefix, area.width as usize);
    app.h_max_offset.set(max_offset);
    let h_off = params.h_offset.min(max_offset);

    let table_rows = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| table_row(line, (top + i) % 2 == 1, theme))
        .collect();
    frame.render_widget(
        RowsTable::new(block, header, theme.header_style(), table_rows)
            .hscroll(prefix as u16, h_off),
        area,
    );

    let body_height = area.height.saturating_sub(2);
    if rows.is_empty()
        && body_height > 0
        && let Some(text) = notice
    {
        let notice_area = Rect {
            x: area.x,
            y: area.y + 2 + body_height / 3,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(dim_line(theme, text)).alignment(Alignment::Center),
            notice_area,
        );
    }
}
