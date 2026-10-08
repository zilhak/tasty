//! journal 시험의 정체 감지.
//!
//! 시험 스레드가 폴링 사이에 쉰 시간 가운데 worker가 잠들어 있던 시간만 정체로 센다.
//! - 폴링 호출 안에서 시험 스레드가 직접 하는 작업은 세지 않는다. 그 호출이 돌아오지 않으면
//!   이 검사로는 원래 잡을 수 없다.
//! - worker가 실행 중이거나 디스크 I/O를 기다리는 시간도 세지 않는다. 같은 디스크를 쓰는 다른
//!   프로세스 때문에 fsync가 몇 초씩 늦어지면 벽시계 기한은 교착이 없어도 실패했다.
//! - worker가 잠든 채 진행이 없으면 그 시간은 정체다. 응답을 잃었거나 잠금을 기다리며 멈춘
//!   교착이 이렇게 보이므로 [`StallBudget::IDLE_LIMIT`] 안에 실패한다.
//!
//! worker 상태는 Linux에서만 읽는다. 다른 OS에서는 쉰 시간을 모두 센다.
//! 끝없이 도는 worker는 잠들지 않으므로 [`StallBudget::CEILING`]의 벽시계 상한으로 잡는다.
//! 정체로 판정하면 패닉 전에 worker를 버린다고 표시한다. 그래야 풀리는 중의 Drop이 멈춘
//! worker를 join하지 않고 시험이 실패로 끝난다.

use std::time::{Duration, Instant};

use super::JournalWorker;
use super::thread_probe::ThreadProbe;

pub(crate) struct StallBudget {
    idle: Duration,
    started: Instant,
    worker: ThreadProbe,
}

impl StallBudget {
    /// worker가 잠든 채 진행이 없는 시간의 한도.
    pub(crate) const IDLE_LIMIT: Duration = Duration::from_secs(10);
    /// worker가 일하는 중이어도 넘지 않는 벽시계 상한. 끝없이 도는 worker를 잡는다.
    pub(crate) const CEILING: Duration = Duration::from_secs(300);

    pub(crate) fn for_worker(worker: &JournalWorker) -> Self {
        Self {
            idle: Duration::ZERO,
            started: Instant::now(),
            worker: worker.thread_probe.clone(),
        }
    }

    /// 새로 띄운 worker를 지켜본다. 쌓인 정체 시간은 유지한다.
    pub(crate) fn watch_worker(&mut self, worker: &JournalWorker) {
        self.worker = worker.thread_probe.clone();
    }

    /// 한도를 넘었으면 실패하고, 아니면 잠깐 쉰다.
    pub(crate) fn nap(&mut self, what: &str) {
        self.nap_with(|| what.to_owned());
    }

    /// [`Self::nap`]과 같다. 실패 메시지의 상태 덤프는 실패할 때만 만든다.
    pub(crate) fn nap_with(&mut self, what: impl FnOnce() -> String) {
        if self.idle >= Self::IDLE_LIMIT {
            self.worker.abandon();
            panic!(
                "{} stalled: the journal worker slept {:?} without progress",
                what(),
                self.idle
            );
        }
        if self.started.elapsed() >= Self::CEILING {
            self.worker.abandon();
            panic!(
                "{} stalled: still waiting after {:?} although the journal worker was busy",
                what(),
                self.started.elapsed()
            );
        }
        let napped = Instant::now();
        std::thread::sleep(Duration::from_millis(1));
        if !self.worker.is_working() {
            self.idle += napped.elapsed();
        }
    }
}
