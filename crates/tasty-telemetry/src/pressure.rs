//! 요청 압력 계측 — 큐 대기 · 큐 깊이 · handler 실행 시간 · 연결 자리.
//!
//! 이 모듈이 생기기 전 호스트가 IPC 경로에서 남기던 값은 `ipc_calls` 카운터 하나였고,
//! 요청이 **얼마나 밀렸는지 · 얼마나 걸렸는지**를 재는 자리는 한 곳도 없었다. 큐에서
//! 기다린 시간과 handler 안에서 보낸 시간이 구분되지 않으면, 느린 응답을 보고도 그것이
//! 적체인지 handler 비용인지 고를 수 없다. 이 집계가 그 둘을 다른 값으로 만든다.
//!
//! **왜 영구 기록이 아닌가**: 기존 `TelemetryEvent` 경로는 호출마다 저장소에 한 행을
//! 남긴다. 지연을 그렇게 재면 진단 자체가 호출당 기록 폭주를 다시 만든다. 그래서 여기
//! 값들은 프로세스 안에만 있고 크기가 고정이다 — 게이지마다 원자값 몇 개씩이고 그 수가
//! 호출 수와 무관하다.
//!
//! **시간 축과 자원 축이 따로 있다**: 앞의 셋은 *얼마나 걸렸나* 를 재고
//! [`ConnectionStats`] 는 *자리가 남았나* 를 잰다. 요청이 하나도 안 느려도 연결 자리가
//! 차면 새 client 는 못 붙으므로, 시간만 재는 게이지로는 그 포화가 안 보인다.
//!
//! **시간 축은 평균·최대 옆에 분포를 함께 든다**([`LatencyHistogram`]). 평균과 최대만
//! 있으면 답하지 못하는 물음이 있다 — "전부 조금씩 느린가, 대부분 빠른데 꼬리가 몇 건
//! 있는가". 두 상태는 같은 평균과 같은 최대를 낼 수 있고, 처방이 반대다(앞은 용량,
//! 뒤는 그 몇 건의 원인). 버킷은 고정 경계라 관측 수와 무관하게 크기가 안 자란다 —
//! 그것이 분위수를 정확히 주는 대신 버킷 해상도로 접는 대가다.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// 지연 버킷의 상한(마이크로초, `le` — 상한과 같은 값은 그 버킷에 든다).
///
/// **이 경계는 파생되지 않는다** — 고른 값이다. 고른 근거는 두 가지다. ① 실측:
/// ADR-0333 이 격리 인스턴스에서 잰 값이 큐 대기 max 25.4 ms · handler max 0.48 ms
/// 였고, plugin 왕복은 초 단위까지 간다. ② 해상도: 반-십진(√10 ≈ 3.16 배) 간격이라
/// 10 µs 부터 1 s 까지 열한 칸으로 덮는다. 이 간격은 "밀렸다" 와 "안 밀렸다" 를 가르기에
/// 충분하고, 그보다 촘촘하게 하면 게이지가 갖고 있지도 않은 정밀도를 말하게 된다.
///
/// 상한을 넘은 관측은 [`LATENCY_BUCKET_BOUNDS_US`] 밖의 마지막 칸으로 간다. 그 칸에는
/// 상한이 없으므로 **"1 초를 넘었다" 까지만 말하고 얼마나 넘었는지는 max 가 답한다** —
/// histogram 과 max 를 함께 두는 이유가 그것이다.
pub const LATENCY_BUCKET_BOUNDS_US: [u64; 11] = [
    10, 32, 100, 316, 1_000, 3_162, 10_000, 31_623, 100_000, 316_228, 1_000_000,
];

/// 버킷 수 = 유한 상한 수 + 넘침 한 칸.
pub const LATENCY_BUCKET_COUNT: usize = LATENCY_BUCKET_BOUNDS_US.len() + 1;

/// 관측이 어느 버킷에 드는지. 상한이 열한 개뿐이라 선형 탐색이 이분 탐색에 안 진다.
fn bucket_of(us: u64) -> usize {
    LATENCY_BUCKET_BOUNDS_US
        .iter()
        .position(|&bound| us <= bound)
        .unwrap_or(LATENCY_BUCKET_BOUNDS_US.len())
}

/// 고정 경계 지연 histogram. 관측 수와 무관하게 원자값 [`LATENCY_BUCKET_COUNT`] 개다.
///
/// 이 크레이트의 다른 게이지와 같은 성질을 공유한다: 프로세스 수명 누계 · 고정 크기 ·
/// `Relaxed` · 스냅샷이 원자적이지 않음. 스냅샷이 원자적이지 않다는 것은 여기서 한 가지
/// 형태로 드러난다 — **칸들의 합이 같은 순간의 `count` 와 안 맞을 수 있다.** 읽는 도중
/// 다른 스레드가 올린 것이 뒤쪽 칸에만 반영되기 때문이다. 진단값이라 허용한다.
#[derive(Debug, Default)]
pub struct LatencyHistogram {
    counts: [AtomicU64; LATENCY_BUCKET_COUNT],
}

impl LatencyHistogram {
    /// 관측 하나를 접는다. 1 마이크로초 미만은 `as_micros` 가 0 으로 접으므로 첫 칸에
    /// 든다 — 그 칸이 "10 µs 이하" 라 옳다.
    pub fn record_us(&self, us: u64) {
        self.counts[bucket_of(us)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> HistogramSnapshot {
        let mut counts = [0u64; LATENCY_BUCKET_COUNT];
        for (dst, src) in counts.iter_mut().zip(self.counts.iter()) {
            *dst = src.load(Ordering::Relaxed);
        }
        HistogramSnapshot { counts }
    }
}

/// [`LatencyHistogram`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct HistogramSnapshot {
    /// 버킷마다의 관측 수. **누적이 아니다** — 칸끼리 겹치지 않고 합이 전체 관측 수다
    /// (Prometheus 의 `le` 누적 버킷과 다르다). 마지막 칸은 마지막 상한을 넘은 것들이다.
    pub counts: [u64; LATENCY_BUCKET_COUNT],
}

impl HistogramSnapshot {
    /// 칸에 붙는 상한. 값과 경계를 **같은 자리에서** 내보내려고 여기 둔다 — 소비자가
    /// 경계를 자기 쪽에 복제하면 그 복제본이 갈린다.
    pub fn bounds_us() -> &'static [u64; LATENCY_BUCKET_BOUNDS_US.len()] {
        &LATENCY_BUCKET_BOUNDS_US
    }

    /// 접힌 관측 수의 합.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }
}

/// 프로세스 수명 동안 누적되는 고정 크기 압력 집계.
///
/// 모든 갱신이 `Relaxed` 다 — 값들 사이에 순서 불변식이 없고 각각이 독립 카운터라,
/// 더 강한 순서를 요구하면 비용만 늘고 얻는 것이 없다. `snapshot` 이 여러 원자를 따로
/// 읽으므로 **한 스냅샷 안의 값들이 같은 순간의 것은 아니다**(예: `handler_calls` 를
/// 읽은 뒤 다른 스레드가 `handler_us_sum` 을 올릴 수 있다). 진단값이라 그 정도의
/// 어긋남은 허용하고, 대신 그 사실을 여기 적어 둔다.
#[derive(Debug, Default)]
pub struct PressureStats {
    /// 큐를 비운 횟수(= 비어 있지 않은 drain 만).
    queue_drains: AtomicU64,
    /// 한 번의 drain 이 집어 든 명령 수의 최댓값 = 관측된 최대 큐 깊이.
    queue_depth_max: AtomicU64,
    /// drain 으로 집어 든 명령 수의 합. **회차가 끝날 때** 오른다.
    queue_commands: AtomicU64,
    /// 대기를 기록한 명령 수 — 아래 합·최댓값·분포와 **같은 자리**(명령을 꺼낼 때)에서 오른다.
    /// 평균의 분모가 이것이다. `queue_commands` 를 분모로 쓰면 아직 안 끝난 회차(조회 자신의
    /// 회차 포함)가 꺼낸 명령의 대기는 분자에 있고 분모에 없어, 평균이 최댓값을 넘는다.
    queue_waits: AtomicU64,
    /// 큐에 들어간 뒤 꺼내질 때까지의 대기 시간 합(마이크로초).
    queue_wait_us_sum: AtomicU64,
    /// 그 대기 시간의 최댓값(마이크로초).
    queue_wait_us_max: AtomicU64,
    /// handler 를 실행한 횟수.
    handler_calls: AtomicU64,
    /// handler 실행 시간 합(마이크로초).
    handler_us_sum: AtomicU64,
    /// handler 실행 시간 최댓값(마이크로초).
    handler_us_max: AtomicU64,
    /// 큐 대기 시간의 분포. sum·max 가 못 답하는 "꼬리인가 전체인가" 를 답한다.
    queue_wait_hist: LatencyHistogram,
    /// handler 실행 시간의 분포. 위와 같은 이유로 따로 든다 — 두 모수를 한 histogram
    /// 에 접으면 게이트 앞뒤가 다시 섞인다.
    handler_hist: LatencyHistogram,
}

/// 마이크로초로 접는다. 나노초를 그대로 더하면 `u64` 가 약 584 년에 넘치는데, 그보다
/// 실질적인 이유는 이 값들이 **사람이 읽는 진단값**이라 나노초 해상도가 의미가 없다는
/// 것이다. 1 마이크로초 미만은 0 으로 접힌다 — `count` 는 그대로 오르므로 "아주 빠른
/// 호출이 많았다" 와 "호출이 없었다" 는 구분된다.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

impl PressureStats {
    /// 큐를 한 번 비웠다 — 그때 집어 든 명령 수를 깊이로 기록한다.
    ///
    /// 빈 drain 은 호출하지 않는다(호출부가 비면 일찍 빠진다). 그래서 `queue_drains` 는
    /// "명령이 있었던 프레임 수" 이고, 프레임 수가 아니다.
    pub fn record_drain(&self, depth: usize) {
        let depth = depth as u64;
        self.queue_drains.fetch_add(1, Ordering::Relaxed);
        self.queue_commands.fetch_add(depth, Ordering::Relaxed);
        self.queue_depth_max.fetch_max(depth, Ordering::Relaxed);
    }

    /// 명령 하나가 큐에서 기다린 시간.
    pub fn record_queue_wait(&self, waited: Duration) {
        let us = as_micros(waited);
        self.queue_waits.fetch_add(1, Ordering::Relaxed);
        self.queue_wait_us_sum.fetch_add(us, Ordering::Relaxed);
        self.queue_wait_us_max.fetch_max(us, Ordering::Relaxed);
        self.queue_wait_hist.record_us(us);
    }

    /// handler 하나가 실행에 쓴 시간. 큐 대기는 포함하지 않는다 — 그것이 이 둘을
    /// 따로 재는 이유다.
    pub fn record_handler(&self, elapsed: Duration) {
        let us = as_micros(elapsed);
        self.handler_calls.fetch_add(1, Ordering::Relaxed);
        self.handler_us_sum.fetch_add(us, Ordering::Relaxed);
        self.handler_us_max.fetch_max(us, Ordering::Relaxed);
        self.handler_hist.record_us(us);
    }

    /// 지금까지의 누계를 한 덩어리로 읽는다. 위 struct 주석대로 **원자적 스냅샷이
    /// 아니다.**
    pub fn snapshot(&self) -> PressureSnapshot {
        PressureSnapshot {
            queue_drains: self.queue_drains.load(Ordering::Relaxed),
            queue_depth_max: self.queue_depth_max.load(Ordering::Relaxed),
            queue_commands: self.queue_commands.load(Ordering::Relaxed),
            queue_waits: self.queue_waits.load(Ordering::Relaxed),
            queue_wait_us_sum: self.queue_wait_us_sum.load(Ordering::Relaxed),
            queue_wait_us_max: self.queue_wait_us_max.load(Ordering::Relaxed),
            handler_calls: self.handler_calls.load(Ordering::Relaxed),
            handler_us_sum: self.handler_us_sum.load(Ordering::Relaxed),
            handler_us_max: self.handler_us_max.load(Ordering::Relaxed),
            queue_wait_hist: self.queue_wait_hist.snapshot(),
            handler_hist: self.handler_hist.snapshot(),
        }
    }
}

/// [`PressureStats`] 의 한 시점 읽기. 평균은 파생값이라 필드로 두지 않고 메서드로 낸다 —
/// 분모가 0 인 경우를 소비자마다 다르게 처리하지 않게 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PressureSnapshot {
    pub queue_drains: u64,
    pub queue_depth_max: u64,
    pub queue_commands: u64,
    pub queue_waits: u64,
    pub queue_wait_us_sum: u64,
    pub queue_wait_us_max: u64,
    pub handler_calls: u64,
    pub handler_us_sum: u64,
    pub handler_us_max: u64,
    pub queue_wait_hist: HistogramSnapshot,
    pub handler_hist: HistogramSnapshot,
}

impl PressureSnapshot {
    /// 명령 하나가 큐에서 기다린 평균(마이크로초). 관측이 없으면 `None` — 0 을 돌려주면
    /// "기다림이 없었다" 와 "잰 적이 없다" 가 같은 값이 된다.
    ///
    /// 분모는 대기를 기록한 명령 수(`queue_waits`)다 — 합과 같은 자리에서 오르는 수라야
    /// 평균이 최댓값을 넘지 않는다. `queue_commands` 는 회차가 끝날 때 오르므로 한 스냅샷
    /// 안에서 분자보다 늦다(ADR-0466).
    pub fn queue_wait_us_mean(&self) -> Option<u64> {
        (self.queue_waits > 0).then(|| self.queue_wait_us_sum / self.queue_waits)
    }

    /// handler 하나의 평균 실행 시간(마이크로초). 위와 같은 이유로 `Option`.
    pub fn handler_us_mean(&self) -> Option<u64> {
        (self.handler_calls > 0).then(|| self.handler_us_sum / self.handler_calls)
    }

    /// drain 한 번이 집어 든 평균 명령 수. 이 값이 1 에 가까우면 적체가 없는 것이고,
    /// 크면 한 프레임이 여러 요청을 몰아 처리하고 있다는 뜻이다.
    pub fn queue_depth_mean(&self) -> Option<u64> {
        (self.queue_drains > 0).then(|| self.queue_commands / self.queue_drains)
    }
}

/// host→plugin 요청 하나가 **응답을 기다린 시간**의 누계.
///
/// [`PressureStats`] 와 **다른 축**이다. 그쪽 셋은 호스트가 자기 큐와 자기 handler 에서
/// 보낸 시간이고, 이 값은 호스트가 **남의 프로세스를 기다린** 시간이다. 느린 응답의
/// 원인을 고를 때 이 구분이 답을 가른다 — 큐도 handler 도 빠른데 응답이 느리면 그
/// 요청은 plugin 안에 있었던 것이다.
///
/// 별도 타입인 이유는 재는 주체가 다르기 때문이다. `PressureStats` 는 본 바이너리의
/// `Core` 가 들고 관측 자리 셋이 쓰는데, 이 값을 올리는 자리는 `tasty-host-plugin` 의
/// 응답 매칭부 하나뿐이고 그 크레이트는 `Core` 를 못 본다. 한 타입에 다 넣으면 어느
/// 인스턴스가 어느 필드를 채우는지가 타입으로 안 보인다.
///
/// `PressureStats` 와 같은 성질을 공유한다: 프로세스 수명 누계 · 고정 크기 · `Relaxed` ·
/// 스냅샷이 원자적이지 않음 · 1 마이크로초 미만은 0 으로 접히되 횟수는 오름.
#[derive(Debug, Default)]
pub struct PluginWaitStats {
    /// 응답이 **매칭된** 요청 수. 응답이 영영 안 온 요청은 여기 안 센다.
    matched: AtomicU64,
    /// 그 왕복 대기 시간 합(마이크로초).
    us_sum: AtomicU64,
    /// 그 최댓값(마이크로초).
    us_max: AtomicU64,
    /// 왕복 시간의 분포. plugin 이 대체로 빠른데 몇 건만 초 단위인지, 전부 느린지를
    /// 가른다 — 앞은 그 몇 건의 일이고 뒤는 plugin 자체의 일이다.
    hist: LatencyHistogram,
}

impl PluginWaitStats {
    /// 요청 하나의 응답이 도착했다 — 보낸 뒤 흐른 시간을 접는다.
    pub fn record(&self, waited: Duration) {
        let us = as_micros(waited);
        self.matched.fetch_add(1, Ordering::Relaxed);
        self.us_sum.fetch_add(us, Ordering::Relaxed);
        self.us_max.fetch_max(us, Ordering::Relaxed);
        self.hist.record_us(us);
    }

    pub fn snapshot(&self) -> PluginWaitSnapshot {
        PluginWaitSnapshot {
            matched: self.matched.load(Ordering::Relaxed),
            us_sum: self.us_sum.load(Ordering::Relaxed),
            us_max: self.us_max.load(Ordering::Relaxed),
            hist: self.hist.snapshot(),
        }
    }
}

/// [`PluginWaitStats`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct PluginWaitSnapshot {
    pub matched: u64,
    pub us_sum: u64,
    pub us_max: u64,
    pub hist: HistogramSnapshot,
}

impl PluginWaitSnapshot {
    /// 왕복 하나의 평균(마이크로초). 관측이 없으면 `None` — 위와 같은 이유다.
    pub fn us_mean(&self) -> Option<u64> {
        (self.matched > 0).then(|| self.us_sum / self.matched)
    }
}

/// 동시에 살아 있는 IPC 연결 수 게이지.
///
/// 위 셋과 **성질이 다르다.** `PressureStats` · `PluginWaitStats` 는 전부 *시간*
/// 누계이고 "느렸다" 를 말한다. 이 값은 *자원 점유*이고 "자리가 없다" 를 말한다 —
/// 요청이 하나도 안 느려도 연결 자리가 차면 새 client 는 아예 못 붙는다. 그 거절은
/// 요청이 되기 전에 일어나므로 위 셋 어디에도 흔적이 안 남고, `ipc_calls` 에도 안
/// 남는다(JSON-RPC 요청이 아니라 TCP 연결이다).
///
/// 재는 주체는 accept 루프 하나(`TcpIpcServer`)이고 자리 반납은 각 연결 스레드의
/// `Drop` 이다. 그래서 `Arc` 이고 모든 갱신이 원자적이다.
///
/// **상한은 여기 없다.** 상한은 서버의 정책이라 [`ConnectionStats::try_open`] 이
/// 인자로 받는다 — 게이지가 정책을 들면 그 값이 두 곳에 살게 된다.
#[derive(Debug, Default)]
pub struct ConnectionStats {
    /// 지금 살아 있는 연결 수. **이것만 내려가는 값이다** — 나머지 필드는 전부 누계나
    /// 최댓값이고 이것은 순간값이다(스냅샷의 accept 대기 평균은 파생값이라 내려갈 수 있다).
    live: AtomicU64,
    /// 관측된 `live` 의 최댓값. 누계 게이지가 순간값만 주면 "지금은 비었지만 아까
    /// 꽉 찼었다" 를 못 본다.
    live_max: AtomicU64,
    /// 자리를 받아 간 연결 수의 누계.
    accepted: AtomicU64,
    /// 상한에 걸려 거절된 연결 수의 누계.
    refused_saturated: AtomicU64,
    /// accept 대기 상한을 기록한 연결 수 — accept 루프가 꺼낸 TCP 연결 **전부**(자리를 받은 것과
    /// 거절된 것 둘 다)다. 그래서 `accepted + refused_saturated` 와 같다.
    accept_waits: AtomicU64,
    /// 연결 하나가 OS 의 accept 큐에서 기다렸을 수 있는 시간의 **상한** 합(마이크로초). 재는 법은
    /// [`ConnectionStats::record_accept_wait`].
    accept_wait_bound_us_sum: AtomicU64,
    /// 그 상한의 최댓값(마이크로초).
    accept_wait_bound_us_max: AtomicU64,
}

impl ConnectionStats {
    /// 자리를 하나 잡아 본다. 잡았으면 **잡은 뒤의** 살아 있는 수, 상한을 넘었으면
    /// 되돌리고 `None`.
    ///
    /// 먼저 올리고 초과면 내리는 순서인 이유는 검사와 점유 사이에 틈을 안 두려는
    /// 것이다 — `load` 로 보고 `fetch_add` 하면 두 스레드가 같은 마지막 자리를
    /// 가져간다. 되돌림이 빠지면 계수가 영구히 상한 위로 떠서 자리가 다시는 안
    /// 열리므로, 그 자리는 호출부의 시험이 잡는다.
    pub fn try_open(&self, limit: u64) -> Option<u64> {
        let prev = self.live.fetch_add(1, Ordering::Relaxed);
        if prev >= limit {
            self.live.fetch_sub(1, Ordering::Relaxed);
            self.refused_saturated.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let now = prev + 1;
        self.accepted.fetch_add(1, Ordering::Relaxed);
        self.live_max.fetch_max(now, Ordering::Relaxed);
        Some(now)
    }

    /// accept 루프가 연결 하나를 꺼냈다 — 그 연결이 OS 의 accept 큐에서 기다렸을 수 있는 시간의
    /// **상한**을 기록한다.
    ///
    /// OS 가 연결을 큐에 넣은 시각은 사용자 공간에서 안 보인다. 보이는 것은 루프가 큐를 **마지막으로
    /// 비어 있다고 본 시각**(accept 가 `WouldBlock` 을 돌려준 순간)이고, 그 뒤에 꺼낸 연결은 그
    /// 시각 **뒤에** 도착했다. 그래서 `꺼낸 시각 − 마지막으로 비어 있던 시각` 은 그 연결의 대기를
    /// 넘지 않는 값이 아니라 **넘을 수 없는 값**(상한)이다. 루프가 빈 큐를 보면 100 ms 자므로 이
    /// 값은 대개 0–100 ms 이고, 잠든 동안 고르게 도착하면 실제 대기는 평균적으로 그 절반이다.
    pub fn record_accept_wait(&self, bound: Duration) {
        let us = as_micros(bound);
        self.accept_waits.fetch_add(1, Ordering::Relaxed);
        self.accept_wait_bound_us_sum
            .fetch_add(us, Ordering::Relaxed);
        self.accept_wait_bound_us_max
            .fetch_max(us, Ordering::Relaxed);
    }

    /// 자리 하나를 돌려준다. [`ConnectionStats::try_open`] 이 `Some` 을 돌려준
    /// 자리에서만 부른다 — 짝이 안 맞으면 `live` 가 0 아래로 돌아 감싼다.
    pub fn close(&self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ConnectionSnapshot {
        ConnectionSnapshot {
            live: self.live.load(Ordering::Relaxed),
            live_max: self.live_max.load(Ordering::Relaxed),
            accepted: self.accepted.load(Ordering::Relaxed),
            refused_saturated: self.refused_saturated.load(Ordering::Relaxed),
            accept_waits: self.accept_waits.load(Ordering::Relaxed),
            accept_wait_bound_us_sum: self.accept_wait_bound_us_sum.load(Ordering::Relaxed),
            accept_wait_bound_us_max: self.accept_wait_bound_us_max.load(Ordering::Relaxed),
        }
    }
}

/// [`ConnectionStats`] 의 한 시점 읽기. 상한은 서버가 아는 값이라 여기 없다 —
/// 노출부가 자기가 아는 상한과 함께 내보낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ConnectionSnapshot {
    pub live: u64,
    pub live_max: u64,
    pub accepted: u64,
    pub refused_saturated: u64,
    pub accept_waits: u64,
    pub accept_wait_bound_us_sum: u64,
    pub accept_wait_bound_us_max: u64,
}

impl ConnectionSnapshot {
    /// accept 대기 상한의 평균(마이크로초). 기록이 없으면 `None`.
    pub fn accept_wait_bound_us_mean(&self) -> Option<u64> {
        (self.accept_waits > 0).then(|| self.accept_wait_bound_us_sum / self.accept_waits)
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_aggregate_says_it_has_no_observation() {
        let s = PressureStats::default().snapshot();
        assert_eq!(s.queue_wait_us_mean(), None);
        assert_eq!(s.handler_us_mean(), None);
        assert_eq!(s.queue_depth_mean(), None);
    }

    #[test]
    fn a_drain_records_its_depth_and_keeps_the_largest() {
        let p = PressureStats::default();
        p.record_drain(3);
        p.record_drain(11);
        p.record_drain(1);
        let s = p.snapshot();
        assert_eq!(s.queue_drains, 3);
        assert_eq!(s.queue_commands, 15);
        assert_eq!(s.queue_depth_max, 11, "최댓값은 내려가지 않는다");
        assert_eq!(s.queue_depth_mean(), Some(5));
    }

    /// 회차가 아직 안 끝났어도(명령을 꺼내 대기는 기록했지만 `record_drain` 전) 평균은 최댓값을
    /// 안 넘고, 분포의 합이 평균의 분모와 같다 — 조회는 늘 자기 회차 안에서 스냅샷을 찍는다.
    #[test]
    fn the_wait_mean_shares_its_modulus_with_the_sum_while_a_round_is_open() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_micros(300));
        p.record_queue_wait(Duration::from_micros(100));
        let s = p.snapshot();
        assert_eq!(s.queue_commands, 0, "대조군: 회차가 아직 안 끝났다");
        assert_eq!(s.queue_waits, 2);
        assert_eq!(s.queue_wait_us_mean(), Some(200));
        assert!(s.queue_wait_us_mean().unwrap() <= s.queue_wait_us_max);
        assert_eq!(s.queue_wait_hist.total(), s.queue_waits);
    }

    #[test]
    fn queue_wait_and_handler_time_are_separate_values() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_millis(40));
        p.record_drain(1);
        p.record_handler(Duration::from_millis(2));
        let s = p.snapshot();
        assert_eq!(s.queue_wait_us_max, 40_000);
        assert_eq!(s.handler_us_max, 2_000);
        assert_eq!(
            s.queue_wait_us_mean(),
            Some(40_000),
            "큐 대기가 handler 시간에 섞이면 안 된다"
        );
        assert_eq!(s.handler_us_mean(), Some(2_000));
    }

    // 1 마이크로초 미만은 0 으로 접히지만 **횟수는 오른다** — 그래야 "아주 빠른 호출이
    // 많았다" 가 "호출이 없었다" 로 보이지 않는다.
    #[test]
    fn a_sub_microsecond_call_still_counts() {
        let p = PressureStats::default();
        p.record_handler(Duration::from_nanos(10));
        let s = p.snapshot();
        assert_eq!(s.handler_calls, 1);
        assert_eq!(s.handler_us_sum, 0);
        assert_eq!(s.handler_us_mean(), Some(0), "0 이지 None 이 아니다");
    }

    #[test]
    fn an_untouched_plugin_wait_says_it_has_no_observation() {
        let s = PluginWaitStats::default().snapshot();
        assert_eq!(s.matched, 0);
        assert_eq!(s.us_mean(), None);
    }

    #[test]
    fn plugin_wait_keeps_the_largest_round_trip() {
        let p = PluginWaitStats::default();
        p.record(Duration::from_millis(3));
        p.record(Duration::from_millis(21));
        p.record(Duration::from_millis(6));
        let s = p.snapshot();
        assert_eq!(s.matched, 3);
        assert_eq!(s.us_sum, 30_000);
        assert_eq!(s.us_max, 21_000, "최댓값은 내려가지 않는다");
        assert_eq!(s.us_mean(), Some(10_000));
    }

    // host 큐/handler 축과 **섞이지 않는다** — 둘은 서로 다른 집계다.
    #[test]
    fn a_plugin_round_trip_is_not_host_handler_time() {
        let host = PressureStats::default();
        let plugin = PluginWaitStats::default();
        host.record_handler(Duration::from_millis(2));
        plugin.record(Duration::from_millis(900));
        assert_eq!(host.snapshot().handler_us_max, 2_000);
        assert_eq!(plugin.snapshot().us_max, 900_000);
    }

    /// accept 대기 상한은 자리 계수와 따로 쌓이고, 평균은 자기 기록 수로 나눈다.
    #[test]
    fn the_accept_wait_bound_has_its_own_count() {
        let c = ConnectionStats::default();
        assert_eq!(c.snapshot().accept_wait_bound_us_mean(), None);
        c.record_accept_wait(Duration::from_millis(100));
        c.record_accept_wait(Duration::from_millis(20));
        let s = c.snapshot();
        assert_eq!(
            (
                s.accept_waits,
                s.accept_wait_bound_us_sum,
                s.accept_wait_bound_us_max
            ),
            (2, 120_000, 100_000)
        );
        assert_eq!(s.accept_wait_bound_us_mean(), Some(60_000));
        assert_eq!((s.live, s.accepted), (0, 0), "기록은 자리를 잡지 않는다");
    }

    // 연결 게이지의 자리 계수는 **시간이 아니라 자리**를 센다 — live 는 내려가고 누계 셋은 안
    // 내려간다. 그 비대칭이 자리 계수의 전부다(accept 대기 상한은 위 시험이 따로 잰다).
    #[test]
    fn a_closed_connection_frees_the_seat_but_not_the_total() {
        let c = ConnectionStats::default();
        assert!(c.try_open(4).is_some());
        assert!(c.try_open(4).is_some());
        c.close();
        let s = c.snapshot();
        assert_eq!(s.live, 1, "하나 닫혔으니 자리는 하나 남았다");
        assert_eq!(s.live_max, 2, "최댓값은 내려가지 않는다");
        assert_eq!(s.accepted, 2, "누계는 닫아도 안 내려간다");
        assert_eq!(s.refused_saturated, 0);
    }

    // 거절은 **자리를 안 먹는다.** 되돌림이 빠지면 계수가 영구히 상한 위로 떠서
    // 그 뒤로 아무도 못 붙는다 — 그때도 live 는 상한 그대로라, 이 시험이 보는 것은
    // live 가 안 늘었다는 것과 거절이 자기 칸에 세졌다는 것 둘이다.
    #[test]
    fn a_refusal_counts_itself_without_taking_a_seat() {
        let c = ConnectionStats::default();
        let _held: Vec<_> = (0..2)
            .map(|_| c.try_open(2).expect("상한까지는 열린다"))
            .collect();
        assert!(c.try_open(2).is_none(), "상한을 넘으면 거절이다");
        assert!(c.try_open(2).is_none());
        let s = c.snapshot();
        assert_eq!(s.live, 2, "거절이 자리를 먹으면 안 된다");
        assert_eq!(s.accepted, 2, "거절은 accepted 에 안 센다");
        assert_eq!(s.refused_saturated, 2);
        assert_eq!(s.live_max, 2);
    }

    // 자리가 반납되면 다음이 들어온다 — 상한이 영구 차단이 아니다.
    #[test]
    fn a_returned_seat_lets_the_next_connection_in() {
        let c = ConnectionStats::default();
        assert_eq!(c.try_open(1), Some(1));
        assert!(c.try_open(1).is_none());
        c.close();
        assert_eq!(c.try_open(1), Some(1), "반납된 자리로 다음이 들어와야 한다");
        assert_eq!(c.snapshot().refused_saturated, 1);
    }

    // 자원 축과 시간 축은 **섞이지 않는다.** 연결이 꽉 차도 handler 시간은 0 일 수
    // 있고(요청이 아예 안 들어온 것이다), 그 구분이 이 게이지를 더한 이유다.
    #[test]
    fn a_full_connection_table_is_not_a_slow_handler() {
        let host = PressureStats::default();
        let conn = ConnectionStats::default();
        assert!(conn.try_open(1).is_some());
        assert!(conn.try_open(1).is_none());
        assert_eq!(
            host.snapshot().handler_calls,
            0,
            "요청은 하나도 안 들어왔다"
        );
        assert_eq!(conn.snapshot().refused_saturated, 1, "그래도 자리는 찼다");
    }

    // 버킷 경계는 `le` 다 — 상한과 **같은** 값은 그 칸에 든다. 이 경계가 배타적으로
    // 바뀌면 같은 관측이 한 칸 뒤로 밀리므로 여기서 죽는다.
    #[test]
    fn an_observation_equal_to_a_bound_lands_in_that_bucket() {
        let h = LatencyHistogram::default();
        h.record_us(10);
        h.record_us(11);
        let c = h.snapshot().counts;
        assert_eq!(c[0], 1, "10 µs 는 상한이 10 인 첫 칸이다");
        assert_eq!(c[1], 1, "11 µs 는 다음 칸이다");
    }

    // 마지막 상한을 넘은 것은 **넘침 칸**으로 가고, 그 칸에는 상한이 없다.
    #[test]
    fn everything_past_the_last_bound_lands_in_the_overflow_bucket() {
        let h = LatencyHistogram::default();
        h.record_us(LATENCY_BUCKET_BOUNDS_US[LATENCY_BUCKET_BOUNDS_US.len() - 1] + 1);
        h.record_us(u64::MAX);
        let s = h.snapshot();
        assert_eq!(s.counts[LATENCY_BUCKET_COUNT - 1], 2);
        assert_eq!(s.total(), 2);
        assert_eq!(
            HistogramSnapshot::bounds_us().len(),
            LATENCY_BUCKET_COUNT - 1,
            "상한 수보다 칸이 하나 많다 — 그 하나가 넘침이다"
        );
    }

    // 칸은 **누적이 아니다** — 겹치지 않고 합이 관측 수다. 누적(Prometheus `le`)으로
    // 바뀌면 합이 관측 수의 배가 되어 여기서 죽는다.
    #[test]
    fn the_buckets_do_not_overlap() {
        let h = LatencyHistogram::default();
        for us in [1, 50, 5_000, 500_000] {
            h.record_us(us);
        }
        let s = h.snapshot();
        assert_eq!(s.total(), 4, "합이 관측 수여야 한다");
        assert_eq!(s.counts.iter().filter(|&&n| n == 1).count(), 4);
    }

    // 평균과 최대가 같아도 분포는 다르다 — 그것이 histogram 을 더한 이유다.
    // 여기서 둘은 sum·max·count 가 전부 같고 칸만 다르다.
    #[test]
    fn two_runs_with_the_same_mean_have_different_shapes() {
        // 꼬리형: 한 건이 1 s, 나머지 셋이 0 — "대부분 빠른데 몇 건이 튄다".
        let tail = PressureStats::default();
        for us in [1_000_000, 0, 0, 0] {
            tail.record_handler(Duration::from_micros(us));
        }
        // 고른형: 넷이 전부 250 ms — "전부 조금씩 느리다".
        let spread = PressureStats::default();
        for _ in 0..4 {
            spread.record_handler(Duration::from_micros(250_000));
        }

        let t = tail.snapshot();
        let p = spread.snapshot();
        assert_eq!(t.handler_us_sum, p.handler_us_sum, "합이 같다");
        assert_eq!(t.handler_calls, p.handler_calls, "건수가 같다");
        assert_eq!(t.handler_us_mean(), p.handler_us_mean(), "평균이 같다");
        assert_ne!(
            t.handler_hist, p.handler_hist,
            "평균이 같아도 분포는 달라야 한다 — 그것이 이 값의 존재 이유다"
        );
        assert_eq!(t.handler_hist.counts[0], 3, "꼬리형은 셋이 맨 앞 칸이다");
        assert_eq!(p.handler_hist.counts[0], 0, "고른형은 맨 앞 칸이 비어 있다");
    }

    // 큐 대기와 handler 시간의 분포가 **섞이지 않는다.** 한 histogram 을 공유하면
    // 게이트 앞뒤가 다시 한 수로 합쳐진다.
    #[test]
    fn the_two_time_moduli_keep_separate_distributions() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_millis(40));
        p.record_handler(Duration::from_micros(2));
        let s = p.snapshot();
        assert_eq!(s.queue_wait_hist.total(), 1);
        assert_eq!(s.handler_hist.total(), 1);
        assert_eq!(s.handler_hist.counts[0], 1, "2 µs 는 맨 앞 칸");
        assert_eq!(
            s.queue_wait_hist.counts[0], 0,
            "40 ms 가 앞 칸에 오면 안 된다"
        );
        assert_ne!(s.queue_wait_hist, s.handler_hist);
    }

    // plugin 왕복도 자기 분포를 든다 — 호스트 축과 별개다.
    #[test]
    fn a_plugin_round_trip_has_its_own_distribution() {
        let host = PressureStats::default();
        let plugin = PluginWaitStats::default();
        host.record_handler(Duration::from_millis(2));
        plugin.record(Duration::from_millis(900));
        assert_eq!(plugin.snapshot().hist.total(), 1);
        assert_eq!(
            host.snapshot().handler_hist.total(),
            1,
            "호스트 칸에 plugin 왕복이 섞이면 안 된다"
        );
        assert_ne!(plugin.snapshot().hist, host.snapshot().handler_hist);
    }
}
