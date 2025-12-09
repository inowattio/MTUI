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
use ratatui::widgets::{Block, Borders, BorderType, Paragraph};

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
            State::Dump(_) => "Enter - Start/Continue; Q - Back"
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
                    "Pinned data\nNo pinned registers. Press P to add one.".to_string()
                } else {
                    format!("Pinned data\n{}\n{}", params.header, params.pinned_data)
                };
                frame.render_widget(
                    Paragraph::new(pinned_text).style(base_style).alignment(Alignment::Left),
                    columns[1],
                );
                return;
            }

            let content = match &app.state {
                State::Read(_) => unreachable!(),
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
                State::Dump(params) => format!("From: {} on {}, started: {}", app.position, app.displaying_type(), params.started),
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
