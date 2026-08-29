//! 데드라인 기반 wakeup 스레드 — 허브가 정한 다음 데드라인까지만 자고 호스트를 깨운다.
//!
//! 고정 주기 ticker 스레드를 대체한다. 주기가 고정이 아니라 [`TimerHub::next_deadline`]
//! 이므로 등록된 타이머가 없으면 무기한 park 하고(= idle wakeup 0), 타이머가 등록되면
//! `Condvar` 로 즉시 재무장한다.
//!
//! **이벤트 루프의 `WaitUntil` 대신 이 스레드가 정확성을 책임지는 이유**: 창이 없거나
//! (macOS 최소화 = window 파괴, tray 상주) 사실상 없는 상태에서 이벤트 루프가 계속
//! 깨어난다는 보장이 플랫폼마다 다르다. 근거·판단 전체는 `docs/dev-guide/timer-hub.md`.
//!
//! [`TimerHub::next_deadline`]: crate::TimerHub::next_deadline

use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug)]
struct State {
    /// 다음에 호스트를 깨울 시각. `None` = 깨울 이유 없음(무기한 park).
    deadline: Option<Instant>,
    stopped: bool,
}

#[derive(Debug)]
struct TimerWaker {
    state: Mutex<State>,
    cv: Condvar,
}

impl TimerWaker {
    fn set_deadline(&self, at: Option<Instant>) {
        let mut st = self.lock();
        if st.deadline == at {
            return;
        }
        st.deadline = at;
        drop(st);
        self.cv.notify_all();
    }

    fn stop(&self) {
        let mut st = self.lock();
        st.stopped = true;
        drop(st);
        self.cv.notify_all();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // poisoned = waker 스레드가 panic 한 것. 타이머 스케줄은 복구 가능한 상태
        // (다음 set_deadline 이 통째로 덮어쓴다)라 그대로 이어서 쓴다.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// waker 스레드 핸들. drop 되면 스레드에 정지를 알린다(join 하지 않는다 — 종료
/// 경로에서 최대 한 번의 `Condvar` 깨우기만 남는다).
#[derive(Debug)]
pub struct TimerWakerHandle {
    waker: Arc<TimerWaker>,
}

impl TimerWakerHandle {
    /// 다음에 호스트를 깨울 시각을 갱신한다. 매 프레임 말미에
    /// `hub.next_deadline()` 을 그대로 넘기면 된다(같은 값이면 no-op).
    pub fn set_deadline(&self, at: Option<Instant>) {
        self.waker.set_deadline(at);
    }
}

impl Drop for TimerWakerHandle {
    fn drop(&mut self) {
        self.waker.stop();
    }
}

/// 데드라인 waker 스레드를 띄운다.
///
/// `fire` 는 데드라인에 도달할 때마다 waker 스레드에서 호출된다 — 호스트 이벤트
/// 루프를 깨우기만 하는 얇은 클로저여야 한다(실제 타이머 실행은 메인 스레드의
/// `drain_due`). `false` 를 반환하면(= 수신 측이 사라졌다) 스레드가 종료한다.
pub fn spawn_timer_waker(mut fire: impl FnMut() -> bool + Send + 'static) -> TimerWakerHandle {
    let waker = Arc::new(TimerWaker {
        state: Mutex::new(State {
            deadline: None,
            stopped: false,
        }),
        cv: Condvar::new(),
    });
    let thread_waker = Arc::clone(&waker);
    let spawned = std::thread::Builder::new()
        .name("tasty-timer-waker".to_string())
        .spawn(move || {
            while wait_for_deadline(&thread_waker) {
                if !fire() {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        // 스레드를 못 띄우면 시간축이 통째로 멈춘다 — 조용히 넘기지 않는다.
        // (호스트는 계속 동작하되 주기 작업이 다른 wakeup 에 편승하게 된다.)
        tracing::error!("timer waker thread spawn failed: {e}");
    }
    TimerWakerHandle { waker }
}

/// 다음 데드라인까지 park 한다. `true` = 데드라인 도달(발화해야 함),
/// `false` = 정지 요청.
fn wait_for_deadline(waker: &TimerWaker) -> bool {
    let mut st = waker.lock();
    loop {
        if st.stopped {
            return false;
        }
        match st.deadline {
            // 깨울 이유가 없다 — 등록이 생길 때까지 무기한 park(idle wakeup 0).
            None => {
                st = waker
                    .cv
                    .wait(st)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            Some(at) => {
                let now = Instant::now();
                if at <= now {
                    // 발화. 다음 데드라인은 메인 루프가 프레임 말미에 다시 알려준다 —
                    // 여기서 비워두지 않으면 같은 데드라인으로 즉시 재발화해 spin 한다.
                    st.deadline = None;
                    return true;
                }
                let (next, _) = waker
                    .cv
                    .wait_timeout(st, at - now)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                st = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    /// 데드라인이 설정되면 그 시각에 fire 가 호출된다.
    #[test]
    fn fires_at_the_deadline() {
        let (tx, rx) = mpsc::channel();
        let handle = spawn_timer_waker(move || tx.send(()).is_ok());
        handle.set_deadline(Some(Instant::now() + Duration::from_millis(20)));
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "데드라인이 지났는데 waker 가 깨우지 않았다"
        );
    }

    /// 데드라인이 없으면 park 한 채 아무것도 발화하지 않는다(idle wakeup 0).
    #[test]
    fn idle_hub_never_fires() {
        let (tx, rx) = mpsc::channel();
        let _handle = spawn_timer_waker(move || tx.send(()).is_ok());
        assert!(
            rx.recv_timeout(Duration::from_millis(120)).is_err(),
            "등록된 타이머가 없는데 waker 가 깨웠다"
        );
    }

    /// 더 이른 데드라인으로 갱신하면 기존 park 를 중단하고 앞당겨 깨운다.
    #[test]
    fn earlier_deadline_preempts_the_current_wait() {
        let (tx, rx) = mpsc::channel();
        let handle = spawn_timer_waker(move || tx.send(()).is_ok());
        handle.set_deadline(Some(Instant::now() + Duration::from_secs(30)));
        handle.set_deadline(Some(Instant::now() + Duration::from_millis(20)));
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "앞당긴 데드라인이 반영되지 않았다"
        );
    }

    /// fire 가 false 를 반환하면(수신 측 소멸) 스레드가 종료한다.
    #[test]
    fn stops_when_fire_reports_a_dead_receiver() {
        let (tx, rx) = mpsc::channel();
        let handle = spawn_timer_waker(move || tx.send(()).is_ok());
        handle.set_deadline(Some(Instant::now() + Duration::from_millis(10)));
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(rx);
        // 수신 측이 사라진 뒤의 데드라인은 한 번 발화하고 스레드를 끝낸다.
        handle.set_deadline(Some(Instant::now() + Duration::from_millis(10)));
    }
}
