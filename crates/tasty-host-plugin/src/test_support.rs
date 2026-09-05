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

/// 홈 경로를 임시 디렉토리로 갈아끼우는 RAII 가드. 두 진입점이 **다른 메커니즘**을 쓴다
/// — 그 자리에서 홈이 도구인가 피험자인가에 따라(처방 등급·근거는
/// `docs/adr/0155-global-state-race-prescription-by-parameterization.md`):
///
/// - [`Self::tasty_home`] — 홈이 **도구**(임시 격리 루트일 뿐, 검증 대상은 다른 것)인
///   다수. `tasty_utils::path` 의 스레드 로컬 override 로 격리하고 **env 를 만지지
///   않는다**. 그래서 서로, 그리고 다른 완주와 경합하지 않는다(락 불필요).
/// - [`Self::derived_from_home`] — `HOME`→`.tasty{-debug}` **파생 규칙 자체가 검증
///   대상**(홈이 피험자)이라 실제 `HOME` 을 set 해야 한다. [`HOME_ENV_LOCK`] 을 본문
///   동안 유지하고 `Drop` 에서 복원한다. `set_var` 가 남는 유일한 갈래다.
pub(crate) struct HomeEnvGuard {
    dir: tempfile::TempDir,
    mode: GuardMode,
}

enum GuardMode {
    /// override 격리(env 미접촉). `Drop` 은 이 스레드의 override 만 pop 한다.
    Override,
    /// 실제 env 를 바꾼 갈래. `Drop` 은 두 키를 복원한다. `_lock` 은 마지막 필드라
    /// 복원(및 임시 디렉토리 삭제)이 끝난 뒤에 풀린다.
    Env {
        prev_home: Option<OsString>,
        prev_tasty_home: Option<OsString>,
        _lock: MutexGuard<'static, ()>,
    },
}

impl HomeEnvGuard {
    /// `tasty_home()` 을 임시 루트로 고정한다 — [`Self::path`] 와 같은 경로. env 를 만지지
    /// 않는 스레드 로컬 override 를 쓴다.
    pub(crate) fn tasty_home() -> Self {
        // `tempdir()` 는 내부적으로 `TMPDIR` env 를 읽는다. `derived_from_home` 의
        // `set_var` 와 environ 배열을 두고 경합하지 않도록 **생성 동안만** 락을 잡는다 —
        // 격리 자체는 env 를 안 만지는 override 라 본문엔 락이 필요 없다.
        let dir = {
            let _lock = HOME_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            tempfile::tempdir().expect("tempdir")
        };
        tasty_utils::path::push_home_override(dir.path().to_path_buf());
        Self {
            dir,
            mode: GuardMode::Override,
        }
    }

    /// `HOME` 을 임시 디렉토리로 지정하고 `TASTY_HOME` 을 비운다 — `tasty_home()` 이
    /// `$HOME/.tasty{-debug}` 로 **파생**되게 한다. 이 파생 규칙 자체가 검증 대상이라
    /// override 로 대체할 수 없다(그러면 검증하려던 것이 사라진다). `TASTY_HOME` 을
    /// 비우지 않으면 우선순위 때문에 `HOME` 교체가 통째로 무시된다.
    pub(crate) fn derived_from_home() -> Self {
        let lock = HOME_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_home = std::env::var_os("HOME");
        let prev_tasty_home = std::env::var_os("TASTY_HOME");
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: HOME_ENV_LOCK 보유 중 — 이 crate 에서 홈 경로 env 를 set/remove 하는
        // 모든 자리(HomeEnvGuard)와 직렬화되므로 동시 접근 데이터 레이스가 없다.
        unsafe { std::env::set_var("HOME", dir.path()) };
        // SAFETY: 상동.
        unsafe { std::env::remove_var("TASTY_HOME") };
        Self {
            dir,
            mode: GuardMode::Env {
                prev_home,
                prev_tasty_home,
                _lock: lock,
            },
        }
    }

    /// 이 가드가 세운 임시 디렉토리.
    pub(crate) fn path(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for HomeEnvGuard {
    fn drop(&mut self) {
        match &self.mode {
            GuardMode::Override => tasty_utils::path::pop_home_override(),
            GuardMode::Env {
                prev_home,
                prev_tasty_home,
                ..
            } => {
                restore("HOME", prev_home.as_ref());
                restore("TASTY_HOME", prev_tasty_home.as_ref());
            }
        }
    }
}

fn restore(key: &str, prev: Option<&OsString>) {
    match prev {
        // SAFETY: 아직 HOME_ENV_LOCK 을 쥔 상태다(`_lock` 은 `GuardMode::Env` 의 필드라
        // 이 함수가 끝난 뒤에 drop 된다).
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
            //
            // ★ 이 아래 단언은 이 crate 에서 `set_var`(HOME) 가 남는 유일한 이유다. 검증
            // 대상이 host-plugin 이 아니라 `crates/tasty-utils/src/path.rs` 의
            // `tasty_home()` 폴백 규칙("`TASTY_HOME` 이 비면 `$HOME/.tasty{-debug}` 로
            // 파생")이라, 그 규칙 자체가 피험자다 — override 로 대체하면 검증하려던 것이
            // 사라진다. **그래서 이 한 자리는 override 로 못 옮긴다.** 다만 규칙의 SoT 가
            // tasty-utils 이므로, tasty-utils 가 자체 테스트 격리를 갖추면 이 단언은 그리로
            // 옮겨야 한다(여기 있는 것은 임시). 근거·재검토 트리거는
            // `docs/adr/0155-global-state-race-prescription-by-parameterization.md`.
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
    /// 스캔 하한 — ADR-0133 의 두 용도 중 **연기 검사**다("경로가 틀렸거나 읽기에
    /// 실패했다" 를 잡는 용도). **모수 고정**("이만큼 봤으니 다 봤다")으로 쓰지 않는다.
    ///
    /// 위반 0 이 **정말 없어서인지 아무것도 안 봐서인지**를 가른다. [`visit`] 는 디렉토리를
    /// 못 읽으면 `return` 으로 조용히 빠져나가므로, 하한이 없으면 스캔 루트가 틀린 날 이
    /// 가드가 아무 말 없이 통과한다.
    ///
    /// 값의 근거: 2026-09-05 기준 **[`visit`] 가 실제로 읽은 `.rs` 수 37** 이다
    /// (`crates/tasty-host-plugin/src` 하위, `test_support.rs` 자신 제외).
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

    /// (C) thread-local override 처방의 **전제 가드**. `tasty_utils::path` 의 스레드 로컬
    /// override 는 자식 스레드에 상속되지 않으므로, 프로덕션이 `spawn` 한 스레드 본문에서
    /// `tasty_home()` 을 읽으면 override 를 못 보고 실제 홈/env 로 폴백한다. 그런 자리가
    /// 하나도 없다("구멍 0")는 측정 위에서 이 처방을 골랐다 — 이 가드가 그 전제를 지킨다.
    ///
    /// 위반이 생기면 실패 메시지는 "고치지 마라" 가 아니라 **ADR-0155 를 다시 열라**고
    /// 말한다: 자식 스레드가 홈을 읽기 시작하면 처방 선택(왜 (C) 인가) 자체가 재검토
    /// 대상이기 때문이다(705 식 전제-가드).
    ///
    /// 근사다(ADR-0133 의 연기 검사): `spawn(` 뒤 클로저 블록 안의 `tasty_home` **직접**
    /// 호출만 잡는다. 경유 함수(discovery/known_plugins 등)를 통한 **간접** 호출은 못
    /// 잡으며, 그 갈래는 ADR-0155 에 문장 트리거로 남긴다.
    #[test]
    fn spawned_thread_bodies_do_not_read_tasty_home() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut scanned = 0usize;
        visit_spawn(&root, &mut offenders, &mut scanned);
        assert!(
            scan_is_credible(scanned),
            "스캔한 `.rs` 가 {scanned}개다(하한 {MIN_SCANNED_FILES}) — 순회가 깨졌다. \
             위반 0 은 이 상태에서 아무 뜻도 없다"
        );
        assert!(
            offenders.is_empty(),
            "자식 스레드 본문에서 tasty_home() 을 직접 읽는 자리가 생겼다. thread-local \
             override 는 자식 스레드에 상속되지 않아 ADR-0155 의 (C) 전제(구멍 0)가 깨진다 \
             — 처방 선택을 다시 열어라(docs/adr/0155-global-state-race-prescription-by-parameterization.md). \
             위반: {offenders:#?}"
        );
    }

    fn visit_spawn(dir: &Path, offenders: &mut Vec<String>, scanned: &mut usize) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit_spawn(&path, offenders, scanned);
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
            for line in spawn_bodies_reading_home(&text) {
                offenders.push(format!("{}:{line}", path.display()));
            }
        }
    }

    /// `spawn(` 뒤 클로저 블록(첫 `{` 부터 균형 잡힌 `}` 까지) 안에서 `tasty_home` 를 찾아
    /// 그 `spawn` 이 있는 1-기반 라인 번호를 돌려준다. `respawn`/`spawner` 등은 단어 경계로
    /// 거른다. 완벽한 Rust 파서가 아니라 연기 검사용 근사다.
    fn spawn_bodies_reading_home(text: &str) -> Vec<usize> {
        let bytes = text.as_bytes();
        let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
        let mut hits = Vec::new();
        let mut i = 0;
        while let Some(rel) = text[i..].find("spawn") {
            let start = i + rel;
            i = start + "spawn".len();
            // 단어 경계 — respawn/spawner 등 다른 식별자면 건너뛴다.
            if start > 0 && is_ident(bytes[start - 1]) {
                continue;
            }
            // `spawn` 뒤 첫 `{` 가 클로저 본문 시작. 그 전에 `;` 가 오면 클로저 블록이
            // 아니다(예: `let h = spawner();`).
            let Some(brace_rel) = text[i..].find('{') else {
                break;
            };
            let brace = i + brace_rel;
            if text[i..brace].contains(';') {
                continue;
            }
            // 균형 매칭으로 닫는 `}` 를 찾는다.
            let mut depth = 0i32;
            let mut end = None;
            for (off, &b) in bytes[brace..].iter().enumerate() {
                match b {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(brace + off);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else {
                break;
            };
            if text[brace..=end].contains("tasty_home") {
                let line = text[..start].bytes().filter(|&b| b == b'\n').count() + 1;
                hits.push(line);
            }
            i = end + 1;
        }
        hits
    }
}
