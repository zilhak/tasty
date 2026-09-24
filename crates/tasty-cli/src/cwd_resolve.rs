//! path 인자 정규화 + 검증. 호스트는 absolute + valid 만 받는다는 contract
//! (`path_kind = "directory"` / `"file"` 공통, `tasty-plugin-manifest::types::CliArg`).
//!
//! `--cwd` / `--directory`(`path_kind = "directory"`) 같이 디렉토리를 가리키는
//! 인자와 `--profile-file`(`path_kind = "file"`) 같이 파일을 가리키는 인자 모두
//! CLI process 의 cwd 기준으로 absolute path 로 변환된다 — 호스트(GUI)의 cwd 와
//! 무관하게, 사용자가 명령을 친 디렉토리 기준으로 해석된다.
//!
//! 검증 실패는 `bail!` — 호출자가 stderr 출력 + exit 1.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// raw 값을 호출자 cwd 기준 absolute path 로 정규화(symlink 해석 포함). 존재 여부·
/// 종류(dir/file) 검증은 호출자가 이어서 한다 — `normalize_cwd_arg`/`normalize_file_arg`
/// 공통 1 단계.
///
/// `not_accessible_key` 는 실패 문구의 번역 키다 — 대상 종류(cwd/파일)를 부르는 말이
/// 언어마다 달라, 영어 라벨을 인자로 끼워 넣으면 번역문 안에 영어가 남는다.
fn canonicalize_from_cwd(raw: &str, not_accessible_key: &str) -> Result<PathBuf> {
    let p = Path::new(raw);
    let base: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .with_context(|| tasty_i18n::t("cli.cwd_resolve.current_dir_failed").to_string())?
            .join(p)
    };
    base.canonicalize()
        .with_context(|| tasty_i18n::t_fmt(not_accessible_key, raw))
}

/// canonicalize 는 Windows 에서 `\\?\` verbatim 경로를 돌려준다. 정규화된 경로는
/// PTY working_dir / 외부 프로세스 인자로 전달되는데, 일부 도구가 `\\?\` 를 file URL
/// 로 변환하지 못해 깨진다. 외부 전달 전에 일반 경로로 되돌린다(비-Windows 는 no-op
/// — `tasty_utils::path::strip_verbatim_prefix` 참조).
fn strip_verbatim(p: &Path) -> String {
    tasty_utils::path::strip_verbatim_prefix(&p.to_string_lossy())
}

/// raw 값을 호출자 cwd 기준 absolute path 로 정규화 + 디렉토리 존재 검증.
///
/// - 절대경로 → `canonicalize()` (symlink 해석) 후 dir 검증.
/// - 상대경로 → `env::current_dir()?.join(raw).canonicalize()` 후 dir 검증.
/// - 비존재 / file / 권한 부족 → `Err`.
pub fn normalize_cwd_arg(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("{}", tasty_i18n::t("cli.cwd_resolve.cwd_empty"));
    }
    let canon = canonicalize_from_cwd(raw, "cli.cwd_resolve.cwd_not_accessible")?;
    let meta = canon.metadata().with_context(|| {
        tasty_i18n::t_fmt(
            "cli.cwd_resolve.cwd_metadata_failed",
            &canon.display().to_string(),
        )
    })?;
    if !meta.is_dir() {
        bail!(
            "{}",
            tasty_i18n::t_fmt(
                "cli.cwd_resolve.cwd_not_directory",
                &canon.display().to_string()
            )
        );
    }
    Ok(strip_verbatim(&canon))
}

/// raw 값을 호출자 cwd 기준 absolute path 로 정규화 + **파일** 존재 검증
/// (`path_kind = "file"`, `normalize_cwd_arg` 의 파일 버전 — 디렉토리면 거부).
/// 파일 *내용* 검증(JSON 파싱 등)은 하지 않는다 — 그건 그 내용을 아는 plugin 핸들러
/// 몫이다(예: claude 세션 프로필의 JSON 유효성은 `tasty-plugin-claude` 가 검증).
pub fn normalize_file_arg(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("{}", tasty_i18n::t("cli.cwd_resolve.path_empty"));
    }
    let canon = canonicalize_from_cwd(raw, "cli.cwd_resolve.file_not_accessible")?;
    let meta = canon.metadata().with_context(|| {
        tasty_i18n::t_fmt(
            "cli.cwd_resolve.file_metadata_failed",
            &canon.display().to_string(),
        )
    })?;
    if !meta.is_file() {
        bail!(
            "{}",
            tasty_i18n::t_fmt(
                "cli.cwd_resolve.file_not_regular",
                &canon.display().to_string()
            )
        );
    }
    Ok(strip_verbatim(&canon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// cwd를 바꾸는 이 테스트 바이너리의 모든 시험은 CwdGuard로 직렬화한다.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    /// 이전 cwd를 저장하고 패닉 시에도 복원한다. 락만으로는 복원을 보장하지 않는다.
    /// Drop 본문이 필드보다 먼저 실행되므로 복원은 CWD_LOCK을 잡은 상태에서 이루어진다.
    struct CwdGuard {
        prev: std::path::PathBuf,
        /// 가드 수명 동안 `CWD_LOCK` 을 쥔다. **복원이 락 안에서 나는 근거는 필드
        /// 순서가 아니다** — `Drop::drop` 이 필드보다 먼저 돌기 때문에 이 핸들이
        /// 어느 자리에 있든 [`Drop::drop`] 본문 뒤에 풀린다(아래에서 실측).
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl CwdGuard {
        fn enter(dir: &Path) -> Self {
            // 락을 먼저 잡는다 — 아래 `current_dir()` 읽기까지 직렬화 범위에 넣는다.
            let lock = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let prev = std::env::current_dir().expect("cwd");
            std::env::set_current_dir(dir).expect("set_current_dir");
            Self { prev, _lock: lock }
        }
    }

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            // 이유: CWD_LOCK은 Drop 본문 뒤에 해제된다.
            // 복원 실패를 알리되 이미 unwind 중이면 두 번째 패닉으로 abort하지 않는다.
            if std::env::set_current_dir(&self.prev).is_err() && !std::thread::panicking() {
                panic!("cwd 를 {} 로 되돌리지 못했다", self.prev.display());
            }
        }
    }

    #[test]
    fn absolute_existing_directory_is_returned_canonicalized() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = normalize_cwd_arg(tmp.path().to_str().unwrap()).expect("ok");
        // canonicalize 는 symlink 해석 → 정확 비교 대신 dir 존재 + dir 인지만 확인.
        assert!(Path::new(&out).is_absolute());
        assert!(Path::new(&out).is_dir());
    }

    #[test]
    fn relative_path_resolves_against_process_cwd() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let _cwd = CwdGuard::enter(tmp.path());
        let out = normalize_cwd_arg("sub").expect("ok");
        assert!(out.ends_with("sub"), "got {out}");
        assert!(Path::new(&out).is_absolute());
    }

    #[test]
    fn nonexistent_path_errors() {
        let err = normalize_cwd_arg("/this/path/should/not/exist/tasty-test");
        assert!(err.is_err());
    }

    #[test]
    fn file_path_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let err = normalize_cwd_arg(file.to_str().unwrap());
        assert!(err.is_err(), "file path should be rejected");
    }

    #[test]
    fn empty_errors() {
        assert!(normalize_cwd_arg("").is_err());
    }

    #[test]
    fn file_arg_absolute_existing_file_is_returned_canonicalized() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("profile.json");
        std::fs::write(&file, "{}").unwrap();
        let out = normalize_file_arg(file.to_str().unwrap()).expect("ok");
        assert!(Path::new(&out).is_absolute());
        assert!(Path::new(&out).is_file());
    }

    #[test]
    fn file_arg_relative_path_resolves_against_process_cwd() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join("profile.json");
        std::fs::write(&file, "{}").unwrap();
        let _cwd = CwdGuard::enter(tmp.path());
        let out = normalize_file_arg("profile.json").expect("ok");
        assert!(out.ends_with("profile.json"), "got {out}");
        assert!(Path::new(&out).is_absolute());
    }

    #[test]
    fn file_arg_nonexistent_path_errors() {
        let err = normalize_file_arg("/this/path/should/not/exist/tasty-test.json");
        assert!(err.is_err());
    }

    #[test]
    fn file_arg_directory_path_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let err = normalize_file_arg(tmp.path().to_str().unwrap());
        assert!(err.is_err(), "directory path should be rejected");
    }

    #[test]
    fn file_arg_empty_errors() {
        assert!(normalize_file_arg("").is_err());
    }

    #[test]
    #[cfg(windows)]
    fn windows_normalize_returns_non_verbatim() {
        // canonicalize 가 \\?\ 를 붙여도 normalize_cwd_arg 결과는 일반 경로여야 한다.
        let tmp = tempfile::tempdir().expect("tempdir");
        let out = normalize_cwd_arg(tmp.path().to_str().unwrap()).expect("ok");
        assert!(!out.starts_with(r"\\?\"), "verbatim prefix leaked: {out}");
        assert!(Path::new(&out).is_dir());
    }

    /// 주석·문자열을 제외한 코드에서 set_current_dir 호출 줄을 센다.
    fn cwd_mutation_call_lines(text: &str) -> usize {
        tasty_doc_guards::source_text::mask_non_code(text)
            .lines()
            .filter(|l| l.contains("set_current_dir("))
            .count()
    }

    /// 레포 소스 트리를 훑어 `set_current_dir(` 호출이 있는 `.rs` 파일의 상대경로를
    /// 모은다. 텍스트 스캔(런타임 `read_dir`/`read_to_string` + `CARGO_MANIFEST_DIR`)이라
    /// cfg 로 컴파일에서 빠지는 파일도 디스크에서 그대로 읽는다 — 두 CI 채널에서 같은
    /// 것을 본다.
    fn scan_cwd_mutations(dir: &Path, base: &Path, out: &mut std::collections::BTreeSet<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 빌드 산출물과 점 디렉터리의 로컬 사본은 검사하지 않는다.
                let skip = path.file_name().is_some_and(|n| {
                    let n = n.to_string_lossy();
                    n == "target" || n.starts_with('.')
                });
                if !skip {
                    scan_cwd_mutations(&path, base, out);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if cwd_mutation_call_lines(&text) > 0 {
                let rel = path.strip_prefix(base).unwrap_or(&path);
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    fn repo_root() -> std::path::PathBuf {
        // CARGO_MANIFEST_DIR = <repo>/crates/tasty-cli → 레포 루트는 그 조부모.
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("repo root (crates/tasty-cli/../..)")
            .to_path_buf()
    }

    /// cwd 변경 파일의 추가와 삭제를 모두 확인한다. 새 호출은 직렬화한 뒤 등록한다.
    #[test]
    fn set_current_dir_is_confined_to_serialized_tests() {
        let root = repo_root();
        let mut found = std::collections::BTreeSet::new();
        scan_cwd_mutations(&root, &root, &mut found);

        let expected: std::collections::BTreeSet<String> =
            ["crates/tasty-cli/src/cwd_resolve.rs".to_string()]
                .into_iter()
                .collect();

        assert_eq!(
            found, expected,
            "set_current_dir 를 만지는 소스 집합이 바뀌었다. 새로 생긴 것은 CWD_LOCK \
             같은 직렬화 처방을 적용한 뒤 이 집합에 등재하고, 사라진 것은 지워라 \
             (unit-test-isolation.md §7)."
        );
    }

    #[test]
    fn cwd_guard_detector_counts_calls_not_mentions() {
        // 언급은 세지 않는다 — 주석이든 문자열이든, 괄호가 붙어 있든.
        assert_eq!(cwd_mutation_call_lines("// set_current_dir 를 조심"), 0);
        assert_eq!(
            cwd_mutation_call_lines("let s = \"set_current_dir call\";"),
            0
        );
        assert_eq!(
            cwd_mutation_call_lines("const NEEDLE: &str = \"set_current_dir(\";"),
            0
        );
        assert_eq!(
            cwd_mutation_call_lines("let s = r#\"call set_current_dir(p) here\"#;"),
            0
        );
        assert_eq!(
            cwd_mutation_call_lines("let x = 1; // set_current_dir(p)"),
            0
        );
        assert_eq!(
            cwd_mutation_call_lines("    std::env::set_current_dir(p).unwrap();"),
            1
        );
    }

    #[test]
    fn cwd_guard_scan_root_is_intact() {
        // 스캔 루트가 통째로 깨지면(경로 오류 등) 집합이 비어 위 가드가 거짓 통과할 수
        // 있다 — 최소한 이 파일(실제 호출 보유)을 잡는지로 스캔이 살아있음을 고정한다.
        let root = repo_root();
        let mut found = std::collections::BTreeSet::new();
        scan_cwd_mutations(&root, &root, &mut found);
        assert!(
            found.contains("crates/tasty-cli/src/cwd_resolve.rs"),
            "스캔이 자기 파일도 못 찾았다 — 스캔 루트가 깨졌다"
        );
    }
}
