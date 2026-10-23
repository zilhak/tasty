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
        observation: Option<tasty_utils::batch_writer::ShutdownObservation>,
        name: &str,
        now: Instant,
        timed_out: &str,
    ) -> bool {
        if self.keep_waiting(remaining, now, timed_out) {
            return true;
        }
        if !std::mem::replace(&mut self.reported, true) {
            record_writer_outcome(name, observation, self.gave_up);
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
    observation: Option<tasty_utils::batch_writer::ShutdownObservation>,
    timed_out: bool,
) {
    let Some(observation) = observation else {
        tracing::info!(target: "tasty::shutdown", writer = name,
            configured = false, "writer_shutdown_observation");
        return;
    };
    record_configured_writer(name, observation, timed_out);
}

fn record_configured_writer(
    name: &str,
    observation: tasty_utils::batch_writer::ShutdownObservation,
    timed_out: bool,
) {
    use tasty_utils::batch_writer::ShutdownObservation;
    let clean = matches!(
        &observation,
        ShutdownObservation::Joined {
            panicked: false,
            flush: Ok(_),
        }
    );
    if clean {
        tracing::info!(target: "tasty::shutdown", writer = name,
            ?observation, clean, "writer_shutdown_observation");
    } else {
        tracing::warn!(target: "tasty::shutdown", writer = name,
            ?observation, clean, timed_out, "writer_shutdown_observation");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_until_the_deadline_then_gives_up_once() {
        let mut drain = WriterDrain::default();
        let start = Instant::now();
        assert!(drain.keep_waiting(1, start, "x"));
        assert!(drain.keep_waiting(1, start + WRITER_DRAIN_TIMEOUT / 2, "x"));
        // 기한은 첫 대기에서 정해지고 뒤의 대기가 늘리지 않는다.
        assert!(!drain.keep_waiting(1, start + WRITER_DRAIN_TIMEOUT, "x"));
        assert!(
            !drain.keep_waiting(1, start, "x"),
            "a given-up drain never waits again"
        );
    }

    #[test]
    fn a_joined_writer_never_waits() {
        let mut drain = WriterDrain::default();
        assert!(!drain.keep_waiting(0, Instant::now(), "x"));
    }
}
