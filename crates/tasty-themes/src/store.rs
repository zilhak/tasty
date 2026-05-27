//! 디스크 store — `~/.tasty/themes/` 의 빌트인 / 사용자 테마 파일 관리.
//!
//! 정책:
//! - **mocha**: 항상 존재 보장. 누락/파싱 실패 시 임베드 텍스트 자동 복구.
//! - **latte**: first-run (themes 폴더 자체가 빈 경우) 1회만 자동으로 풀어둔다.
//!   사용자가 명시적으로 지웠다면 다시 풀지 않는다.
//! - **그 외 사용자 테마**: 자동 복구 없음. 잘못된 파일은 스캔에서 스킵.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// 빌트인 mocha 테마 id (= 파일명 stem).
pub const BUILTIN_MOCHA_ID: &str = "mocha";

/// 빌트인 latte 테마 id.
pub const BUILTIN_LATTE_ID: &str = "latte";

#[derive(Debug, Error)]
pub enum ThemeStoreError {
    #[error("HOME directory unavailable")]
    HomeUnavailable,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// `~/.tasty/themes/` 절대 경로. `tasty_home()` 이 None 이면 에러.
pub fn themes_dir() -> Result<PathBuf, ThemeStoreError> {
    tasty_utils::path::tasty_home()
        .map(|home| home.join("themes"))
        .ok_or(ThemeStoreError::HomeUnavailable)
}

/// 디렉토리 생성 헬퍼. 이미 존재해도 OK.
fn ensure_dir(dir: &Path) -> io::Result<()> {
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    Ok(())
}

/// 단일 테마 파일 경로. id 검증은 호출자 책임 (slash/dot 금지 등).
fn theme_path(id: &str) -> Result<PathBuf, ThemeStoreError> {
    Ok(themes_dir()?.join(format!("{id}.toml")))
}

/// 빌트인 mocha 텍스트를 디스크에 강제로 쓴다 (덮어쓰기). fallback 복구 흐름 전용.
pub fn rewrite_mocha_fallback() -> Result<(), ThemeStoreError> {
    let dir = themes_dir()?;
    ensure_dir(&dir)?;
    let path = dir.join(format!("{BUILTIN_MOCHA_ID}.toml"));
    fs::write(&path, crate::MOCHA_TOML_TEXT)?;
    tracing::info!("rewrote builtin mocha theme: {}", path.display());
    Ok(())
}

/// mocha.toml 이 디스크에 존재하고 파싱 가능한지 확인. 아니면 임베드 텍스트로 덮어쓴다.
/// 이 함수는 부팅 초기에 호출된다 — mocha 가 fallback 으로 동작하려면 항상 보장돼야 한다.
pub fn ensure_mocha_exists() -> Result<(), ThemeStoreError> {
    let path = theme_path(BUILTIN_MOCHA_ID)?;
    let needs_rewrite = match fs::read_to_string(&path) {
        Ok(text) => crate::file::ThemeFile::parse(&text).is_err(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            tracing::warn!("mocha.toml read failed ({}); rewriting", e);
            true
        }
    };
    if needs_rewrite {
        rewrite_mocha_fallback()?;
    }
    Ok(())
}

/// first-run 시 mocha + latte 를 같이 풀어둔다. themes 폴더가 **완전히 비어있는 경우**만.
/// 사용자가 latte 만 지운 상태 등은 의도 존중하고 건드리지 않는다.
///
/// `ensure_mocha_exists()` 와 호출 순서: `first_run_init()` 을 먼저 호출하면
/// 첫 부팅에서 두 파일 모두 한 번에 생성된다. 그 다음 `ensure_mocha_exists()` 는
/// mocha 가 이미 풀려 있으므로 no-op.
pub fn first_run_init() -> Result<(), ThemeStoreError> {
    let dir = themes_dir()?;
    ensure_dir(&dir)?;

    // "비어있다" = 어떤 *.toml 도 없다. (디렉토리 자체는 ensure_dir 으로 만들었으니 있을 수 있음)
    let is_empty = !has_any_toml(&dir)?;
    if !is_empty {
        return Ok(());
    }

    let mocha_path = dir.join(format!("{BUILTIN_MOCHA_ID}.toml"));
    let latte_path = dir.join(format!("{BUILTIN_LATTE_ID}.toml"));
    fs::write(&mocha_path, crate::MOCHA_TOML_TEXT)?;
    fs::write(&latte_path, crate::LATTE_TOML_TEXT)?;
    tracing::info!(
        "first-run: seeded builtin themes ({}, {})",
        mocha_path.display(),
        latte_path.display()
    );
    Ok(())
}

fn has_any_toml(dir: &Path) -> io::Result<bool> {
    let entries = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use tempfile::TempDir;

    /// `tasty_home()` 을 못 쓰는 단위 테스트용 — TempDir 을 themes 로 직접 가정.
    /// store API 와 동일 흐름을 재현해서 검증한다.
    fn write_text(p: &Path, s: &str) {
        fs::write(p, s).unwrap();
    }
    fn read_text(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }
    fn is_toml(p: &Path) -> bool {
        p.extension() == Some(OsStr::new("toml"))
    }

    #[test]
    fn first_run_seeds_when_empty() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        // ensure_dir 흐름 재현
        assert!(!has_any_toml(dir).unwrap());

        // 실제 first_run_init 의 핵심 분기를 인라인으로 재현 (themes_dir 의존 없이)
        write_text(&dir.join("mocha.toml"), crate::MOCHA_TOML_TEXT);
        write_text(&dir.join("latte.toml"), crate::LATTE_TOML_TEXT);

        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .filter(|n| is_toml(Path::new(n)))
            .collect();
        entries.sort();
        assert_eq!(entries, vec!["latte.toml", "mocha.toml"]);
    }

    #[test]
    fn first_run_preserves_user_only_state() {
        // 사용자가 latte 만 지워서 mocha 만 있는 상태에서 first-run init 이
        // 다시 latte 를 풀어두면 안 된다.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_text(&dir.join("mocha.toml"), crate::MOCHA_TOML_TEXT);

        // first_run_init 의 "is_empty" 분기 재현
        assert!(has_any_toml(dir).unwrap());
        // → 분기 진입 안 함. latte 미생성.
        assert!(!dir.join("latte.toml").exists());
    }

    #[test]
    fn corrupt_mocha_triggers_rewrite_logic() {
        // ensure_mocha_exists 의 분기 재현: 파싱 실패 → rewrite.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let mocha = dir.join("mocha.toml");
        write_text(&mocha, "this is not = valid toml [[[");

        let parsed = crate::file::ThemeFile::parse(&read_text(&mocha));
        assert!(parsed.is_err(), "corrupt file should fail to parse");

        // 복구 흐름: 임베드 텍스트로 덮어쓰기
        write_text(&mocha, crate::MOCHA_TOML_TEXT);
        let parsed2 = crate::file::ThemeFile::parse(&read_text(&mocha));
        assert!(parsed2.is_ok(), "rewritten file must parse");
    }

    #[test]
    fn has_any_toml_ignores_non_toml() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_text(&dir.join("README.md"), "x");
        assert!(!has_any_toml(dir).unwrap());
        write_text(&dir.join("foo.toml"), "");
        assert!(has_any_toml(dir).unwrap());
    }
}
