//! 여러 종료 진입점과 event_loop.exit 이후 App Drop이 같은 시작 시각을 사용한다.
//! 단계별 시간은 각 호출부가 따로 측정한다.

use std::sync::OnceLock;
use std::time::Instant;

static SHUTDOWN_T0: OnceLock<Instant> = OnceLock::new();

/// 처음 호출한 시각을 고정한다. 이후 호출은 같은 시각을 반환한다.
pub(crate) fn mark_start() -> Instant {
    *SHUTDOWN_T0.get_or_init(Instant::now)
}

/// 시작 기록이 없으면 None이며 전체 종료 시간을 계산할 기준도 없다.
pub(crate) fn started_at() -> Option<Instant> {
    SHUTDOWN_T0.get().copied()
}

pub(crate) fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

pub(crate) fn duration_ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
