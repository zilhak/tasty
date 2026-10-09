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
    /// 항목이 심볼릭 링크인가. 원격 응답은 이 값을 싣지 않으므로 원격 항목은 `NotALink`다.
    /// 읽는 쪽이 GUI의 탐색기뿐이라 `path`처럼 GUI에서만 둔다.
    #[cfg(feature = "gui")]
    pub(crate) link: EntryLink,
}

/// 목록 항목의 링크 상태. 링크는 `path`(링크 자신의 경로)로 탐색하고 조작한다.
#[cfg(feature = "gui")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EntryLink {
    #[default]
    NotALink,
    /// 대상이 있다. 종류·크기·수정 시각은 대상의 값이다.
    Valid,
    /// 대상이 없다. 폴더로 보지 않으며 크기·수정 시각은 링크 자신의 값이다.
    Broken,
}

/// 숨김 파일도 포함한다. 개별 항목 읽기 오류는 건너뛰고 metadata 오류는 기본값으로 처리한다.
/// `DirEntry::metadata`는 링크를 따라가지 않으므로 링크는 대상의 metadata로 종류를 정한다.
pub(crate) fn read_dir_entries(dir: &Path) -> std::io::Result<Vec<DirEntryInfo>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let is_link = entry.file_type().is_ok_and(|t| t.is_symlink());
        // 링크면 대상의 metadata를 먼저 읽고, 대상이 없으면 링크 자신의 값을 쓴다.
        let target = is_link.then(|| std::fs::metadata(&path).ok());
        #[cfg(feature = "gui")]
        let link = match &target {
            None => EntryLink::NotALink,
            Some(Some(_)) => EntryLink::Valid,
            Some(None) => EntryLink::Broken,
        };
        let meta = match target {
            Some(Some(target)) => Some(target),
            _ => entry.metadata().ok(),
        };
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
            #[cfg(feature = "gui")]
            link,
        });
    }
    Ok(out)
}

/// 정렬 방향과 무관하게 디렉터리를 먼저 두고 선택한 컬럼으로 정렬한다.
pub(crate) fn sort_entries(entries: &mut [DirEntryInfo], col: SortColumn, dir: SortDir) {
    entries.sort_by(|a, b| compare_entries(a, b, col, dir));
}

/// `sort_entries` 의 순서. 이미 정렬한 목록에 항목을 끼워 넣을 자리를 찾을 때도 쓴다.
pub(crate) fn compare_entries(
    a: &DirEntryInfo,
    b: &DirEntryInfo,
    col: SortColumn,
    dir: SortDir,
) -> std::cmp::Ordering {
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

/// 세 자리마다 쉼표를 넣는다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
// 테스트 임시 파일의 삭제 실패는 무시한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    #[test]
    fn digits_are_grouped_by_three() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(499_712), "499,712");
        assert_eq!(group_digits(1_204_000), "1,204,000");
    }

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
                #[cfg(feature = "gui")]
                link: Default::default(),
            },
            DirEntryInfo {
                #[cfg(feature = "gui")]
                path: "/a".into(),
                name: "a".into(),
                is_dir: true,
                size: 0,
                modified: None,
                ext: String::new(),
                #[cfg(feature = "gui")]
                link: Default::default(),
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

    #[cfg(unix)]
    #[test]
    fn links_take_their_kind_from_the_target_and_a_broken_link_is_not_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::create_dir(d.join("real")).unwrap();
        std::fs::write(d.join("file.md"), b"hello").unwrap();
        std::os::unix::fs::symlink(d.join("real"), d.join("dir_link")).unwrap();
        std::os::unix::fs::symlink(d.join("file.md"), d.join("file_link.md")).unwrap();
        std::os::unix::fs::symlink(d.join("gone"), d.join("broken")).unwrap();
        let entries = read_dir_entries(d).unwrap();
        let find = |name: &str| entries.iter().find(|e| e.name == name).unwrap();
        assert!(find("dir_link").is_dir);
        #[cfg(feature = "gui")]
        assert_eq!(find("dir_link").link, EntryLink::Valid);
        assert!(!find("file_link.md").is_dir);
        assert_eq!(find("file_link.md").size, 5);
        #[cfg(feature = "gui")]
        assert_eq!(find("file_link.md").link, EntryLink::Valid);
        assert!(!find("broken").is_dir);
        #[cfg(feature = "gui")]
        assert_eq!(find("broken").link, EntryLink::Broken);
        assert!(find("real").is_dir);
        #[cfg(feature = "gui")]
        assert_eq!(find("real").link, EntryLink::NotALink);
    }
}
