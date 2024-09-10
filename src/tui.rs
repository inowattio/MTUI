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

impl<B: Backend> Tui<B> {
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
        let headline = match app.state {
            State::Configure => "Configure!".to_string(),
            State::Read => format!("At: {} on {}\n\n{}", app.position, app.displaying_type(), app.rendered_data),
            State::Jump => format!("Jump from {} at: {}", app.position, app.input_number.map_or("none".to_string(), |n| n.to_string())),
            State::Write => format!("Write at {} value: {}", app.position, app.input_number.map_or("none".to_string(), |n| n.to_string()))
        };

        self.terminal.draw(|frame| frame.render_widget(
            Paragraph::new(headline)
                .block(
                    Block::default()
                        .title("Q - Exit; Up/Down - Move; R - Refresh; T - Switch Register Type; W - Write; J - Jump; Enter - Action")
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded),
                )
                .style(Style::default().fg(Color::Cyan).bg(Color::Black))
                .alignment(Alignment::Left),
            frame.size(),
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
