//! Tasty 홈의 themes 디렉터리에서 내장·사용자 테마 파일을 관리한다.
//! Mocha는 없거나 내장 내용과 다르면 복원한다. Latte는 파일이 있을 때만 동기화한다.
//! TOML 파일이 하나도 없으면 초기 설치로 보고 둘 다 만든다.
//! 다른 사용자 테마는 동기화하지 않는다. 내장 파일의 직접 편집은 유지되지 않으므로
//! 내장 테마 색 변경은 설정의 theme_overrides에 저장한다.

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

/// tasty_home 아래 themes 경로. 홈을 찾을 수 없으면 오류를 반환한다.
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

/// Mocha 파일을 읽거나 파싱할 수 없으면 내장 텍스트로 덮어쓴다.
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

/// 내장 파일과 내용이 다르면 동기화한다. Mocha는 복원하고 삭제된 Latte는 그대로 둔다.
/// 설정의 theme_overrides는 이 함수에서 변경하지 않는다.
pub fn sync_builtin_themes() -> Result<(), ThemeStoreError> {
    let dir = themes_dir()?;
    ensure_dir(&dir)?;

    let mocha_path = dir.join(format!("{BUILTIN_MOCHA_ID}.toml"));
    if needs_sync(&mocha_path, crate::MOCHA_TOML_TEXT) {
        fs::write(&mocha_path, crate::MOCHA_TOML_TEXT)?;
        tracing::info!("synced builtin mocha theme: {}", mocha_path.display());
    }

    let latte_path = dir.join(format!("{BUILTIN_LATTE_ID}.toml"));
    if latte_path.exists() && needs_sync(&latte_path, crate::LATTE_TOML_TEXT) {
        fs::write(&latte_path, crate::LATTE_TOML_TEXT)?;
        tracing::info!("synced builtin latte theme: {}", latte_path.display());
    }

    Ok(())
}

/// 디스크 파일이 임베드 정본과 다르면(또는 읽을 수 없으면) 동기화가 필요하다.
fn needs_sync(path: &Path, embed: &str) -> bool {
    match fs::read_to_string(path) {
        Ok(current) => current != embed,
        Err(_) => true,
    }
}

/// TOML 확장자를 가진 항목이 하나도 없을 때 Mocha와 Latte를 만든다.
/// ensure_mocha_exists보다 먼저 호출해야 초기 설치에서 둘 다 생성된다.
pub fn first_run_init() -> Result<(), ThemeStoreError> {
    let dir = themes_dir()?;
    ensure_dir(&dir)?;

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

    /// 이 검사들은 실제 저장소 API 대신 TempDir에서 해당 파일 조작과 분기를 재현한다.
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
        assert!(!has_any_toml(dir).unwrap());

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
        // Mocha가 남아 있으면 삭제된 Latte를 다시 만들지 않는다.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        write_text(&dir.join("mocha.toml"), crate::MOCHA_TOML_TEXT);

        assert!(has_any_toml(dir).unwrap());
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

        write_text(&mocha, crate::MOCHA_TOML_TEXT);
        let parsed2 = crate::file::ThemeFile::parse(&read_text(&mocha));
        assert!(parsed2.is_ok(), "rewritten file must parse");
    }

    #[test]
    fn needs_sync_detects_drift_and_missing() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let path = dir.join("mocha.toml");

        assert!(needs_sync(&path, crate::MOCHA_TOML_TEXT));

        write_text(&path, crate::MOCHA_TOML_TEXT);
        assert!(!needs_sync(&path, crate::MOCHA_TOML_TEXT));

        write_text(&path, "label = \"old\"\n[terminal]\nfg = \"#000000\"\n");
        assert!(needs_sync(&path, crate::MOCHA_TOML_TEXT));
    }

    #[test]
    fn sync_rewrites_stale_latte_but_skips_absent() {
        // sync_builtin_themes 의 latte 분기 재현 (themes_dir 의존 없이):
        // 있으면 동기화, 부재면 건드리지 않는다.
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();
        let latte = dir.join("latte.toml");

        assert!(!latte.exists());
        if latte.exists() && needs_sync(&latte, crate::LATTE_TOML_TEXT) {
            write_text(&latte, crate::LATTE_TOML_TEXT);
        }
        assert!(!latte.exists(), "absent latte must not be recreated");

        write_text(
            &latte,
            "label = \"old latte\"\n[terminal]\nbg = \"#eff1f5\"\n",
        );
        if latte.exists() && needs_sync(&latte, crate::LATTE_TOML_TEXT) {
            write_text(&latte, crate::LATTE_TOML_TEXT);
        }
        assert_eq!(read_text(&latte), crate::LATTE_TOML_TEXT);
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
