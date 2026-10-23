//! Retired observer workers stay owned until an actual nonblocking join or router destruction.
use super::*;
use std::sync::atomic::Ordering;

/// Retired-worker observations, not a promise that every accepted record reached durable storage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShutdownProgress {
    pub pending: usize,
    pub joined: usize,
    pub panicked: usize,
    pub failed: usize,
    pub dropped: u64,
    pub submit_rejected: u64,
    pub store_failed: u64,
}

impl ShutdownProgress {
    #[cfg(feature = "gui")]
    pub fn merge(&mut self, other: Self) {
        self.pending += other.pending;
        self.joined += other.joined;
        self.panicked += other.panicked;
        self.failed += other.failed;
        self.dropped += other.dropped;
        self.submit_rejected += other.submit_rejected;
        self.store_failed += other.store_failed;
    }

    /// Records worker completion independently of the process exit code.
    pub fn record_outcome(self, elapsed: std::time::Duration) {
        let ms = elapsed.as_secs_f64() * 1000.0;
        if self.clean() {
            tracing::info!(target: "tasty::shutdown", ms, progress = ?self,
                clean = true, "S3b observer_sink_shutdown");
        } else {
            tracing::warn!(target: "tasty::shutdown", ms, progress = ?self,
                clean = false, "S3b observer_sink_shutdown");
        }
    }

    pub fn clean(&self) -> bool {
        self.pending == 0
            && self.panicked == 0
            && self.failed == 0
            && self.dropped == 0
            && self.submit_rejected == 0
            && self.store_failed == 0
    }
}

impl ObserverRouter {
    /// Disconnect every producer, including wildcard observers. Never waits for a sink.
    pub fn begin_shutdown(&mut self) {
        for (id, entry) in std::mem::take(&mut self.observers) {
            self.retire(id, entry);
        }
        self.line_buffers.clear();
    }

    /// Join only handles that are already finished; pending handles remain in this router.
    pub fn poll_shutdown(&mut self) -> ShutdownProgress {
        self.begin_shutdown();
        self.reap_finished();
        ShutdownProgress {
            pending: self.retired.len(),
            ..self.shutdown_progress
        }
    }

    /// Stop new memory-writer submissions before an unjoined sink's owner proceeds to drain.
    /// Accepted writer requests remain owned by that writer; pending sink handles stay here.
    pub fn fence_submissions(&self) {
        for retired in &self.retired {
            retired.tally.close_submissions();
        }
    }

    pub(super) fn retire(&mut self, id: ObserverId, entry: ObserverEntry) {
        drop(entry.tx);
        if let Some(join) = entry.join {
            self.retired.push(RetiredSink {
                id,
                join,
                tally: entry.sink_tally,
                dropped: entry.dropped,
            });
        }
    }

    pub(super) fn reap_finished(&mut self) {
        let mut pending = Vec::new();
        for retired in self.retired.drain(..) {
            if !retired.join.is_finished() {
                pending.push(retired);
                continue;
            }
            if let Err(error) = retired.join.join() {
                self.shutdown_progress.panicked += 1;
                tracing::warn!(
                    observer = retired.id,
                    ?error,
                    "observer sink worker panicked"
                );
            }
            self.shutdown_progress.joined += 1;
            self.shutdown_progress.failed +=
                usize::from(retired.tally.worker_failed.load(Ordering::Acquire));
            self.shutdown_progress.dropped += retired.dropped;
            self.shutdown_progress.submit_rejected +=
                retired.tally.submit_rejected.load(Ordering::Relaxed);
            self.shutdown_progress.store_failed +=
                retired.tally.store_failed.load(Ordering::Relaxed);
        }
        self.retired = pending;
    }

    #[cfg(test)]
    pub(super) fn join_retired(&mut self) {
        while self.poll_shutdown().pending != 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

impl Drop for ObserverRouter {
    fn drop(&mut self) {
        let progress = self.poll_shutdown();
        self.fence_submissions();
        if !progress.clean() {
            tracing::warn!(
                ?progress,
                "observer router dropped without clean sink completion; pending handles were not joined and process exit may discard accepted output"
            );
        }
        // Dropping an unfinished JoinHandle detaches it. Do not turn an expired shutdown
        // deadline into an unbounded wait here or report those workers as joined.
    }
}
