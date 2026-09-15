//! One startup's fixed observations, retained independently of the stderr ring.
//! Like the host boot trace, durations use Instant; these are parent observations,
//! not measurements of child scheduling or individual bootstrap operations.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::spawn_diag::StderrCapture;

#[derive(Clone)]
pub(super) struct StartupTimeline(Arc<Observations>);

struct Observations {
    started: Instant,
    pid: u32,
    spawn_returned: Duration,
    port_found: OnceLock<Duration>,
    first_response: OnceLock<Duration>,
    shell_ready: OnceLock<Duration>,
}

impl StartupTimeline {
    pub(super) fn new(started: Instant, pid: u32) -> Self {
        let spawn_returned = started.elapsed();
        super::spawn_diag::init_test_tracing();
        let value = Self(Arc::new(Observations {
            started,
            pid,
            spawn_returned,
            port_found: OnceLock::new(),
            first_response: OnceLock::new(),
            shell_ready: OnceLock::new(),
        }));
        tracing::info!("{}", value.snapshot());
        value
    }

    fn mark(&self, slot: &OnceLock<Duration>) {
        // Repeated IPC calls must not overwrite the first response timestamp.
        if slot.set(self.0.started.elapsed()).is_ok() {
            tracing::info!("{}", self.snapshot());
        }
    }

    pub(super) fn port_found(&self) {
        self.mark(&self.0.port_found);
    }

    pub(super) fn first_response(&self) {
        self.mark(&self.0.first_response);
    }

    pub(super) fn shell_ready(&self) {
        self.mark(&self.0.shell_ready);
    }

    pub(super) fn snapshot(&self) -> String {
        let pending = if self.0.port_found.get().is_none() {
            "port"
        } else if self.0.first_response.get().is_none() {
            "first_ipc_response"
        } else if self.0.shell_ready.get().is_none() {
            "shell_ready"
        } else {
            "none"
        };
        let ms = |value: Option<&Duration>| {
            value.map_or_else(
                || "pending".to_owned(),
                |d| format!("{:.3}", d.as_secs_f64() * 1000.0),
            )
        };
        format!(
            "startup pid={} elapsed_ms={:.3} pending={} spawn_returned_ms={} port_found_ms={} first_ipc_response_ms={} shell_ready_ms={}",
            self.0.pid,
            self.0.started.elapsed().as_secs_f64() * 1000.0,
            pending,
            ms(Some(&self.0.spawn_returned)),
            ms(self.0.port_found.get()),
            ms(self.0.first_response.get()),
            ms(self.0.shell_ready.get()),
        )
    }
}

/// Libtest retains panic output; reprint the fixed prefix there even when later
/// child stderr has displaced all early lines. This does not install a panic hook.
pub(super) struct StartupFailure<'a> {
    timeline: &'a StartupTimeline,
    stderr: Option<&'a StderrCapture>,
}

impl<'a> StartupFailure<'a> {
    pub(super) fn new(timeline: &'a StartupTimeline, stderr: Option<&'a StderrCapture>) -> Self {
        Self { timeline, stderr }
    }
}

impl Drop for StartupFailure<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            tracing::warn!("startup failure: {}", self.timeline.snapshot());
            if let Some(stderr) = self.stderr {
                tracing::warn!(
                    "stderr tail ({} lines):\n{}",
                    stderr.tail_lines(),
                    stderr.tail()
                );
            }
        }
    }
}
