//! `~/.tasty/themes/*.toml` 디렉토리 스캔 + 사용 가능한 테마 목록 캐시.
//!
//! 파일명 stem = 테마 id. 잘못된 파일은 `warn!` 후 스킵.

use std::fs;
use std::sync::atomic::AtomicBool;
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

/// 임계구역이 `Vec` 통째 교체와 복제뿐이라 복구가 맞다. 조용히 두면 목록이 낡거나 빈
/// 채로 굳는 것이 **설정 화면에 테마가 안 보인다** 로만 드러나 원인을 되짚을 수 없다.
pub(crate) const CACHE_WHAT: &str = "the theme scan cache";
pub(crate) static CACHE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 디스크에서 다시 스캔하여 캐시 갱신.
pub fn rescan() -> Result<Vec<ThemeEntry>, ThemeStoreError> {
    let entries = do_scan()?;
    let lock = cache();
    let mut guard =
        tasty_utils::poison::recover_mutex(lock.lock(), CACHE_WHAT, &CACHE_POISON_REPORTED);
    *guard = entries.clone();
    Ok(entries)
}

/// 현재 캐시된 테마 목록. 없으면 디스크 스캔하여 캐시한다.
pub fn scan_themes() -> Vec<ThemeEntry> {
    let lock = cache();
    let guard = tasty_utils::poison::recover_mutex(lock.lock(), CACHE_WHAT, &CACHE_POISON_REPORTED);
    guard.clone()
}

#[allow(clippy::cognitive_complexity)] // complexity-exempt: 테마 디렉토리 스캔 — 확장자/스템/읽기/파싱 4단계 조기 continue 나열, tasty-presets::storage::scan_dir 와 동형 패턴.
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

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// 복구는 이 자리에 **이미** 있었다 — 이번에 더한 것은 관측뿐이라, "poison 이어도 값이
    /// 나온다" 만 보는 테스트는 헬퍼를 되돌려도 그대로 통과한다(변이가 안 죽는다). 그래서
    /// 보고 플래그가 실제로 뒤집혔는지를 함께 본다. 그 플래그가 곧 `tracing::error!` 가
    /// 나갔다는 증거이고, `unwrap_or_else(into_inner)` 로 되돌리면 `false` 로 남는다.
    #[test]
    fn a_poisoned_scan_cache_still_lists_and_says_so() {
        let before = scan_themes().len();

        let panicked = std::thread::spawn(|| {
            let _held = cache().lock().expect("not poisoned yet");
            panic!("poison the scan cache");
        })
        .join();
        assert!(panicked.is_err());
        assert!(
            cache().lock().is_err(),
            "the cache lock must be poisoned now"
        );

        assert_eq!(scan_themes().len(), before, "the cached list must survive");
        assert!(
            CACHE_POISON_REPORTED.load(Ordering::Relaxed),
            "the poison must have been reported once"
        );
    }
}
