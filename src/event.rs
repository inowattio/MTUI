use crate::app::AppResult;
use crate::constants::EVENT_HANDLER_TICKRATE;
use crossterm::event::{Event as CrosstermEvent, KeyEvent};
use futures::{FutureExt, StreamExt};
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Resize(u16, u16),
    Paste(String),
}

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
                        if key.kind == crossterm::event::KeyEventKind::Press {
                            let _ = tx.send(Event::Key(key));
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
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        let handler = tokio::spawn(event_processor(sender.clone()));

        Self {
            sender,
            receiver,
            handler,
        }
    }

    pub async fn next(&mut self) -> AppResult<Event> {
        self.receiver
            .recv()
            .await
            .ok_or(Box::new(std::io::Error::other("This is an IO error")))
    }
}
