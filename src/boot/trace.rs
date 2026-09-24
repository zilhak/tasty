//! 부팅 Ready부터 첫 render 성공까지의 T7 시간을 한 번 기록한다.
//! T1~T6은 각 호출부에서 따로 측정한다.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// finish_boot가 창 등록을 마친 시각. 셸 설정 화면을 거쳤으면 설정 완료 뒤의 시각이다.
static RESUMED_DONE: OnceLock<Instant> = OnceLock::new();

static FIRST_PAINT_LOGGED: AtomicBool = AtomicBool::new(false);

/// 처음 부팅 완료한 시각을 유지한다.
pub(crate) fn mark_resumed_done() {
    RESUMED_DONE.set(Instant::now()).ok();
}

/// 호출자는 render 성공 때만 호출한다. 한 번 기록한 뒤에는 다시 기록하지 않는다.
pub(crate) fn mark_first_paint() {
    if FIRST_PAINT_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(t0) = RESUMED_DONE.get() {
        tracing::info!(
            target: "tasty::boot",
            ms = t0.elapsed().as_secs_f64() * 1000.0,
            "T7 first_paint (boot Ready -> first render success)"
        );
    }
}
