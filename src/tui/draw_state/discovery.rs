use crate::state::{DiscoveryField, DiscoveryParams, InterfaceKind};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(params: &DiscoveryParams, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = vec![Line::default()];

    for (i, &field) in params.fields().iter().enumerate() {
        if field == DiscoveryField::Connect {
            lines.push(Line::default());
        }
        lines.push(render_field(
            params,
            field,
            i as u16 == params.selected,
            theme,
        ));
    }

    if let Some(status) = &params.status {
        lines.push(Line::default());
        let style = if status.to_lowercase().contains("fail") {
            theme.err_style()
        } else {
            theme.warn_style()
        };
        lines.push(Line::from(Span::styled(status.clone(), style)));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn render_field(
    params: &DiscoveryParams,
    field: DiscoveryField,
    selected: bool,
    theme: &Theme,
) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let style = if selected {
        theme.selected_style()
    } else {
        theme.base()
    };

    if field == DiscoveryField::Connect {
        return Line::from(Span::styled(format!("{marker}[ Connect ]"), style));
    }

    let (name, value, cyclable) = field_view(params, field);
    let value_text = if selected && cyclable {
        format!("\u{2039} {value} \u{203a}")
    } else if selected {
        format!("{value}_")
    } else {
        value
    };

    Line::from(vec![
        Span::styled(format!("{marker}{name:<22} "), theme.dim_style()),
        Span::styled(value_text, style),
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
