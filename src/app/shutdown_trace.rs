//! 종료 구간 계측 (S1~S5, target: `tasty::shutdown`) 의 크로스-모듈 상태.
//!
//! 부팅 계측(`src/boot/trace.rs`, target `tasty::boot`)과 대칭이다. 로그는 debug
//! 빌드의 file layer(`$TASTY_HOME/debug-dev.log`, debug 레벨)에 수집되고, stderr
//! 기본 필터가 warn 이라 평소 콘솔에는 나오지 않는다. release 검증은
//! `TASTY_LOG=info` 로 실행한다.
//!
//! 단계별 소요(S1/S3/S4 등)는 각 지점의 지역 `Instant` 로 직접 재므로 이 모듈은
//! **함수 경계를 넘는 t0 하나**만 담당한다. t0 을 전역으로 둬야 하는 이유는 둘:
//!
//! - 종료 진입점이 3곳(`AppEvent::Shutdown` / quit modal 즉시 종료 /
//!   `close_behavior=="quit"`)이라 경로마다 지역 t0 을 잡으면 `shutdown_total` 의
//!   기준이 어긋난다.
//! - 사용자 체감 종료 시간의 tail 은 `event_loop.exit()` *이후* 의 `App` drop
//!   구간(`src/boot.rs`)에 있어, t0 이 event loop 밖까지 살아남아야 한다.

use std::sync::OnceLock;
use std::time::Instant;

/// 종료 시퀀스 진입 시각. 최초 1회만 확정된다.
static SHUTDOWN_T0: OnceLock<Instant> = OnceLock::new();

/// 종료 시퀀스 진입점에서 호출 — t0 을 확정하고 돌려준다.
///
/// 두 번째 이후 호출은 최초 시각을 그대로 반환한다(원샷). quit modal 이 열렸다
/// 종료로 이어지는 경로에서도 t0 이 뒤로 밀리지 않는다.
pub(crate) fn mark_start() -> Instant {
    *SHUTDOWN_T0.get_or_init(Instant::now)
}

/// 이미 확정된 t0. `None` 이면 종료 시퀀스를 타지 않고 event loop 가 끝난 경우
/// (예: 마지막 창이 OS 쪽에서 사라져 winit 이 자체 종료) — 그때는
/// `shutdown_total_with_drop` 의 기준이 없으므로 발화하지 않는다.
pub(crate) fn started_at() -> Option<Instant> {
    SHUTDOWN_T0.get().copied()
}

/// 계측 로그의 `ms` 필드 값 — 부팅 계측과 같은 표기(f64 밀리초).
pub(crate) fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// `Duration` → `ms` 필드 값. 누적 델타(S5b/S5c)용.
pub(crate) fn duration_ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
