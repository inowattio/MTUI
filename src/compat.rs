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
    use futures::future::{Either, select};
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
    abort: futures::future::AbortHandle,
}

impl<T> Drop for TaskHandle<T> {
    fn drop(&mut self) {
        self.abort.abort();
    }
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
    let (future, abort) = futures::future::abortable(future);
    tokio::spawn(async move {
        if let Ok(value) = future.await {
            let _ = sender.send(value);
        }
    });
    TaskHandle { receiver, abort }
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<T, F>(future: F) -> TaskHandle<T>
where
    T: 'static,
    F: Future<Output = T> + 'static,
{
    let (sender, receiver) = futures::channel::oneshot::channel();
    let (future, abort) = futures::future::abortable(future);
    wasm_bindgen_futures::spawn_local(async move {
        if let Ok(value) = future.await {
            let _ = sender.send(value);
        }
    });
    TaskHandle { receiver, abort }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::Arc;

    async fn settle(guard: &Arc<()>) {
        for _ in 0..200 {
            if Arc::strong_count(guard) == 1 {
                return;
            }
            tokio::task::yield_now().await;
            sleep(Duration::from_millis(1)).await;
        }
        panic!("task kept running after its handle was dropped");
    }

    #[tokio::test]
    async fn dropping_the_handle_aborts_the_task() {
        let guard = Arc::new(());
        let held = guard.clone();
        let handle = spawn(async move {
            let _held = held;
            sleep(Duration::from_secs(60)).await;
        });
        tokio::task::yield_now().await;
        assert_eq!(Arc::strong_count(&guard), 2, "task must own its clone");

        drop(handle);
        settle(&guard).await;
    }

    #[tokio::test]
    async fn finished_task_delivers_its_result() {
        let mut handle = spawn(async { 7 });
        for _ in 0..200 {
            match handle.poll_result() {
                TaskPoll::Finished(value) => {
                    assert_eq!(value, 7);
                    return;
                }
                TaskPoll::Pending => tokio::task::yield_now().await,
                TaskPoll::Gone => panic!("task vanished without a result"),
            }
        }
        panic!("task never finished");
    }
}
