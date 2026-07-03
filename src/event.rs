use crate::app::AppResult;
use crate::constants::EVENT_HANDLER_TICKRATE;
use crate::input;
use crossterm::event::{Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEventKind};
use futures::{FutureExt, StreamExt};
use num_traits::Zero;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Event {
    Tick,
    Key(input::KeyEvent),
    Resize(u16, u16),
    Paste(String),
}

fn convert_key(code: CrosstermKeyCode) -> Option<input::KeyCode> {
    Some(match code {
        CrosstermKeyCode::Char(c) => input::KeyCode::Char(c),
        CrosstermKeyCode::Esc => input::KeyCode::Esc,
        CrosstermKeyCode::Enter => input::KeyCode::Enter,
        CrosstermKeyCode::Backspace => input::KeyCode::Backspace,
        CrosstermKeyCode::Tab => input::KeyCode::Tab,
        CrosstermKeyCode::Up => input::KeyCode::Up,
        CrosstermKeyCode::Down => input::KeyCode::Down,
        CrosstermKeyCode::Left => input::KeyCode::Left,
        CrosstermKeyCode::Right => input::KeyCode::Right,
        CrosstermKeyCode::PageUp => input::KeyCode::PageUp,
        CrosstermKeyCode::PageDown => input::KeyCode::PageDown,
        _ => return None,
    })
}

const EVENTS_CAPACITY: usize = 16;

#[allow(dead_code)]
#[derive(Debug)]
pub struct EventHandler {
    sender: mpsc::UnboundedSender<Event>,
    receiver: mpsc::UnboundedReceiver<Event>,
    handler: tokio::task::JoinHandle<()>,
}

async fn event_processor(tx: mpsc::UnboundedSender<Event>) {
    let mut reader = crossterm::event::EventStream::new();
    let mut tick = tokio::time::interval(EVENT_HANDLER_TICKRATE);
    loop {
        let tick_delay = tick.tick();
        let crossterm_event = reader.next().fuse();
        tokio::select! {
            _ = tick_delay => {
                let _ = tx.send(Event::Tick);
            }
            Some(Ok(evt)) = crossterm_event => {
                match evt {
                    CrosstermEvent::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            if let Some(code) = convert_key(key.code) {
                                let _ = tx.send(Event::Key(input::KeyEvent::new(code)));
                            }
                        }
                    },
                    CrosstermEvent::Resize(x, y) => {
                        let _ = tx.send(Event::Resize(x, y));
                    },
                    CrosstermEvent::Paste(data) => {
                        let _ = tx.send(Event::Paste(data));
                    },
                    _ => ()
                }
            }
        }
    }
}

impl EventHandler {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let handler = tokio::spawn(event_processor(sender.clone()));

        Self {
            sender,
            receiver,
            handler,
        }
    }

    pub async fn nexts(&mut self) -> AppResult<Vec<Event>> {
        let mut buffer = Vec::with_capacity(EVENTS_CAPACITY);
        let c = self.receiver.recv_many(&mut buffer, EVENTS_CAPACITY).await;

        if c.is_zero() {
            return Err(anyhow::anyhow!("event channel closed"));
        }

        Ok(buffer)
    }
}
