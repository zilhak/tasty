//! CA loading and runtime construction/destruction stay on bounded owned setup threads.
use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread::JoinHandle,
};
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static RETIRED: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());
static POISONED: AtomicBool = AtomicBool::new(false);
struct Permit;
impl Drop for Permit {
    fn drop(&mut self) {
        WORKERS.fetch_sub(1, Ordering::AcqRel);
    }
}
fn reap() {
    let mut retired =
        crate::poison::recover_mutex(RETIRED.lock(), "HTTP setup retirement", &POISONED);
    let mut live = Vec::new();
    for thread in retired.drain(..) {
        if thread.is_finished() {
            if thread.join().is_err() {
                tracing::warn!("HTTP setup thread panicked");
            }
        } else {
            live.push(thread);
        }
    }
    *retired = live;
}
pub(super) fn limits() -> core::Limits {
    let capacity = tasty_ipc::admission::QUEUED_BYTES_LIMIT;
    core::Limits {
        workers: 8,
        requests_per_document: 256,
        retained_bytes: capacity,
        upload_bytes: capacity,
        encoded_bytes: capacity,
        decoded_bytes: capacity,
        header_bytes: wire::HEADER_BYTES,
        connect_timeout: Duration::from_secs(30),
        read_timeout: Duration::from_secs(30),
        network_time: Duration::from_secs(30),
    }
}
pub(super) struct Setup {
    result: mpsc::Receiver<core::Result<core::Backend>>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}
impl Setup {
    pub(super) fn start(wake: Arc<dyn Fn() + Send + Sync>) -> core::Result<Self> {
        reap();
        WORKERS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < 8).then_some(n + 1)
            })
            .map_err(|_| core::Error::Capacity)?;
        let permit = Permit;
        let (tx, result) = mpsc::sync_channel(1);
        let (stop, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("html-http-setup".into())
            .spawn(move || {
                let _permit = permit;
                let runtime = match tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .max_blocking_threads(8)
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        // A dropped receiver means its engine already retired this setup.
                        if tx.send(Err(core::Error::Worker)).is_err() {
                            tracing::debug!("HTTP setup error receiver already retired");
                        }
                        wake();
                        return;
                    }
                };
                let entered = runtime.enter();
                let result = core::Backend::new(limits(), runtime.handle().clone())
                    .and_then(|backend| backend.with_waker(wake.clone()));
                drop(entered);
                let success = result.is_ok();
                let delivered = tx.send(result).is_ok();
                wake();
                if success && delivered {
                    // Closing the owner channel is the normal shutdown signal.
                    if rx.recv().is_ok() {
                        tracing::debug!("HTTP setup explicitly stopped");
                    }
                }
                // Runtime destruction may wait for blocking decode; only this owned worker waits.
                drop(runtime);
            })
            .map_err(|_| core::Error::Worker)?;
        Ok(Self {
            result,
            stop: Some(stop),
            thread: Some(thread),
        })
    }
    pub(super) fn poll(&self) -> Option<core::Result<core::Backend>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(core::Error::Worker)),
        }
    }
}
impl Drop for Setup {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            crate::poison::recover_mutex(RETIRED.lock(), "HTTP setup retirement", &POISONED)
                .push(thread);
        }
    }
}
#[cfg(test)]
pub(super) fn reaped_for_tests() -> bool {
    reap();
    WORKERS.load(Ordering::Acquire) == 0
        && crate::poison::recover_mutex(RETIRED.lock(), "HTTP setup retirement", &POISONED)
            .is_empty()
}
