//! 종료 단계의 저장 writer drain 기한. memory.db·state.db·설정 파일 writer가 같은 기한과 포기 규칙을 쓴다.

use std::time::{Duration, Instant};

/// 첫 대기부터 잰 drain 기한.
pub(crate) const WRITER_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// writer 하나의 drain 상태. 기한을 넘기면 미회수로 기록하고 다시 기다리지 않는다.
#[derive(Debug, Default)]
pub(crate) struct WriterDrain {
    deadline: Option<Instant>,
    gave_up: bool,
    reported: bool,
}

impl WriterDrain {
    /// Record the actual final flush/join independently of the requested process exit code.
    pub(crate) fn observe_writer(
        &mut self,
        remaining: usize,
        observation: Option<tasty_utils::batch_writer::ShutdownReport>,
        name: &str,
        now: Instant,
        timed_out: &str,
    ) -> bool {
        if self.keep_waiting(remaining, now, timed_out) {
            return true;
        }
        if !std::mem::replace(&mut self.reported, true) {
            record_writer_outcome(
                name,
                observation,
                self.gave_up,
                self.deadline.is_none(),
                remaining,
            );
        }
        false
    }

    /// 남은 worker 수를 받아 더 기다릴지 돌려준다. 기한은 처음 기다릴 때 정하고, 넘기면
    /// `timed_out`을 경고로 한 번 남긴 뒤 진행한다.
    pub(crate) fn keep_waiting(&mut self, remaining: usize, now: Instant, timed_out: &str) -> bool {
        if remaining == 0 || self.gave_up {
            return false;
        }
        let deadline = *self.deadline.get_or_insert(now + WRITER_DRAIN_TIMEOUT);
        if now < deadline {
            return true;
        }
        tracing::warn!("{timed_out}");
        self.gave_up = true;
        false
    }
}

fn record_writer_outcome(
    name: &str,
    observation: Option<tasty_utils::batch_writer::ShutdownReport>,
    timed_out: bool,
    immediate: bool,
    remaining: usize,
) {
    let Some(observation) = observation else {
        tracing::info!(target: "tasty::shutdown", writer = name,
            configured = false, "writer_shutdown_observation");
        return;
    };
    record_configured_writer(name, observation, timed_out, immediate, remaining);
}

fn record_configured_writer(
    name: &str,
    report: tasty_utils::batch_writer::ShutdownReport,
    timed_out: bool,
    immediate: bool,
    remaining: usize,
) {
    use tasty_utils::batch_writer::ShutdownObservation;
    let observation = report.observation;
    let elapsed_us = report.timing.map(|t| t.elapsed.as_micros());
    let joined_us = report
        .timing
        .and_then(|t| t.joined_after)
        .map(|d| d.as_micros());
    let right_censored = timed_out && joined_us.is_none();
    let immediate = immediate && joined_us.is_some();
    let clean = matches!(
        &observation,
        ShutdownObservation::Joined {
            panicked: false,
            flush: Ok(_),
        }
    );
    if clean {
        tracing::info!(target: "tasty::shutdown", writer = name,
            ?observation, clean, timed_out, immediate, remaining, ?elapsed_us, ?joined_us, right_censored, "writer_shutdown_observation");
    } else {
        tracing::warn!(target: "tasty::shutdown", writer = name,
            ?observation, clean, timed_out, immediate, remaining, ?elapsed_us, ?joined_us, right_censored, "writer_shutdown_observation");
    }
}

#[cfg(test)]
mod tests;
