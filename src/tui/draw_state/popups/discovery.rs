use crate::app::App;
use crate::state::{DiscoveryColumn, DiscoveryField, DiscoveryParams, InterfaceKind};
use crate::tui::draw_state::{dim_line, edit_value, field_row, marker};
use crate::tui::hints::{self, Hint};
use crate::tui::theme::{Theme, spinner_frame};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::net::Ipv4Addr;

const LEFT_W: u16 = 46;
const DIVIDER_W: u16 = 3;
const SIDE_W: u16 = 36;
const COMMON_LABEL: usize = 22;
const SIDE_LABEL: usize = 10;
const LIST_ROWS: usize = 6;

pub fn draw(params: &DiscoveryParams, app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let kb = &app.config.keybinds;
    let field = params.current_field();
    let has_side = !params.side_fields().is_empty();
    let blocked = blocked_reason(params);

    let left = common_lines(params, field, blocked, theme);
    let right = side_lines(params, field, app, theme);

    let action = match field {
        DiscoveryField::ScanNetwork => "Scan",
        DiscoveryField::Port(_) => "Use port",
        DiscoveryField::Found(_) => "Use address",
        _ => "Connect",
    };
    let (footer_w, footer) = if has_side {
        let items = [
            Hint::pair(kb.move_up, kb.move_down, "Move"),
            Hint::key(kb.switch_view, "Section"),
            Hint::key(kb.action, action),
            Hint::key(kb.exit, "Back"),
        ];
        (hints::width(&items) as u16, hints::footer(theme, items))
    } else {
        let items = [
            Hint::pair(kb.move_up, kb.move_down, "Move"),
            Hint::key(kb.action, action),
            Hint::key(kb.exit, "Back"),
        ];
        (hints::width(&items) as u16, hints::footer(theme, items))
    };

    let mut tail: Vec<Line> = Vec::new();
    super::push_status(&mut tail, theme, params.status.as_ref());
    tail.push(Line::default());
    tail.push(footer);

    let body_w = if has_side {
        LEFT_W + DIVIDER_W + SIDE_W
    } else {
        LEFT_W
    };
    let width = (body_w + 2).max(footer_w);
    let body_rows = left.len().max(right.len()) as u16;
    let height = body_rows + tail.len() as u16 + 2;
    let rect = super::centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    let block = theme
        .panel(" Connection")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme.bg));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let [body, tail_area] =
        Layout::vertical([Constraint::Length(body_rows), Constraint::Min(0)]).areas(inner);
    if has_side {
        let [l, divider, r] = Layout::horizontal([
            Constraint::Length(LEFT_W),
            Constraint::Length(DIVIDER_W),
            Constraint::Min(0),
        ])
        .areas(body);
        frame.render_widget(Paragraph::new(left), l);
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(theme.dim_style()),
            divider,
        );
        frame.render_widget(Paragraph::new(right), r);
    } else {
        frame.render_widget(Paragraph::new(left), body);
    }
    frame.render_widget(Paragraph::new(tail), tail_area);
}

fn blocked_reason(p: &DiscoveryParams) -> Option<&'static str> {
    match p.interface {
        InterfaceKind::Network => p.ip.parse::<Ipv4Addr>().is_err().then_some("invalid IP"),
        InterfaceKind::Wired => p.serial_path().is_none().then_some("no serial port"),
        InterfaceKind::Mock => None,
    }
}

fn common_lines(
    p: &DiscoveryParams,
    current: DiscoveryField,
    blocked: Option<&'static str>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let focused = p.column == DiscoveryColumn::Common;
    let mut lines = Vec::with_capacity(DiscoveryParams::COMMON.len() + 1);
    for field in DiscoveryParams::COMMON {
        let selected = focused && field == current;
        if field == DiscoveryField::Connect {
            lines.push(Line::default());
            let reason = blocked
                .map(|reason| Span::styled(format!("   \u{2717} {reason}"), theme.err_style()));
            lines.push(button_line(
                theme,
                "Connect",
                selected,
                blocked.is_some(),
                reason,
            ));
            continue;
        }
        let (label, value, cyclable) = common_view(p, field);
        let value = edit_value(value, selected, cyclable);
        lines.push(row(theme, label, COMMON_LABEL, value, selected, None));
    }
    lines
}

fn common_view(p: &DiscoveryParams, field: DiscoveryField) -> (&'static str, String, bool) {
    match field {
        DiscoveryField::Interface => {
            let name = match p.interface {
                InterfaceKind::Mock => "Mock",
                InterfaceKind::Wired => "Wired (serial)",
                InterfaceKind::Network => "Network (TCP)",
            };
            ("Interface", name.to_string(), true)
        }
        DiscoveryField::SlaveId => ("Slave ID", p.slave_id.to_string(), false),
        DiscoveryField::ConnectTimeout => (
            "Connect timeout (ms)",
            p.connect_timeout_ms.to_string(),
            false,
        ),
        DiscoveryField::CommandTimeout => (
            "Command timeout (ms)",
            p.command_timeout_ms.to_string(),
            false,
        ),
        DiscoveryField::BetweenCommands => (
            "Between commands (ms)",
            p.between_commands_ms.to_string(),
            false,
        ),
        DiscoveryField::WordOrder => ("Word order", format!("{:?}", p.word_order), true),
        _ => ("", String::new(), false),
    }
}

fn side_lines(
    p: &DiscoveryParams,
    current: DiscoveryField,
    app: &App,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let focused = p.column == DiscoveryColumn::Side;
    let selected = |field: DiscoveryField| focused && field == current;
    let mut lines: Vec<Line> = Vec::new();

    match p.interface {
        InterfaceKind::Mock => {}
        InterfaceKind::Wired => {
            lines.push(section_title(theme, "SERIAL PORTS"));
            if p.ports.is_empty() {
                lines.push(dim_line(theme, "   (no serial ports found)"));
            } else {
                let cursor = match current {
                    DiscoveryField::Port(i) if focused => Some(i),
                    _ => None,
                };
                let chosen = (!p.custom_path_active()).then_some(p.port_index as usize);
                push_list(&mut lines, theme, &p.ports, cursor, true, chosen);
            }
            lines.push(custom_path_row(
                p,
                selected(DiscoveryField::CustomPath),
                theme,
            ));
            lines.push(Line::default());
            let serial = [
                (DiscoveryField::Baud, "Baud", p.baud_rate.to_string()),
                (
                    DiscoveryField::DataBits,
                    "Data bits",
                    format!("{:?}", p.data_bits),
                ),
                (DiscoveryField::Parity, "Parity", format!("{:?}", p.parity)),
                (
                    DiscoveryField::StopBits,
                    "Stop bits",
                    format!("{:?}", p.stop_bits),
                ),
            ];
            for (field, label, value) in serial {
                let on = selected(field);
                lines.push(row(
                    theme,
                    label,
                    SIDE_LABEL,
                    edit_value(value, on, true),
                    on,
                    None,
                ));
            }
        }
        InterfaceKind::Network => {
            lines.push(section_title(theme, "NETWORK"));
            let ip_style = p.ip.parse::<Ipv4Addr>().is_err().then(|| theme.err_style());
            let on = selected(DiscoveryField::Ip);
            lines.push(row(
                theme,
                "IP",
                SIDE_LABEL,
                edit_value(p.ip.clone(), on, false),
                on,
                ip_style,
            ));
            let on = selected(DiscoveryField::NetPort);
            lines.push(row(
                theme,
                "Port",
                SIDE_LABEL,
                edit_value(p.net_port.to_string(), on, false),
                on,
                None,
            ));
            lines.push(Line::default());

            let suffix = if let Some((done, total)) = app.scan_progress() {
                Some(Span::styled(
                    format!("   {} {done}/{total}", spinner_frame(app.frame)),
                    theme.warn_style(),
                ))
            } else if p.found.is_empty() {
                None
            } else {
                Some(Span::styled(
                    format!("   {} found", p.found.len()),
                    theme.dim_style(),
                ))
            };
            lines.push(button_line(
                theme,
                "Scan network",
                selected(DiscoveryField::ScanNetwork),
                false,
                suffix,
            ));

            if !p.found.is_empty() {
                lines.push(Line::default());
                lines.push(section_title(theme, "FOUND"));
                let cursor = match current {
                    DiscoveryField::Found(i) if focused => Some(i),
                    _ => None,
                };
                push_list(&mut lines, theme, &p.found, cursor, false, None);
            }
        }
    }
    lines
}

fn push_list(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    items: &[String],
    cursor: Option<usize>,
    radio: bool,
    chosen: Option<usize>,
) {
    let len = items.len();
    let anchor = cursor.or(chosen).unwrap_or(0);
    let top = anchor
        .saturating_sub(LIST_ROWS - 1)
        .min(len.saturating_sub(LIST_ROWS));
    let end = (top + LIST_ROWS).min(len);
    if top > 0 {
        lines.push(hints::more(theme, top, 0));
    }
    for (i, item) in items.iter().enumerate().take(end).skip(top) {
        let selected = cursor == Some(i);
        let mark = match (radio, chosen) {
            (false, _) => "",
            (true, Some(c)) if c == i => "\u{25cf} ",
            (true, _) => "\u{25cb} ",
        };
        let room = (SIDE_W as usize).saturating_sub(2 + mark.chars().count() + 1);
        let style = if selected {
            theme.selected_style()
        } else if chosen == Some(i) {
            theme.accent_style()
        } else {
            theme.base()
        };
        lines.push(Line::from(vec![
            Span::styled(marker(selected), theme.dim_style()),
            Span::styled(format!("{mark}{}", truncate(item, room)), style),
        ]));
    }
    if end < len {
        lines.push(hints::more(theme, 0, len - end));
    }
}

fn custom_path_row(p: &DiscoveryParams, selected: bool, theme: &Theme) -> Line<'static> {
    const LABEL: &str = "Other: ";
    let active = p.custom_path_active();
    let room = (SIDE_W as usize).saturating_sub(2 + 2 + LABEL.len() + 2);
    let (value, style) = if selected {
        (
            format!("{}_", truncate_start(&p.custom_path, room)),
            theme.selected_style(),
        )
    } else if active {
        (truncate(&p.custom_path, room), theme.accent_style())
    } else {
        ("type a path".to_string(), theme.dim_style())
    };
    let mark_style = if active {
        theme.accent_style()
    } else {
        theme.base()
    };
    Line::from(vec![
        Span::styled(marker(selected), theme.dim_style()),
        Span::styled(if active { "\u{25cf} " } else { "\u{25cb} " }, mark_style),
        Span::styled(LABEL, theme.dim_style()),
        Span::styled(value, style),
    ])
}

fn truncate_start(text: &str, room: usize) -> String {
    let count = text.chars().count();
    if count <= room {
        return text.to_string();
    }
    let keep = room.saturating_sub(1);
    let tail: String = text.chars().skip(count - keep).collect();
    format!("\u{2026}{tail}")
}

fn truncate(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let mut shown: String = text.chars().take(room.saturating_sub(1)).collect();
    shown.push('\u{2026}');
    shown
}

fn section_title(theme: &Theme, text: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {text}"), theme.header_style()))
}

fn row(
    theme: &Theme,
    label: &str,
    width: usize,
    value: String,
    selected: bool,
    value_style: Option<Style>,
) -> Line<'static> {
    let mut line = field_row(theme, label, width, value, selected);
    if let Some(style) = value_style
        && !selected
        && let Some(span) = line.spans.last_mut()
    {
        span.style = style;
    }
    line
}

fn button_line(
    theme: &Theme,
    label: &str,
    selected: bool,
    disabled: bool,
    suffix: Option<Span<'static>>,
) -> Line<'static> {
    let style = if disabled {
        theme.dim_style()
    } else if selected {
        theme.selected_style()
    } else {
        theme.accent_style()
    };
    let marker_style = if selected {
        theme.accent_style()
    } else {
        theme.dim_style()
    };
    let mut spans = vec![
        Span::styled(marker(selected), marker_style),
        Span::styled(format!("[ {label} ]"), style),
    ];
    spans.extend(suffix);
    Line::from(spans)
}
