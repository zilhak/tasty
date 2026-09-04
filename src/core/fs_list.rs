//! 디렉토리 엔트리 나열 — Explorer surface 와 파일 피커(로컬/원격)가 공유하는 순수
//! I/O 계층. GUI/egui 비의존이라 attach 서버측 핸들러(원격 디렉토리 조회 요청 처리)
//! 에서도 그대로 재사용한다 — "로컬/원격이 완전히 동일한 요청/응답 스키마" 설계
//! 목표(파일 피커 원격 디렉토리 브라우징)의 기반.
//!
//! 원래 `src/adapters/ui/surface/explorer/view.rs` 의 private 함수였던 것을 이
//! 모듈로 추출해 `pub(crate)` 로 일반화했다.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tasty_model::{SortColumn, SortDir};

/// 디렉토리 엔트리 한 줄의 메타데이터 (디스크에서 1회 읽어 캐시).
#[derive(Clone)]
pub(crate) struct DirEntryInfo {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) is_dir: bool,
    /// 파일 바이트 크기 (디렉토리는 0).
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    /// 소문자 확장자 (없으면 빈 문자열).
    pub(crate) ext: String,
}

/// 디렉토리 엔트리를 읽어 메타데이터로 변환. 숨김 파일은 포함(필터는 뷰 레벨).
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

/// 엔트리 정렬: 디렉토리 우선, 그 다음 선택된 컬럼/방향.
pub(crate) fn sort_entries(entries: &mut [DirEntryInfo], col: SortColumn, dir: SortDir) {
    entries.sort_by(|a, b| {
        // 디렉토리는 항상 위 (방향 무관).
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

/// 수정 시각을 `YYYY-MM-DD` 로. 시스템 시계 의존 포맷은 chrono 없이 epoch 계산.
/// 로컬/원격(wire 복원) 어디서 만들어진 `SystemTime` 이든 동일 포맷 — 사람이 읽는
/// 포맷팅은 view 렌더 직전에서만(파일 피커의 wire 조립/파싱은 `modified_unix: u64`
/// 그대로 다룬다).
pub(crate) fn format_modified(m: Option<SystemTime>) -> String {
    let Some(t) = m else { return "—".to_string() };
    let dur = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "—".to_string(),
    };
    let days = dur.as_secs() / 86_400;
    // 1970-01-01 기준 일수 → (y, m, d). 윤년 포함 그레고리력.
    let (y, mo, d) = civil_from_days(days as i64);
    format!("{y:04}-{mo:02}-{d:02}")
}

/// Howard Hinnant days→civil 알고리즘 (proleptic Gregorian).
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

/// 사람이 읽는 파일 크기 (e.g. "4 KB"). 디렉토리는 "—".
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
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
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
                path: "/z".into(),
                name: "z".into(),
                is_dir: false,
                size: 1,
                modified: None,
                ext: String::new(),
            },
            DirEntryInfo {
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
