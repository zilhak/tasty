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

/// T7 시작점 — 부팅 상태 머신이 Ready 에 도달(`finish_boot` = MainView 등록 완료)
/// 한 시각. 상태 머신 도입 전에는 `resumed()` 정상 완료 시각이었고, 두 시점은
/// "부팅 작업 완료 → 첫 실 UI paint 대기" 라는 같은 의미다 (이름은 호환 유지).
///
/// shell setup 모드로 빠지는 early-return 경로에서는 setup 확정 후의 finish_boot
/// 가 set 한다 — 그 T7 은 "부팅 흰 화면" 이 아니라 setup 완료 후 첫 paint 지연을
/// 뜻하므로 비교표에서는 구분해 읽는다.
static RESUMED_DONE: OnceLock<Instant> = OnceLock::new();

static FIRST_PAINT_LOGGED: AtomicBool = AtomicBool::new(false);

/// 부팅 완료(Ready, `finish_boot` 말미)에 호출 — T7 의 기준 시각을 기록한다.
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
