//! 테스트 전용 공용 유틸리티.

use std::sync::{Mutex, MutexGuard};

/// `TASTY_HOME` 환경변수를 건드리는 테스트들이 공유하는 직렬화 락.
///
/// `std::env::set_var`/`remove_var` 는 프로세스 전역 상태를 변경하고, Rust 기본
/// 테스트 러너는 여러 테스트를 동시에 실행한다. 이 crate 안에서 `TASTY_HOME` 을
/// 변경하는 모든 테스트(`platform::screen_capture`, `webhook::config`)는 이 락을
/// 먼저 획득해야 서로 간섭 없이 안전하다.
///
/// 직접 잡을 일은 없다 — [`TastyHomeGuard`] 가 락 획득과 원값 복원을 함께 맡는다.
///
/// ★ **그 문장을 컴파일러가 지킨다.** 이 static 은 모듈 비공개다(`pub(crate)` 가 아니다) —
/// 밖에서 이름을 부르면 컴파일 오류다. 종전에는 소스 스캔 하나가 그 일을 대신했는데,
/// 텍스트로 재는 것보다 **못 쓰게 만드는 것**이 싸고 확실하다. 넓히려면 먼저 물어라:
/// 락만 손에 넣고 값을 직접 바꾸는 경로가 생기면 [`TastyHomeGuard`] 가 묶은 셋(락 ·
/// 이전 값 보관 · `Drop` 복원) 중 뒤 둘이 빠지고, 그 오염은 단독 실행에서 재현되지 않는다.
static TASTY_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

/// 테스트가 `TASTY_HOME` 을 임시 디렉토리로 갈아끼우는 동안 쓰는 RAII 가드.
///
/// 생성 시 [`TASTY_HOME_ENV_LOCK`] 을 잡고 **이전 값을 기억한 뒤** 임시 디렉토리를
/// 가리키게 하며, `Drop` 에서 그 값을 그대로 되돌린다(원래 없었으면 제거). 락은
/// 복원이 끝난 뒤 풀린다.
///
/// 수동 `set_var` / `remove_var` 쌍으로 대신하면 두 가지가 샌다 — 단언 실패로
/// 패닉하면 복원 줄에 도달하지 못하고, `remove_var` 로 끝내면 원래 `TASTY_HOME`
/// 이 설정돼 있던 환경에서 그 값을 잃는다. 어느 쪽이든 같은 프로세스의 뒤따르는
/// 테스트가 오염된 환경을 물려받아, 변경과 무관한 실패가 생긴다.
pub(crate) struct TastyHomeGuard {
    // drop 순서 = 선언 순서다. env 복원 → 임시 디렉토리 삭제 → 락 해제 순으로 끝나야
    // 하므로 이 셋의 순서를 바꾸지 않는다(복원과 정리가 모두 락 보유 중에 끝난다).
    _env: EnvVarGuard,
    dir: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl TastyHomeGuard {
    /// 락 획득 → 이전 값 보관 → 새 임시 디렉토리로 `TASTY_HOME` 설정.
    pub(crate) fn new() -> Self {
        let lock = TASTY_HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let env = EnvVarGuard::set("TASTY_HOME", dir.path());
        Self {
            _env: env,
            dir,
            _lock: lock,
        }
    }

    /// 이 가드가 `TASTY_HOME` 으로 지정한 임시 디렉토리.
    pub(crate) fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

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

/// 임의의 환경변수 하나를 테스트 동안만 바꿔두는 RAII 가드.
///
/// [`TastyHomeGuard`] 와 같은 이유로 존재한다 — env 는 프로세스 전역이라, 테스트가
/// `set_var` 로 바꾼 뒤 `remove_var` 로 "정리" 하면 원래 값이 있던 환경에서 그 값을
/// 잃고, 단언 실패로 패닉하면 정리 자체가 건너뛰어진다. 어느 쪽이든 같은 프로세스의
/// 뒤따르는 테스트가 오염된 환경을 물려받는다.
///
/// 직렬화 락은 이 가드가 잡지 않는다 — 어떤 키를 어느 테스트끼리 직렬화할지는
/// 호출부의 책임이다. `TASTY_HOME` 은 [`TastyHomeGuard`] 가 전용 락과 함께 이 가드를
/// 감싸 쓴다(그래서 이 타입은 플랫폼과 무관하게 항상 사용된다 — OS 별로 미사용이 되어
/// `dead_code` 경고를 내지 않는다).
pub(crate) struct EnvVarGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    /// 현재 값을 보관하고 `value` 로 덮어쓴다.
    pub(crate) fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let guard = Self {
            key,
            prev: std::env::var_os(key),
        };
        // SAFETY: 단위 테스트 한정. 같은 키를 만지는 테스트끼리의 직렬화는 호출부가
        // 보장한다(이 가드를 쓰는 곳은 전용 락 또는 단일 스레드 실행 조건 아래 있다).
        unsafe { std::env::set_var(key, value) };
        guard
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.prev {
            // SAFETY: `set` 과 동일 조건 — 전용 락으로 직렬화된 단위 테스트 한정.
            Some(v) => unsafe { std::env::set_var(self.key, v) },
            // SAFETY: 상동.
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}
