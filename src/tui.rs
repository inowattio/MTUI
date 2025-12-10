use crate::app::{App, AppResult, State};
use crate::event::EventHandler;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::Backend;
use ratatui::Terminal;
use std::io;
use std::panic;
use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::prelude::{Color, Style};
use ratatui::widgets::{Block, Borders, BorderType, Gauge, Paragraph};

#[derive(Debug)]
pub struct Tui<B: Backend> {
    terminal: Terminal<B>,
    pub events: EventHandler,
}

impl<B: Backend> Tui<B> where <B as Backend>::Error: 'static {
    pub fn new(terminal: Terminal<B>, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    pub fn init(&mut self) -> AppResult<()> {
        terminal::enable_raw_mode()?;
        crossterm::execute!(io::stderr(), EnterAlternateScreen, EnableMouseCapture)?;

        let panic_hook = panic::take_hook();
        panic::set_hook(Box::new(move |panic| {
            Self::reset().expect("failed to reset the terminal");
            panic_hook(panic);
        }));

        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    pub fn draw(&mut self, app: &mut App) -> AppResult<()> {
        let device = app.config.display_device();
        let base_style = Style::default().fg(Color::Cyan).bg(Color::Black);

        self.terminal.draw(|frame| {
            let title = match &app.state {
            State::Read(_) => "H - Help; P - Add/Remove Pin",
            State::Jump(_) => "Enter - Go; Q - Back",
            State::Write(_) => "Enter - Write; Q - Back",
            State::Help => "Q/Enter - Back",
            State::Dump(_) => "Enter - Start; 0-9 Set Batches; Q - Back"
            };

            if let State::Read(params) = &app.state {
                let is_pinned = app.pinned_registers.iter()
                    .position(|(kind, address)| kind == &app.register_display_type && *address == app.position).is_some();
                let pinned_string = if is_pinned { "(Pinned)" } else { "" };

                let outer = Block::default()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded);

                let outer_area = frame.area();
                let inner_area = outer.inner(outer_area);
                frame.render_widget(outer, outer_area);

                let info = format!("Device: {}\nAt: {} on {} {}", device, app.position, app.displaying_type(), pinned_string);
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(2), Constraint::Min(0)].as_ref())
                    .split(inner_area);

                frame.render_widget(
                    Paragraph::new(info).style(base_style).alignment(Alignment::Left),
                    rows[0],
                );

                let columns = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
                    .split(rows[1]);

                let main_text = format!("Main data\n{}\n{}", params.header, params.main_data);
                frame.render_widget(
                    Paragraph::new(main_text).style(base_style).alignment(Alignment::Left),
                    columns[0],
                );

                let pinned_text = if params.pinned_data.is_empty() {
                    if app.pinned_registers.is_empty() {
                        "Pinned data\nNo pinned registers."
                    } else {
                        "Pinned data\nNo pinned data."
                    }.to_string()
                } else {
                    format!("Pinned data\n{}\n{}", params.header, params.pinned_data)
                };
                frame.render_widget(
                    Paragraph::new(pinned_text).style(base_style).alignment(Alignment::Left),
                    columns[1],
                );
                return;
            }

            if let State::Dump(params) = &app.state {
                let outer = Block::default()
                    .title(title)
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded);

                let outer_area = frame.area();
                let inner_area = outer.inner(outer_area);
                frame.render_widget(outer, outer_area);

                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(
                        [
                            Constraint::Length(3),
                            Constraint::Length(3),
                            Constraint::Min(0),
                        ]
                            .as_ref(),
                    )
                    .split(inner_area);

                let info = format!(
                    "Device: {}\nStart at {} on {}",
                    device,
                    params.start_position,
                    app.displaying_type()
                );
                frame.render_widget(
                    Paragraph::new(info).style(base_style).alignment(Alignment::Left),
                    rows[0],
                );

                let ratio = if let Some(total) = params.total_batches {
                    if total == 0 {
                        0.0
                    } else {
                        (params.completed_batches as f64 / total as f64).clamp(0.0, 1.0)
                    }
                } else {
                    0.0
                };
                let progress_text = match params.total_batches {
                    Some(total) => format!("{}/{} batches ({}/{} registers)", params.completed_batches, total, params.completed_batches * app.config.registers_batch as u32, total * app.config.registers_batch as i32),
                    None => "Set batch count to start".to_string(),
                };
                let gauge = Gauge::default()
                    .gauge_style(base_style)
                    .label(progress_text)
                    .ratio(ratio);
                frame.render_widget(gauge, rows[1]);

                let status = if params.started { "Running" } else { "Idle" };
                let mut details = format!(
                    "Batch size: {} | Status: {}",
                    app.config.registers_batch, status
                );
                if let Some(err) = &params.error {
                    details.push_str(&format!("\nError: {err}"));
                }
                frame.render_widget(
                    Paragraph::new(details).style(base_style).alignment(Alignment::Left),
                    rows[2],
                );
                return;
            }

            let content = match &app.state {
                State::Jump(params) => format!("Jump from {} at: {}", app.position, params.position.map_or("none".to_string(), |n| n.to_string())),
                State::Write(params) => format!("Write at {} value: {}\nResult: {:?}",
                                        app.position, params.value.map_or("none".to_string(), |n| n.to_string()), params.result),
                State::Help => "Q - Exit/Back
Up/Down - Move Cursor
R - Refresh Data
T - Switch Register Type
W - Write
J - Jump
D - Dump
H - Help
P - Add/Remove Pin (Read only)
Enter - Action".to_string(),
                _ => unreachable!(),
            };

            frame.render_widget(
                Paragraph::new(format!("Device: {device}\n{content}"))
                    .block(
                        Block::default()
                            .title(title)
                            .title_alignment(Alignment::Center)
                            .borders(Borders::ALL)
                            .border_type(BorderType::Rounded),
                    )
                    .style(base_style)
                    .alignment(Alignment::Left),
                frame.area(),
            );
        })?;
        Ok(())
    }

    fn reset() -> AppResult<()> {
        terminal::disable_raw_mode()?;
        crossterm::execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCapture)?;
        Ok(())
    }

    pub fn exit(&mut self) -> AppResult<()> {
        Self::reset()?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}
