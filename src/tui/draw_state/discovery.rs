use crate::state::{DiscoveryParams, DiscoveryStage};
use crate::tui::theme::Theme;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub fn draw(params: &DiscoveryParams, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    match params.stage {
        DiscoveryStage::Select => {
            lines.push(heading("Select an interface:", theme));
            lines.push(Line::default());
            for (i, name) in ["Mock", "Wired (serial)", "Network (TCP)"].iter().enumerate() {
                lines.push(choice(i as u16 == params.selected, name, theme));
            }
            lines.push(Line::default());
            lines.push(hint(
                " \u{2191}/\u{2193} select \u{b7} enter choose \u{b7} esc back/quit",
                theme,
            ));
        }
        DiscoveryStage::Wired => {
            let port = params
                .ports
                .get(params.port_index as usize)
                .map(String::as_str)
                .unwrap_or("(no ports found)");
            lines.push(heading("Wired (serial):", theme));
            lines.push(Line::default());
            lines.push(field(params.selected == 0, "Port", port, theme));
            lines.push(field(params.selected == 1, "Baud", &params.baud_rate.to_string(), theme));
            lines.push(field(params.selected == 2, "Data bits", &format!("{:?}", params.data_bits), theme));
            lines.push(field(params.selected == 3, "Parity", &format!("{:?}", params.parity), theme));
            lines.push(field(params.selected == 4, "Stop bits", &format!("{:?}", params.stop_bits), theme));
            lines.push(Line::default());
            lines.push(choice(params.selected == 5, "Connect", theme));
            lines.push(Line::default());
            lines.push(hint(
                " \u{2191}/\u{2193} field \u{b7} \u{2190}/\u{2192} change \u{b7} enter connect \u{b7} esc back",
                theme,
            ));
        }
        DiscoveryStage::Network => {
            lines.push(heading("Network (TCP):", theme));
            lines.push(Line::default());
            lines.push(text_field(params.selected == 0, "IP", &params.ip, theme));
            lines.push(text_field(params.selected == 1, "Port", &params.port, theme));
            lines.push(Line::default());
            lines.push(choice(params.selected == 2, "Connect", theme));
            lines.push(Line::default());
            lines.push(hint(
                " \u{2191}/\u{2193} field \u{b7} type to edit \u{b7} enter connect \u{b7} esc back",
                theme,
            ));
        }
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

fn heading(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), theme.dim_style()))
}

fn hint(text: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(text.to_string(), theme.dim_style()))
}

fn choice(selected: bool, name: &str, theme: &Theme) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let style = if selected {
        theme.selected_style()
    } else {
        theme.base()
    };
    Line::from(Span::styled(format!("{marker}{name}"), style))
}

fn field(selected: bool, name: &str, value: &str, theme: &Theme) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let value_style = if selected {
        theme.selected_style()
    } else {
        theme.base()
    };
    Line::from(vec![
        Span::styled(format!("{marker}{name:<10} "), theme.dim_style()),
        Span::styled(value.to_string(), value_style),
    ])
}

fn text_field(selected: bool, name: &str, value: &str, theme: &Theme) -> Line<'static> {
    let marker = if selected { "> " } else { "  " };
    let cursor = if selected { "_" } else { "" };
    Line::from(vec![
        Span::styled(format!("{marker}{name:<10} "), theme.dim_style()),
        Span::styled(value.to_string(), theme.accent_style()),
        Span::styled(cursor.to_string(), theme.accent_style()),
    ])
}
