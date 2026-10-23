//! Monotonic observations only; no deadlines, waits or scheduling decisions.
use super::*;

/// Final flush and actual worker join are observed independently of process exit status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShutdownObservation {
    NotStarted,
    Draining,
    Joined { panicked: bool, flush: FlushResult },
}

/// First cut to observation, or the frozen actual-join duration after join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownTiming {
    pub elapsed: Duration,
    pub joined_after: Option<Duration>,
}

/// Nonconsuming final outcome and its timing. No configured writer is represented by the caller's None.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    pub observation: ShutdownObservation,
    pub timing: Option<ShutdownTiming>,
}

/// Per-owner monotonic timer; repeated stop/join observations never restart it.
#[derive(Debug, Default)]
pub struct ShutdownTimer {
    started: Option<Instant>,
    joined: Option<Duration>,
}
impl ShutdownTimer {
    pub fn start(&mut self, now: Instant) {
        self.started.get_or_insert(now);
    }
    /// Call only after the retained JoinHandle's join has actually returned.
    pub fn joined(&mut self, now: Instant) {
        if let Some(started) = self.started {
            self.joined
                .get_or_insert(now.saturating_duration_since(started));
        }
    }
    pub fn snapshot(&self, now: Instant) -> Option<ShutdownTiming> {
        self.started.map(|started| ShutdownTiming {
            elapsed: self
                .joined
                .unwrap_or_else(|| now.saturating_duration_since(started)),
            joined_after: self.joined,
        })
    }
}

impl<X: Executor> Writer<X> {
    /// 접수를 멈추고 마지막 flush cut을 넣은 뒤 worker가 남은 요청을 실행하고 끝나게 한다.
    /// 여러 번 불러도 한 번만 시작한다.
    pub fn begin_shutdown(&mut self) {
        if self.final_flush.is_some() {
            return;
        }
        if self.shutdown_observation == ShutdownObservation::NotStarted {
            self.shutdown_observation = ShutdownObservation::Draining;
        }
        let shared = &self.handle.shared;
        let mut queue = shared.lock();
        self.shutdown_timer.start(shared.clock.now());
        queue.admitting = false;
        if queue.worker_alive {
            self.final_flush = Some(shared.push_flush(&mut queue, FlushPoint::START));
        }
        queue.stopping = true;
        drop(queue);
        shared.arrived.notify_one();
    }

    /// 종료를 진행하고 아직 끝나지 않은 worker 수(0 또는 1)를 돌려준다. join이 끝나면 마지막
    /// flush 결과를 로그에 남긴다.
    pub fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        if self.worker.as_ref().is_some_and(|w| w.is_finished())
            && let Some(worker) = self.worker.take()
        {
            let panicked = worker.join().is_err();
            self.shutdown_timer.joined(self.handle.shared.clock.now());
            let joined_us = self
                .shutdown_timing()
                .and_then(|t| t.joined_after)
                .map(|d| d.as_micros());
            tracing::info!(target: "tasty::shutdown", writer = %self.name,
                panicked, ?joined_us, "writer_actual_join");
            if panicked {
                tracing::error!(writer = %self.name, "writer worker panicked");
            }
            self.log_final_flush(panicked);
        }
        usize::from(self.worker.is_some())
    }

    /// A read-only snapshot; never consumes a request or final flush receipt.
    pub fn shutdown_observation(&self) -> ShutdownObservation {
        self.shutdown_observation.clone()
    }

    /// Numeric timing paired with the cached outcome; consumes no receipt.
    pub fn shutdown_report(&self) -> ShutdownReport {
        ShutdownReport {
            observation: self.shutdown_observation(),
            timing: self.shutdown_timing(),
        }
    }

    /// Read-only first-cut timing; None until shutdown starts.
    pub fn shutdown_timing(&self) -> Option<ShutdownTiming> {
        self.shutdown_timer.snapshot(self.handle.shared.clock.now())
    }

    fn log_final_flush(&mut self, panicked: bool) {
        let result = self
            .final_flush
            .take()
            .and_then(|receipt| receipt.try_take())
            .unwrap_or(Err(FlushFailure::Lost));
        self.shutdown_observation = ShutdownObservation::Joined {
            panicked,
            flush: result.clone(),
        };
        match &result {
            Ok(point) => {
                tracing::debug!(writer = %self.name, seq = point.seq, "writer drained");
            }
            Err(FlushFailure::NotClean {
                point,
                rejected,
                failed,
            }) => log_unclean_drain(&self.name, *point, *rejected, *failed),
            Err(FlushFailure::Lost) => log_lost_drain(&self.name),
        }
    }
}

#[cfg(test)]
mod tests;
