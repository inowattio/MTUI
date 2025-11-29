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
        let content = match &app.state {
            State::Read => format!("At: {} on {}\n\n{}", app.position, app.displaying_type(), app.rendered_data),
            State::Jump => format!("Jump from {} at: {}", app.position, app.input_number.map_or("none".to_string(), |n| n.to_string())),
            State::Write(params) => format!("Write at {} value: {}\nResult: {:?}",
                                    app.position, app.input_number.map_or("none".to_string(), |n| n.to_string()), params.result),
            State::Help => "\n
            Q - Exit/Back\n
            Up/Down - Move\n
            R - Refresh\n
            T - Switch Register Type\n
            W - Write\n
            J - Jump\n
            H - Help\n
            Enter - Action\n
            \n".to_string(),
            State::Dump(params) => format!("From: {} on {}, started: {}", app.position, app.displaying_type(), params.started),
        };

        let title = match app.state {
            State::Read => "H - Help",
            State::Jump => "Enter - Go; Q - Back",
            State::Write(_) => "Enter - Write; Q - Back",
            State::Help => "Q/Enter - Back",
            State::Dump(_) => "Enter - Start/Continue; Q - Back"
        };

        self.terminal.draw(|frame| frame.render_widget(
            Paragraph::new(content)
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
