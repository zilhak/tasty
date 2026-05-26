//! `~/.tasty/themes/*.toml` 디렉토리 스캔 + 사용 가능한 테마 목록 캐시.
//!
//! 파일명 stem = 테마 id. 잘못된 파일은 `warn!` 후 스킵.

use std::fs;
use std::sync::{Mutex, OnceLock};

use crate::file::ThemeFile;
use crate::store::{ThemeStoreError, themes_dir};

/// 스캔 결과 한 항목.
#[derive(Debug, Clone)]
pub struct ThemeEntry {
    /// 파일명 stem.
    pub id: String,
    /// 사용자에게 표시할 이름. 파일에 `label` 이 있으면 그 값, 없으면 `id`.
    pub label: String,
    /// 파싱된 파일 (필드 일부만 채워질 수 있음).
    pub file: ThemeFile,
}

/// 부팅 시 1회 캐시. `rescan()` 으로 명시적 갱신 가능.
static CACHE: OnceLock<Mutex<Vec<ThemeEntry>>> = OnceLock::new();

fn cache() -> &'static Mutex<Vec<ThemeEntry>> {
    CACHE.get_or_init(|| Mutex::new(do_scan().unwrap_or_default()))
}

/// 디스크에서 다시 스캔하여 캐시 갱신.
pub fn rescan() -> Result<Vec<ThemeEntry>, ThemeStoreError> {
    let entries = do_scan()?;
    let lock = cache();
    let mut guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    *guard = entries.clone();
    Ok(entries)
}

/// 현재 캐시된 테마 목록. 없으면 디스크 스캔하여 캐시한다.
pub fn scan_themes() -> Vec<ThemeEntry> {
    let lock = cache();
    let guard = lock.lock().unwrap_or_else(|p| p.into_inner());
    guard.clone()
}

fn do_scan() -> Result<Vec<ThemeEntry>, ThemeStoreError> {
    let dir = themes_dir()?;
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(&dir) {
        Ok(it) => it,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("themes dir entry read failed: {e}");
                continue;
            }
        };
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            tracing::warn!("theme file has no valid stem: {}", path.display());
            continue;
        };
        let text = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("theme file read failed ({}): {e}", path.display());
                continue;
            }
        };
        let file = match ThemeFile::parse(&text) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("theme file parse failed ({}): {e}", path.display());
                continue;
            }
        };
        let label = file.label.clone().unwrap_or_else(|| stem.to_string());
        out.push(ThemeEntry {
            id: stem.to_string(),
            label,
            file,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}
