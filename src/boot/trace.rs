//! 부팅 구간 계측 (T1~T7) 의 크로스-모듈 상태.
//!
//! 계측 로그는 전부 `target: "tasty::boot"` 로 찍힌다 — debug 빌드의 file layer
//! (`debug-dev.log`, debug 레벨) 에 수집되고, stderr 기본 필터가 warn 이라 평소
//! 콘솔에는 나오지 않는다. release 검증 시엔 `TASTY_LOG=info` 로 실행.
//!
//! T1~T6 은 각 호출 지점에서 지역 `Instant` 로 직접 재므로 이 모듈은 유일하게
//! 함수 경계를 넘는 T7(= `resumed()` 반환 → 첫 present)만 담당한다.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// T7 시작점 — 첫 부팅 `resumed()` 가 정상 완료된 시각.
///
/// shell setup 모드로 빠지는 early-return 경로에서는 set 되지 않는다. 그 경우
/// T7 은 (무의미한 값을 찍는 대신) 조용히 생략된다 — setup 완료 후의 첫 paint
/// 는 "부팅 흰 화면" 측정 대상이 아니기 때문.
static RESUMED_DONE: OnceLock<Instant> = OnceLock::new();

static FIRST_PAINT_LOGGED: AtomicBool = AtomicBool::new(false);

/// `resumed()` 정상 완료 직전에 호출 — T7 의 기준 시각을 기록한다.
pub(crate) fn mark_resumed_done() {
    // 두 번째 이후 호출(이론상 없음)은 no-op — 첫 시각 유지가 의도.
    RESUMED_DONE.set(Instant::now()).ok();
}

/// 첫 `RedrawRequested` 처리에서 `gpu.render` 가 `Ok(())` 를 반환했을 때 호출.
///
/// Lost/Outdated 재시도 프레임은 present 가 안 됐으므로 호출하지 않는다 —
/// caller(redraw.rs) 가 `Ok` 분기에서만 부른다. 원샷: 두 번째 이후 호출은 no-op.
pub(crate) fn mark_first_paint() {
    if FIRST_PAINT_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(t0) = RESUMED_DONE.get() {
        tracing::info!(
            target: "tasty::boot",
            ms = t0.elapsed().as_secs_f64() * 1000.0,
            "T7 first_paint (resumed done -> first present)"
        );
    }
}
