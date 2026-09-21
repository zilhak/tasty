//! accept 대기의 **상한**을 재는 시계.
//!
//! OS 가 연결을 accept 큐에 넣은 시각은 사용자 공간에서 안 보인다. accept 루프가 볼 수 있는
//! 것은 큐가 **비어 있었던 마지막 순간**(논블로킹 `accept` 가 `WouldBlock` 을 돌려준 순간)이고,
//! 그 뒤에 꺼낸 연결은 그 순간 **뒤에** 도착했다. 그래서 `꺼낸 시각 − 마지막으로 비어 있던 시각`
//! 은 그 연결이 큐에서 기다린 시간의 상한이다 — 실제 대기는 그보다 길 수 없다.
//!
//! 루프는 빈 큐를 보면 100 ms 자므로(`TcpIpcServer::start_with_port_file`) 이 값은 대개 0–100 ms
//! 이고, 잠든 동안 고르게 도착한 연결의 실제 대기는 평균적으로 그 절반이다. 연속으로 꺼내는 동안
//! (큐에 여럿이 쌓여 있던 때)에는 "마지막으로 비어 있던 순간" 이 안 바뀌므로 뒤에 꺼낸 것일수록
//! 상한이 커진다 — 그것들도 그 순간 뒤에 도착했다는 것만 확실하다.

use std::time::{Duration, Instant};

pub(super) struct AcceptClock {
    last_empty: Instant,
}

impl AcceptClock {
    /// 리스너를 막 열었다 — 그 순간 큐는 비어 있었다.
    pub(super) fn new(now: Instant) -> Self {
        Self { last_empty: now }
    }

    /// `accept` 가 `WouldBlock` 을 돌려줬다 — 큐가 지금 비어 있다.
    pub(super) fn saw_empty(&mut self, now: Instant) {
        self.last_empty = now;
    }

    /// `now` 에 꺼낸 연결이 큐에서 기다렸을 수 있는 시간의 상한.
    pub(super) fn bound(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.last_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::AcceptClock;
    use std::time::{Duration, Instant};

    /// 상한은 큐가 마지막으로 비어 있던 순간부터 잰다 — 연결이 잠든 동안 도착했어도 그 순간이
    /// 기준이다.
    #[test]
    fn the_bound_runs_from_the_last_empty_queue() {
        let t0 = Instant::now();
        let mut clock = AcceptClock::new(t0);
        clock.saw_empty(t0 + Duration::from_millis(10));
        assert_eq!(
            clock.bound(t0 + Duration::from_millis(110)),
            Duration::from_millis(100)
        );
    }

    /// 연속으로 꺼내는 동안에는 기준이 안 바뀐다 — 뒤에 꺼낸 것의 상한이 더 크다.
    #[test]
    fn a_burst_keeps_the_same_reference() {
        let t0 = Instant::now();
        let clock = AcceptClock::new(t0);
        let first = clock.bound(t0 + Duration::from_millis(5));
        let second = clock.bound(t0 + Duration::from_millis(6));
        assert!(second > first);
        assert_eq!(first, Duration::from_millis(5));
    }
}

#[cfg(test)]
mod loop_tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use tasty_ipc::stream_hub::{StreamContext, StreamHub};
    use tasty_telemetry::ConnectionStats;

    use super::super::TcpIpcServer;

    /// accept 루프가 꺼낸 연결마다 상한을 기록한다 — 자리를 받은 연결이든 거절된 연결이든
    /// 같은 모수(`accepted + refused_saturated`)다. 이 호출이 루프에서 빠지면 `accept_waits` 가
    /// 0 에 머문다.
    #[test]
    fn the_accept_loop_records_a_bound_for_every_connection_it_takes() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let dir = tempfile::tempdir().expect("tempdir");
        let port_file = dir.path().join("tasty.port");
        let (inbound_tx, _inbound_rx) = std::sync::mpsc::channel();
        let ctx = StreamContext {
            hub: StreamHub::new(),
            inbound_tx,
            waker: Arc::new(|| {}),
        };
        let stats = Arc::new(ConnectionStats::default());
        let server = TcpIpcServer::start_with_port_file(
            Some(port_file.to_string_lossy().into_owned()),
            None,
            ctx,
            stats.clone(),
        )
        .expect("server");
        let addr = format!("127.0.0.1:{}", server.port);
        let clients: Vec<_> = (0..2)
            .map(|_| std::net::TcpStream::connect(&addr).expect("connect"))
            .collect();

        let deadline = Instant::now() + Duration::from_secs(5);
        let s = loop {
            let s = stats.snapshot();
            if s.accept_waits >= 2 || Instant::now() > deadline {
                break s;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            s.accept_waits, 2,
            "연결 둘을 꺼냈는데 기록이 {}",
            s.accept_waits
        );
        assert_eq!(s.accept_waits, s.accepted + s.refused_saturated);
        assert!(
            s.accept_wait_bound_us_max < 5_000_000,
            "상한이 루프의 잠(100 ms) 규모를 한참 넘었다: {} µs",
            s.accept_wait_bound_us_max
        );
        drop(clients);
        drop(server);
    }
}
