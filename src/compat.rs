use std::fmt;
use std::future::Future;
use std::time::Duration;

pub use web_time::Instant;

pub async fn sleep(duration: Duration) {
    futures_timer::Delay::new(duration).await;
}

/// Resident set size of the current process, in bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn ram_bytes() -> Option<usize> {
    memory_stats::memory_stats().map(|stats| stats.physical_mem)
}

/// Size of the wasm linear memory, in bytes (grows, never shrinks).
#[cfg(target_arch = "wasm32")]
pub fn ram_bytes() -> Option<usize> {
    Some(core::arch::wasm32::memory_size(0) * 65536)
}

#[derive(Debug)]
pub struct Elapsed;

impl fmt::Display for Elapsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("operation timed out")
    }
}

impl std::error::Error for Elapsed {}

pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Elapsed> {
    use futures::future::{select, Either};
    let sleep = sleep(duration);
    futures::pin_mut!(future);
    futures::pin_mut!(sleep);
    match select(future, sleep).await {
        Either::Left((output, _)) => Ok(output),
        Either::Right(_) => Err(Elapsed),
    }
}

#[derive(Debug)]
pub struct TaskHandle<T> {
    receiver: futures::channel::oneshot::Receiver<T>,
}

pub enum TaskPoll<T> {
    Pending,
    Finished(T),
    Gone,
}

impl<T> TaskHandle<T> {
    pub fn poll_result(&mut self) -> TaskPoll<T> {
        match self.receiver.try_recv() {
            Ok(Some(value)) => TaskPoll::Finished(value),
            Ok(None) => TaskPoll::Pending,
            Err(_) => TaskPoll::Gone,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<T, F>(future: F) -> TaskHandle<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let (sender, receiver) = futures::channel::oneshot::channel();
    tokio::spawn(async move {
        let _ = sender.send(future.await);
    });
    TaskHandle { receiver }
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<T, F>(future: F) -> TaskHandle<T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    let (sender, receiver) = futures::channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = sender.send(future.await);
    });
    TaskHandle { receiver }
}
