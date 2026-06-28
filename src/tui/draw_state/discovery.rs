use crate::state::{DiscoveryField, DiscoveryParams, InterfaceKind};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;
use std::net::Ipv4Addr;

pub fn draw(params: &DiscoveryParams, frame: &mut Frame, area: Rect, theme: &Theme) {
    let blocked = match params.interface {
        InterfaceKind::Network => params
            .ip
            .parse::<Ipv4Addr>()
            .is_err()
            .then_some("invalid IP"),
        InterfaceKind::Wired => params.ports.is_empty().then_some("no serial ports"),
        InterfaceKind::Mock => None,
    };

    let mut lines: Vec<Line> = Vec::new();
    for (i, &field) in params.fields().iter().enumerate() {
        if field == DiscoveryField::Connect {
            lines.push(Line::default());
        }
        lines.push(render_field(
            params,
            field,
            i as u16 == params.selected,
            blocked,
            theme,
        ));
    }

    if let Some(status) = &params.status {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!(" {}", status.text),
            theme.message_style(status.kind),
        )));
    }

    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let height = lines.len() as u16 + 2;
    let rect = super::popups::centered_rect(width, height, area);

    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(lines).block(theme.panel("Connection")),
        rect,
    );
}

fn render_field(
    params: &DiscoveryParams,
    field: DiscoveryField,
    selected: bool,
    blocked: Option<&'static str>,
    theme: &Theme,
) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };

    let label_style = if selected {
        theme.accent_style()
    } else {
        theme.dim_style()
    };

    if field == DiscoveryField::Connect {
        let button_style = if blocked.is_some() {
            theme.dim_style()
        } else if selected {
            theme.selected_style()
        } else {
            theme.accent_style()
        };
        let mut spans = vec![
            Span::styled(marker, label_style),
            Span::styled("[ Connect ]", button_style),
        ];
        if let Some(reason) = blocked {
            spans.push(Span::styled(
                format!("   \u{2717} {reason}"),
                theme.err_style(),
            ));
        }
        return Line::from(spans);
    }

    let (name, value, cyclable) = field_view(params, field);
    let value_text = if selected && cyclable {
        format!("\u{2039} {value} \u{203a}")
    } else if selected {
        format!("{value}_")
    } else {
        value
    };

    let value_style = if field == DiscoveryField::Ip && params.ip.parse::<Ipv4Addr>().is_err() {
        theme.err_style()
    } else if selected {
        theme.selected_style()
    } else {
        theme.base()
    };

    Line::from(vec![
        Span::styled(format!("{marker}{name:<22} "), label_style),
        Span::styled(value_text, value_style),
    ])
}

fn field_view(p: &DiscoveryParams, field: DiscoveryField) -> (&'static str, String, bool) {
    match field {
        DiscoveryField::Interface => {
            let v = match p.interface {
                InterfaceKind::Mock => "Mock",
                InterfaceKind::Wired => "Wired (serial)",
                InterfaceKind::Network => "Network (TCP)",
            };
            ("Interface", v.to_string(), true)
        }
        DiscoveryField::Port => (
            "Port",
            p.ports
                .get(p.port_index as usize)
                .cloned()
                .unwrap_or_else(|| "(no ports found)".to_string()),
            true,
        ),
        DiscoveryField::Baud => ("Baud", p.baud_rate.to_string(), true),
        DiscoveryField::DataBits => ("Data bits", format!("{:?}", p.data_bits), true),
        DiscoveryField::Parity => ("Parity", format!("{:?}", p.parity), true),
        DiscoveryField::StopBits => ("Stop bits", format!("{:?}", p.stop_bits), true),
        DiscoveryField::Ip => ("IP", p.ip.clone(), false),
        DiscoveryField::NetPort => ("Port", p.net_port.to_string(), false),
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
        DiscoveryField::Connect => ("Connect", String::new(), false),
    }
}
