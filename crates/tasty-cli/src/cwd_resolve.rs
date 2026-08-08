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
fn canonicalize_from_cwd(raw: &str, label: &str) -> Result<PathBuf> {
    let p = Path::new(raw);
    let base: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .with_context(|| "failed to read current_dir()")?
            .join(p)
    };
    base.canonicalize()
        .with_context(|| format!("{label} '{raw}' does not exist or is not accessible"))
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
        bail!("cwd is empty");
    }
    let canon = canonicalize_from_cwd(raw, "cwd")?;
    let meta = canon
        .metadata()
        .with_context(|| format!("cwd '{}' metadata read failed", canon.display()))?;
    if !meta.is_dir() {
        bail!("cwd '{}' is not a directory", canon.display());
    }
    Ok(strip_verbatim(&canon))
}

/// raw 값을 호출자 cwd 기준 absolute path 로 정규화 + **파일** 존재 검증
/// (`path_kind = "file"`, `normalize_cwd_arg` 의 파일 버전 — 디렉토리면 거부).
/// 파일 *내용* 검증(JSON 파싱 등)은 하지 않는다 — 그건 그 내용을 아는 plugin 핸들러
/// 몫이다(예: claude 세션 프로필의 JSON 유효성은 `tasty-plugin-claude` 가 검증).
pub fn normalize_file_arg(raw: &str) -> Result<String> {
    if raw.is_empty() {
        bail!("path is empty");
    }
    let canon = canonicalize_from_cwd(raw, "file")?;
    let meta = canon
        .metadata()
        .with_context(|| format!("file '{}' metadata read failed", canon.display()))?;
    if !meta.is_file() {
        bail!("file '{}' is not a regular file", canon.display());
    }
    Ok(strip_verbatim(&canon))
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
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(tmp.path()).unwrap();
        let out = normalize_file_arg("profile.json").expect("ok");
        std::env::set_current_dir(prev).unwrap();
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
}
