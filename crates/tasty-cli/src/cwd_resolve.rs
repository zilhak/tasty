//! cwd 인자 정규화 + 검증. 호스트는 absolute + valid 만 받는다는 contract.
//!
//! `--cwd` / `--directory` 같이 디렉토리를 가리키는 인자는 CLI process 의
//! cwd 기준으로 absolute path 로 변환된다 — 호스트(GUI)의 cwd 와 무관하게,
//! 사용자가 명령을 친 디렉토리 기준으로 해석된다.
//!
//! 검증 실패는 `bail!` — 호출자가 stderr 출력 + exit 1.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// raw 값을 호출자 cwd 기준 absolute path 로 정규화 + 디렉토리 존재 검증.
///
/// - 절대경로 → `canonicalize()` (symlink 해석) 후 dir 검증.
/// - 상대경로 → `env::current_dir()?.join(raw).canonicalize()` 후 dir 검증.
/// - 비존재 / file / 권한 부족 → `Err`.
pub fn normalize_cwd_arg(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("cwd is empty");
    }
    let p = Path::new(raw);
    let base: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .with_context(|| "failed to read current_dir()")?
            .join(p)
    };
    let canon = base
        .canonicalize()
        .with_context(|| format!("cwd '{raw}' does not exist or is not accessible"))?;
    let meta = canon
        .metadata()
        .with_context(|| format!("cwd '{}' metadata read failed", canon.display()))?;
    if !meta.is_dir() {
        bail!("cwd '{}' is not a directory", canon.display());
    }
    let out = canon.to_string_lossy().into_owned();
    // Windows 한정: `std::fs::canonicalize` 는 `\\?\` verbatim(extended-length)
    // prefix 가 붙은 경로를 반환한다. 이 cwd 는 PTY working_dir 로 그대로 전달되어
    // 자식 프로세스(예: Claude Code/bun)의 process.cwd() 가 되는데, 일부 도구는
    // `\\?\` 경로를 file URL 로 변환하지 못해 깨진다(pathToFileURL). 따라서
    // verbatim prefix 를 제거해 일반 경로로 돌려준다. 비-Windows 는 영향 없음.
    #[cfg(windows)]
    let out = strip_windows_verbatim_prefix(&out);
    Ok(out)
}

/// Windows verbatim(extended-length) prefix `\\?\` 를 제거한다.
/// - `\\?\UNC\server\share\..` → `\\server\share\..`
/// - `\\?\C:\..`               → `C:\..`
/// - 이미 일반 경로            → 그대로 반환
#[cfg(windows)]
fn strip_windows_verbatim_prefix(p: &str) -> String {
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        p.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // tempdir 안에 하위 디렉토리.
        let sub = tmp.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        // process cwd 를 tempdir 로 옮기고 상대 경로 "sub" 가 sub 로 풀리는지.
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let out = normalize_cwd_arg("sub").expect("ok");
        std::env::set_current_dir(prev).unwrap();
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
    #[cfg(windows)]
    fn windows_strips_verbatim_disk_prefix() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\E:\workspace\tasty\.worktree\wt-1"),
            r"E:\workspace\tasty\.worktree\wt-1"
        );
    }

    #[test]
    #[cfg(windows)]
    fn windows_strips_verbatim_unc_prefix() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"\\?\UNC\server\share\dir"),
            r"\\server\share\dir"
        );
    }

    #[test]
    #[cfg(windows)]
    fn windows_leaves_normal_path_untouched() {
        assert_eq!(
            strip_windows_verbatim_prefix(r"E:\already\normal"),
            r"E:\already\normal"
        );
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
}
