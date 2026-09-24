//! 테스트 전용 공용 유틸리티 — 프로세스 전역 상태(env · `TASTY_HOME`)를 테스트 동안만
//! 갈아끼우는 RAII 가드와, 플랫폼별로 형태가 다른 절대경로 조립.
//!
//! 소비자의 dev-dependency로만 사용하며 제품 바이너리에는 포함하지 않는다.

use std::sync::{Mutex, MutexGuard};

/// TASTY_HOME 변경 시험의 공통 락. TastyHomeGuard가 이전 값 보관·복원과 함께 관리한다.
/// 락만 얻고 복원을 빠뜨리는 호출을 막기 위해 크레이트 밖에는 노출하지 않는다.
static TASTY_HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

/// [`TASTY_HOME_ENV_LOCK`] 복구 보고의 대상 이름과 1 회 보고 플래그.
const LOCK_WHAT: &str = "TASTY_HOME_ENV_LOCK";
static LOCK_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// TASTY_HOME을 임시 디렉터리로 바꾸고 Drop에서 원값을 복원한다.
/// 원래 없던 값은 제거하며, 복원을 마친 뒤 직렬화 락을 놓는다.
pub struct TastyHomeGuard {
    // drop 순서 = 선언 순서다. env 복원 → 임시 디렉토리 삭제 → 락 해제 순으로 끝나야
    // 하므로 이 셋의 순서를 바꾸지 않는다(복원과 정리가 모두 락 보유 중에 끝난다).
    _env: EnvVarGuard,
    dir: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl TastyHomeGuard {
    /// 락 획득 → 이전 값 보관 → 새 임시 디렉토리로 `TASTY_HOME` 설정.
    pub fn new() -> Self {
        // 앞선 시험의 패닉으로 생긴 poison은 복구하고 처음 한 번 보고한다.
        let lock = tasty_utils::poison::recover_mutex(
            TASTY_HOME_ENV_LOCK.lock(),
            LOCK_WHAT,
            &LOCK_POISON_REPORTED,
        );
        let dir = tempfile::tempdir().expect("tempdir");
        let env = EnvVarGuard::set("TASTY_HOME", dir.path());
        Self {
            _env: env,
            dir,
            _lock: lock,
        }
    }

    /// 이 가드가 `TASTY_HOME` 으로 지정한 임시 디렉토리.
    pub fn path(&self) -> &std::path::Path {
        self.dir.path()
    }
}

/// 이 스레드의 tasty_home을 가드 전용 임시 디렉터리로 지정한다.
/// 환경변수보다 먼저 적용되는 스레드 로컬 override이므로 전역 락 없이 병렬로 쓸 수 있다.
/// 생성 뒤에도 파일을 쓰는 CoreState는 수명 전체에 걸쳐 가드를 유지해야 한다.
/// 같은 스레드의 여러 인스턴스도 디렉터리를 공유하지 않는다.
///
/// 자식 스레드에는 상속되지 않는다. 그 스레드가 tasty_home을 호출하면 실제 환경 경로를
/// 읽을 수 있다. 격리 검증 절차는 docs/dev-guide/unit-test-isolation.md를 따른다.
pub struct IsolatedHome {
    // drop 순서 = 선언 순서. override 를 먼저 내리고 그 다음에 디렉토리를 지운다 — 반대면
    // 지워진 경로를 가리키는 override 가 잠깐 살아 있다.
    _pop: PopHomeOverride,
    _dir: tempfile::TempDir,
}

/// `Drop` 에서 override 를 내리기만 하는 자리표시자. [`IsolatedHome`] 의 필드 순서로
/// "override 내림 → 디렉토리 삭제" 를 강제하려고 별도 타입으로 둔다.
struct PopHomeOverride;

impl Drop for PopHomeOverride {
    fn drop(&mut self) {
        tasty_utils::path::pop_home_override();
    }
}

impl IsolatedHome {
    /// 새 임시 디렉토리를 만들어 이 스레드의 `tasty_home()` 으로 세운다.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        tasty_utils::path::push_home_override(dir.path().to_path_buf());
        Self {
            _pop: PopHomeOverride,
            _dir: dir,
        }
    }
}

/// 플랫폼에 맞는 시험용 절대경로. tmp/exp는 Unix의 /tmp/exp, Windows의 C:\tmp\exp가 된다.
pub fn abs_path(rel: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::path::PathBuf::from(format!(r"C:\{}", rel.replace('/', r"\")))
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from(format!("/{rel}"))
    }
}

/// 환경변수를 바꾸고 Drop에서 원값을 복원한다. 직접 직렬화 락을 얻지는 않으므로
/// 호출자가 동시 환경변수 접근을 조율해야 한다. TASTY_HOME은 TastyHomeGuard를 사용한다.
pub struct EnvVarGuard {
    key: &'static str,
    prev: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    /// 현재 값을 보관하고 `value` 로 덮어쓴다.
    pub fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
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
