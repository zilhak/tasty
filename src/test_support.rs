//! 테스트 전용 공용 유틸리티.

use std::sync::Mutex;

/// `TASTY_HOME` 환경변수를 건드리는 테스트들이 공유하는 직렬화 락.
///
/// `std::env::set_var`/`remove_var` 는 프로세스 전역 상태를 변경하고, Rust 기본
/// 테스트 러너는 여러 테스트를 동시에 실행한다. 이 crate 안에서 `TASTY_HOME` 을
/// 변경하는 모든 테스트(`platform::screen_capture`, `webhook::config`)는 이 락을
/// 먼저 획득해야 서로 간섭 없이 안전하다.
pub(crate) static TASTY_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

/// 테스트용 절대경로 조립 — 절대경로 형태가 플랫폼마다 다르다(`/tmp/x` vs `C:\tmp\x`).
///
/// explorer root 처럼 `Path::is_absolute()` 로 채택 여부를 판정하는 코드의 테스트에서
/// 유닉스 리터럴을 그대로 쓰면 Windows 에서 상대경로로 판정돼 기대값이 뒤집힌다
/// (`tasty_model::resolve_root` 가 폴백을 태운다).
///
/// `abs_path("tmp/exp")` → `/tmp/exp`(unix) · `C:\tmp\exp`(Windows).
pub(crate) fn abs_path(rel: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(format!(r"C:\{}", rel.replace('/', r"\")))
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from(format!("/{rel}"))
    }
}
