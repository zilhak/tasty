//! Explorer와 로컬·원격 파일 선택기가 공유하는 디렉터리 조회·정렬·표시 함수.

use std::path::Path;
#[cfg(feature = "gui")]
use std::path::PathBuf;
use std::time::SystemTime;

use tasty_model::{SortColumn, SortDir};

#[derive(Clone)]
pub(crate) struct DirEntryInfo {
    /// 원격 응답에는 없다. client가 조회 경로와 name으로 만들며 GUI에서만 사용한다.
    #[cfg(feature = "gui")]
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    /// metadata의 바이트 길이. 디렉터리도 OS가 돌려준 길이를 보관하며 metadata를 못 읽으면 0이다.
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    /// 소문자 확장자. 디렉터리이거나 확장자가 없으면 비어 있다.
    pub(crate) ext: String,
}

/// 숨김 파일도 포함한다. 개별 항목 읽기 오류는 건너뛰고 metadata 오류는 기본값으로 처리한다.
pub(crate) fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<DirEntryInfo>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified = meta.as_ref().and_then(|m| m.modified().ok());
        let ext = if is_dir {
            String::new()
        } else {
            path.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase())
                .unwrap_or_default()
        };
        out.push(DirEntryInfo {
            #[cfg(feature = "gui")]
            path,
            name,
            is_dir,
            size,
            modified,
            ext,
        });
    }
    Ok(out)
}

/// 정렬 방향과 무관하게 디렉터리를 먼저 두고 선택한 컬럼으로 정렬한다.
pub(crate) fn sort_entries(entries: &mut [DirEntryInfo], col: SortColumn, dir: SortDir) {
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return b.is_dir.cmp(&a.is_dir);
        }
        let ord = match col {
            SortColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortColumn::Size => a.size.cmp(&b.size),
            SortColumn::Modified => a.modified.cmp(&b.modified),
            SortColumn::Type => a
                .ext
                .cmp(&b.ext)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        };
        match dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    });
}

/// Unix epoch 일수로 UTC 날짜를 표시한다. 값이 없거나 epoch 이전이면 대시를 쓴다.
#[cfg(feature = "gui")]
pub(crate) fn format_modified(m: Option<SystemTime>) -> String {
    let Some(t) = m else { return "—".to_string() };
    let dur = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "—".to_string(),
    };
    let days = dur.as_secs() / 86_400;
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Howard Hinnant의 days-to-civil 알고리즘(proleptic Gregorian).
#[cfg(feature = "gui")]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 1024 단위로 파일 크기를 표시한다. 디렉터리는 대시다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn human_size(is_dir: bool, size: u64) -> String {
    if is_dir {
        return "—".to_string();
    }
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut s = size as f64;
    let mut u = 0;
    while s >= 1024.0 && u < UNITS.len() - 1 {
        s /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{} {}", size, UNITS[0])
    } else {
        format!("{:.1} {}", s, UNITS[u])
    }
}

#[cfg(test)]
// 테스트 임시 파일의 삭제 실패는 무시한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(true, 999), "—");
        assert_eq!(human_size(false, 512), "512 B");
        assert_eq!(human_size(false, 4096), "4.0 KB");
    }

    #[test]
    fn sort_dirs_first() {
        let mut v = vec![
            DirEntryInfo {
                #[cfg(feature = "gui")]
                path: "/z".into(),
                name: "z".into(),
                is_dir: false,
                size: 1,
                modified: None,
                ext: String::new(),
            },
            DirEntryInfo {
                #[cfg(feature = "gui")]
                path: "/a".into(),
                name: "a".into(),
                is_dir: true,
                size: 0,
                modified: None,
                ext: String::new(),
            },
        ];
        sort_entries(&mut v, SortColumn::Name, SortDir::Asc);
        assert!(v[0].is_dir);
    }

    #[test]
    fn read_dir_entries_lists_files_and_dirs() {
        let tmp = std::env::temp_dir().join(format!("tasty_fs_list_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("sub")).unwrap();
        std::fs::write(tmp.join("a.txt"), b"hello").unwrap();
        let mut entries = read_dir_entries(&tmp).unwrap();
        sort_entries(&mut entries, SortColumn::Name, SortDir::Asc);
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "sub");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].name, "a.txt");
        assert_eq!(entries[1].size, 5);
        assert_eq!(entries[1].ext, "txt");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
