//! Opt-in numeric callback observations. Never requests a wake or changes control flow.
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

const CAPACITY: usize = 8192;
static OUTPUT: OnceLock<Option<PathBuf>> = OnceLock::new();
thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

#[derive(serde::Serialize)]
struct Sample {
    seq: u64,
    site: u8,
    shutdown: bool,
    start_ns: u128,
    end_ns: u128,
    wake_lateness_ns: Option<u128>,
}
#[derive(serde::Serialize)]
struct Capture {
    #[serde(skip)]
    origin: Instant,
    capacity: usize,
    dropped: u64,
    seen: u64,
    samples: Vec<Sample>,
}
impl Capture {
    fn new(origin: Instant, capacity: usize) -> Self {
        Self {
            origin,
            capacity,
            dropped: 0,
            seen: 0,
            samples: Vec::with_capacity(capacity),
        }
    }
    fn record(
        &mut self,
        site: u8,
        shutdown: bool,
        start: Instant,
        end: Instant,
        wake: Option<u128>,
    ) {
        self.seen = self.seen.saturating_add(1);
        if self.samples.len() == self.capacity {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.samples.push(Sample {
            seq: self.seen,
            site,
            shutdown,
            start_ns: start.saturating_duration_since(self.origin).as_nanos(),
            end_ns: end.saturating_duration_since(self.origin).as_nanos(),
            wake_lateness_ns: wake,
        });
    }
}
fn capacity() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("TASTY_DEBUG_LOOP_SAMPLE_LIMIT")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|n| (1..=CAPACITY).contains(n))
            .unwrap_or(CAPACITY)
    })
}
fn enabled() -> bool {
    OUTPUT
        .get_or_init(|| {
            std::env::var_os("TASTY_DEBUG_LOOP_SAMPLES")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        })
        .is_some()
}
fn record(site: u8, shutdown: bool, start: Instant, end: Instant, wake: Option<u128>) {
    CAPTURE.with(|capture| {
        let mut capture = capture.borrow_mut();
        capture
            .get_or_insert_with(|| Capture::new(start, capacity()))
            .record(site, shutdown, start, end, wake);
    });
}

/// The owned guard covers early returns; callbacks can nest without retaining a buffer borrow.
pub(crate) struct Guard(Option<(Instant, u8, bool)>);
impl Guard {
    pub(crate) fn enter(site: u8, shutdown: bool) -> Self {
        Self::when_enabled(enabled(), site, shutdown)
    }
    fn when_enabled(enabled: bool, site: u8, shutdown: bool) -> Self {
        if enabled {
            Self::started(site, shutdown, Instant::now())
        } else {
            Self(None)
        }
    }
    fn started(site: u8, shutdown: bool, now: Instant) -> Self {
        CAPTURE.with(|c| {
            c.borrow_mut()
                .get_or_insert_with(|| Capture::new(now, capacity()));
        });
        Self(Some((now, site, shutdown)))
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some((start, site, shutdown)) = self.0 {
            record(site, shutdown, start, Instant::now(), None);
        }
    }
}
/// Existing timer wake lateness, not callback cost or a newly scheduled heartbeat.
pub(crate) fn wake(requested: Instant, shutdown: bool) {
    if enabled() {
        let now = Instant::now();
        record(
            0,
            shutdown,
            now,
            now,
            Some(now.saturating_duration_since(requested).as_nanos()),
        );
    }
}
/// Export once, after the event loop and App Drop traces; no output IO occurs in callbacks.
pub(crate) fn finish() {
    let Some(path) = OUTPUT.get().and_then(Option::as_ref) else {
        return;
    };
    let capture = CAPTURE.with(|c| c.borrow_mut().take());
    if let Some(capture) = capture {
        match serde_json::to_vec(&capture)
            .map_err(std::io::Error::other)
            .and_then(|bytes| std::fs::write(path, bytes))
        {
            Ok(()) => {
                tracing::info!(target: "tasty::shutdown", samples=capture.samples.len(), dropped=capture.dropped, "callback_capture_exported")
            }
            Err(error) => {
                tracing::warn!(target: "tasty::shutdown", error_kind=?error.kind(), "callback_capture_export_failed")
            }
        }
    }
}

#[cfg(test)]
mod tests;
