//! 테스트 전용 공용 유틸리티.

use std::sync::Mutex;

/// `TASTY_HOME` 환경변수를 건드리는 테스트들이 공유하는 직렬화 락.
///
/// `std::env::set_var`/`remove_var` 는 프로세스 전역 상태를 변경하고, Rust 기본
/// 테스트 러너는 여러 테스트를 동시에 실행한다. 이 crate 안에서 `TASTY_HOME` 을
/// 변경하는 모든 테스트(`platform::screen_capture`, `webhook::config`)는 이 락을
/// 먼저 획득해야 서로 간섭 없이 안전하다.
pub(crate) static TASTY_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());
