use crate::app::{App, AppResult, State};
use crate::event::EventHandler;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::Backend;
use ratatui::Terminal;
use std::io;
use std::panic;
use ratatui::layout::Alignment;
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

        let content = match &app.state {
            State::Read(params) => format!("At: {} on {}\n\n{}", app.position, app.displaying_type(), params.data),
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
Enter - Action".to_string(),
            State::Dump(params) => format!("From: {} on {}, started: {}", app.position, app.displaying_type(), params.started),
        };

        let title = match app.state {
            State::Read(_) => "H - Help",
            State::Jump(_) => "Enter - Go; Q - Back",
            State::Write(_) => "Enter - Write; Q - Back",
            State::Help => "Q/Enter - Back",
            State::Dump(_) => "Enter - Start/Continue; Q - Back"
        };

        self.terminal.draw(|frame| frame.render_widget(
            Paragraph::new(format!("Device: {device}\n{content}"))
                .block(
                    Block::default()
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                )
                .style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .alignment(Alignment::Left),
            frame.area(),
        ))?;
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
