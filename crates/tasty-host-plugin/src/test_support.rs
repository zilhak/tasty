//! 테스트 전용 공용 유틸리티 — 홈 경로 env 격리.
//!
//! 규칙·근거: `docs/dev-guide/unit-test-isolation.md` ·
//! `docs/adr/0096-unit-tests-isolated-from-user-environment.md`.

use std::ffi::OsString;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// `HOME` 과 `TASTY_HOME` **양쪽**을 덮는 이 crate 단일 직렬화 락.
///
/// `tasty_utils::path::tasty_home()` 은 두 변수를 함께 본다 — `TASTY_HOME` 이 있으면
/// 그것을, 없으면 `$HOME/.tasty{-debug}` 를 쓴다. 그래서 키마다 락을 나누면 격리가
/// 깨진다: 한 테스트가 `TASTY_HOME` 을 임시 루트로 세워둔 사이 다른 테스트가 그것을
/// 지우거나 덮어쓰면, 앞 테스트의 `config.save()` 가 **사용자의 실제
/// `~/.tasty{-debug}`** 에 쓴다. 두 키 중 하나라도 건드리는 이 crate 의 모든 테스트는
/// 반드시 이 하나의 락으로 직렬화한다.
///
/// 직접 잡을 일은 없다 — [`HomeEnvGuard`] 가 획득과 복원을 함께 맡는다. 이 crate 안에서
/// 두 키를 만지는 유일한 지점이 이 모듈이라는 사실은
/// `tests::home_env_is_only_touched_through_this_module` 이 소스 스캔으로 고정한다.
static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

/// 홈 경로 env 를 임시 디렉토리로 갈아끼우는 RAII 가드.
///
/// 생성 시 [`HOME_ENV_LOCK`] 을 잡고 `HOME`·`TASTY_HOME` **둘 다** 의 이전 값을 보관한 뒤
/// 필요한 쪽을 바꾼다. `Drop` 은 두 값을 모두 원래대로 돌려놓고(원래 없었으면 제거) 그
/// 다음에 임시 디렉토리를 지우며, 락은 그 뒤에 풀린다 — 단언이 패닉해도 같은 순서로
/// 복원된다.
pub(crate) struct HomeEnvGuard {
    prev_home: Option<OsString>,
    prev_tasty_home: Option<OsString>,
    dir: tempfile::TempDir,
    // 복원·정리가 끝난 뒤에 풀려야 하므로 마지막에 선언한다 — 구조체 필드는 선언
    // 순서로 drop 되고, `Drop::drop` 본문은 그보다 먼저 실행된다.
    _lock: MutexGuard<'static, ()>,
}

impl HomeEnvGuard {
    fn acquire() -> Self {
        let lock = HOME_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        Self {
            prev_home: std::env::var_os("HOME"),
            prev_tasty_home: std::env::var_os("TASTY_HOME"),
            dir: tempfile::tempdir().expect("tempdir"),
            _lock: lock,
        }
    }

    /// `TASTY_HOME` 을 임시 디렉토리로 지정한다 — `tasty_home()` 이 곧 [`Self::path`].
    pub(crate) fn tasty_home() -> Self {
        let guard = Self::acquire();
        // SAFETY: HOME_ENV_LOCK 보유 중 — 이 crate 에서 홈 경로 env 를 건드리는 모든
        // 테스트와 직렬화되므로 동시 접근 데이터 레이스가 없다.
        unsafe { std::env::set_var("TASTY_HOME", guard.dir.path()) };
        guard
    }

    /// `HOME` 을 임시 디렉토리로 지정하고 `TASTY_HOME` 을 비운다 — `tasty_home()` 이
    /// `$HOME/.tasty{-debug}` 로 **파생**되게 한다.
    ///
    /// `TASTY_HOME` 을 비우지 않으면 우선순위 때문에 `HOME` 교체가 통째로 무시된다
    /// (실행 환경에 `TASTY_HOME` 이 잡혀 있는 경우가 실제로 흔하다).
    pub(crate) fn derived_from_home() -> Self {
        let guard = Self::acquire();
        // SAFETY: `tasty_home` 과 동일 — HOME_ENV_LOCK 보유 중.
        unsafe { std::env::set_var("HOME", guard.dir.path()) };
        // SAFETY: 상동.
        unsafe { std::env::remove_var("TASTY_HOME") };
        guard
    }

    /// 이 가드가 세운 임시 디렉토리.
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for HomeEnvGuard {
    fn drop(&mut self) {
        restore("HOME", self.prev_home.as_ref());
        restore("TASTY_HOME", self.prev_tasty_home.as_ref());
    }
}

fn restore(key: &str, prev: Option<&OsString>) {
    match prev {
        // SAFETY: 아직 HOME_ENV_LOCK 을 쥔 상태다(`_lock` 은 마지막 필드라 이 함수가
        // 끝난 뒤에 drop 된다).
        Some(v) => unsafe { std::env::set_var(key, v) },
        // SAFETY: 상동.
        None => unsafe { std::env::remove_var(key) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 두 진입점이 실제로 `tasty_home()` 을 임시 루트로 옮기고, 스코프를 벗어나면
    /// 실행 환경의 원값이 그대로 돌아오는지.
    #[test]
    fn guard_isolates_then_restores_both_keys() {
        let before_home = std::env::var_os("HOME");
        let before_tasty = std::env::var_os("TASTY_HOME");

        {
            let g = HomeEnvGuard::tasty_home();
            assert_eq!(
                tasty_utils::path::tasty_home().as_deref(),
                Some(g.path()),
                "tasty_home() 은 가드가 세운 임시 루트여야 한다"
            );
        }
        assert_eq!(std::env::var_os("HOME"), before_home);
        assert_eq!(std::env::var_os("TASTY_HOME"), before_tasty);

        {
            let g = HomeEnvGuard::derived_from_home();
            // 전 플랫폼 공통 계약: `HOME` 이 임시 루트를 가리키고 `TASTY_HOME` 은 비어야
            // 한다. 이 두 가지가 가드가 하는 일의 전부이며 OS 에 의존하지 않는다.
            assert_eq!(
                std::env::var_os("HOME").as_deref(),
                Some(g.path().as_os_str())
            );
            assert!(std::env::var_os("TASTY_HOME").is_none());

            // `tasty_home()` 이 그 `HOME` 에서 파생되는지는 **unix 한정**이다.
            // `BaseDirs` 는 unix 에서 `$HOME` 을 읽지만 Windows 에서는
            // `SHGetKnownFolderPath`(`directories::win::base_dirs`)로 실사용자 프로필을
            // 돌려주므로 env override 가 통하지 않는다 — 같은 이유로
            // `bundle_sig::integration_tests` 도 `#[cfg(all(test, unix))]` 다. Windows
            // 에서 이 단언을 그대로 돌리면 100% 실패한다.
            #[cfg(unix)]
            {
                let expected = g.path().join(if cfg!(debug_assertions) {
                    ".tasty-debug"
                } else {
                    ".tasty"
                });
                assert_eq!(
                    tasty_utils::path::tasty_home().as_deref(),
                    Some(expected.as_path()),
                    "TASTY_HOME 을 비웠으므로 HOME 파생 경로여야 한다"
                );
            }
        }
        assert_eq!(std::env::var_os("HOME"), before_home);
        assert_eq!(std::env::var_os("TASTY_HOME"), before_tasty);
    }

    /// 교차 간섭 회귀 가드 — `HOME` / `TASTY_HOME` 을 바꾸는 코드가 이 모듈 밖에 생기면
    /// 그쪽은 [`HOME_ENV_LOCK`] 을 잡지 않는 별개 경로가 되어, 동시에 도는 다른 테스트의
    /// 임시 루트를 지우거나 덮어쓴다(→ 사용자 실제 홈에 쓰게 된다). 그래서 이 crate 는
    /// **이 모듈 한 곳에서만** 두 키를 만진다는 것을 소스 스캔으로 고정한다.
    /// 스캔 하한 — 위반 0 이 **정말 없어서인지 아무것도 안 봐서인지**를 가른다.
    /// [`visit`] 는 디렉토리를 못 읽으면 `return` 으로 조용히 빠져나가므로, 하한이 없으면
    /// 스캔 루트가 틀린 날 이 가드가 아무 말 없이 통과한다. 값은 실측 37 에 대해 아래쪽
    /// 여유를 둔 것이다.
    const MIN_SCANNED_FILES: usize = 20;

    /// 스캔이 믿을 만한가. 단언 안에 인라인으로 두면 그 값이 무엇을 가르는지 시험할 자리가
    /// 없고, 하한 자신이 장식이 된다.
    fn scan_is_credible(found: usize) -> bool {
        found >= MIN_SCANNED_FILES
    }

    /// 하한을 겨냥한 변이 — 하한 자신이 판정을 하는지 본다.
    #[test]
    fn the_scan_refuses_to_report_zero_from_an_empty_walk() {
        assert!(!scan_is_credible(0), "빈 스캔을 믿을 만하다고 판정했다");
        assert!(!scan_is_credible(MIN_SCANNED_FILES - 1));
        assert!(scan_is_credible(MIN_SCANNED_FILES));
    }

    #[test]
    fn home_env_is_only_touched_through_this_module() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut scanned = 0usize;
        visit(&root, &mut offenders, &mut scanned);
        assert!(
            scan_is_credible(scanned),
            "스캔한 `.rs` 가 {scanned}개다(하한 {MIN_SCANNED_FILES}) — 순회가 깨졌다. \
             위반 0 은 이 상태에서 아무 뜻도 없다"
        );
        assert!(
            offenders.is_empty(),
            "HOME/TASTY_HOME env 변경은 test_support::HomeEnvGuard 를 통해서만 한다 \
             (별도 락 경로가 생기면 테스트끼리 서로의 임시 홈을 지운다). 위반: {offenders:#?}"
        );
    }

    fn visit(dir: &Path, offenders: &mut Vec<String>, scanned: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, offenders, scanned);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|n| n == "test_support.rs")
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            *scanned += 1;
            for (i, line) in text.lines().enumerate() {
                let touches_env = line.contains("env::set_var") || line.contains("env::remove_var");
                if touches_env && (line.contains("\"HOME\"") || line.contains("\"TASTY_HOME\"")) {
                    offenders.push(format!("{}:{}", path.display(), i + 1));
                }
            }
        }
    }
}
