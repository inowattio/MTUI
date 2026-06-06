use crate::app::App;
use crate::state::SearchParams;
use crate::tui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};
use ratatui::Frame;

fn row_text(address: impl std::fmt::Display, kind: impl std::fmt::Display, label: &str) -> String {
    format!("{address: >5}  {kind: <8} {label}")
}

pub fn draw(
    params: &SearchParams,
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    _device: &str,
) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    let query_line = Line::from(vec![
        Span::styled("Search labels: ", theme.dim_style()),
        Span::styled(params.query.clone(), theme.accent_style()),
        Span::styled("_", theme.accent_style()),
        Span::styled(
            format!("   ({} match{})", params.matches.len(), if params.matches.len() == 1 { "" } else { "es" }),
            theme.dim_style(),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(query_line).alignment(Alignment::Left),
        rows[0],
    );

    // Report the list height so navigation can scroll correctly.
    let visible = rows[1].height.saturating_sub(3).max(1);
    app.visible_rows.set(visible);

    let header = row_text("addr", "type", "label");

    let table = if params.matches.is_empty() {
        let message = if app.config.labels.holdings.is_empty() && app.config.labels.inputs.is_empty() {
            "No labels defined.".to_string()
        } else {
            "No matching labels.".to_string()
        };
        Table::new(
            vec![Row::new([Cell::from(message)]).style(theme.dim_style())],
            [Constraint::Percentage(100)],
        )
        .header(Row::new([Cell::from(header)]).style(theme.header_style()))
        .block(theme.panel("Labels"))
    } else {
        let len = params.matches.len();
        let top = (params.top as usize).min(len.saturating_sub(1));
        let end = (top + visible as usize).min(len);

        let mut table_rows = Vec::with_capacity(end - top);
        for i in top..end {
            let ((kind, address), text) = &params.matches[i];
            let line = row_text(address, format!("{kind:?}"), text);
            let style = if i as u16 == params.selected {
                theme.selected_style()
            } else if (i - top) % 2 == 1 {
                theme.zebra_style()
            } else {
                theme.base()
            };
            table_rows.push(Row::new([Cell::from(line)]).style(style));
        }

        Table::new(table_rows, [Constraint::Percentage(100)])
            .header(Row::new([Cell::from(header)]).style(theme.header_style()))
            .block(theme.panel("Labels"))
    };

    frame.render_widget(table, rows[1]);
}
