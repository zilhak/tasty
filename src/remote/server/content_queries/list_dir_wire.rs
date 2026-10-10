//! `list_dir_result` 의 항목·목록·볼륨을 wire 값으로 만든다. 받는 쪽은 `attach_client::wire::list_dir` 다.

/// modified는 Unix epoch 초로 보낸다. 사람이 읽는 날짜 표기는 client가 만든다.
/// `os_hidden` 은 OS 가 숨김으로 표시한 항목에만 싣는다. 이 필드를 모르는 옛 client 는 무시하고,
/// 이 필드가 없는 옛 server 의 응답은 받는 쪽에서 거짓으로 읽는다.
pub(super) fn list_dir_entry_wire(e: &crate::core::fs_list::DirEntryInfo) -> serde_json::Value {
    let modified_unix = e
        .modified
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let wire = serde_json::json!({
        "name": e.name,
        "is_dir": e.is_dir,
        "size": e.size,
        "modified_unix": modified_unix,
        "ext": e.ext,
    });
    #[cfg(feature = "gui")]
    let wire = {
        let mut wire = wire;
        if e.os_hidden {
            wire["os_hidden"] = serde_json::Value::Bool(true);
        }
        wire
    };
    wire
}

/// entries의 직렬화 크기 제한. 프레임 상한을 넘기면 연결이 종료되므로 나머지 필드의 여유를 둔다.
/// 전체 프레임을 다시 재는 것은 아니며 각 entry의 직렬화 크기만 합산한다.
const LIST_DIR_ENTRIES_BYTE_BUDGET: usize = 700 * 1024;

/// 예산 안에 들어가는 entry만 반환한다. 두 번째 값은 생략한 항목이 있는지다.
pub(super) fn list_dir_entries_wire_capped(
    entries: &[crate::core::fs_list::DirEntryInfo],
) -> (Vec<serde_json::Value>, bool) {
    list_dir_entries_wire_capped_with_budget(entries, LIST_DIR_ENTRIES_BYTE_BUDGET)
}

pub(super) fn list_dir_entries_wire_capped_with_budget(
    entries: &[crate::core::fs_list::DirEntryInfo],
    mut budget: usize,
) -> (Vec<serde_json::Value>, bool) {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let wire = list_dir_entry_wire(e);
        let approx_len = serde_json::to_vec(&wire).map(|b| b.len()).unwrap_or(0);
        if approx_len > budget {
            return (out, true);
        }
        budget -= approx_len;
        out.push(wire);
    }
    (out, false)
}

/// 읽은 폴더가 있는 볼륨의 식별자. client 는 두 원격 폴더의 값이 같을 때만 끌어 놓기의 기본 동작을
/// 이동으로 정한다. 같은 서버에서 읽은 값끼리만 비교하므로 서버 밖에서 뜻이 없다. 모르면 싣지 않는다.
#[cfg(unix)]
pub(super) fn listing_volume(dir: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir).ok().map(|m| m.dev())
}

/// Windows 는 드라이브·UNC 접두를 볼륨으로 본다. 로컬 끌기의 같은 디스크 판정과 같은 기준이다.
#[cfg(windows)]
pub(super) fn listing_volume(dir: &str) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let prefix = std::path::Path::new(dir).components().next()?;
    let mut hasher = std::hash::DefaultHasher::new();
    prefix.as_os_str().to_ascii_lowercase().hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn listing_volume(_dir: &str) -> Option<u64> {
    None
}

/// `os_hidden` 이 gui 조합에만 있으므로 시험도 그 조합에서만 돈다.
#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;
    use crate::core::fs_list::DirEntryInfo;

    fn entry(name: &str) -> DirEntryInfo {
        DirEntryInfo {
            path: name.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn only_an_os_hidden_entry_carries_the_mark() {
        let marked = DirEntryInfo {
            os_hidden: true,
            ..entry("desktop.ini")
        };
        assert_eq!(list_dir_entry_wire(&marked)["os_hidden"], true);
        assert!(
            list_dir_entry_wire(&entry("a.txt"))
                .get("os_hidden")
                .is_none()
        );
    }

    /// 새 server 응답을 이 필드를 모르는 옛 client 의 항목 형태로 읽어도 실패하지 않는다.
    #[test]
    fn an_old_client_reads_a_marked_entry_without_error() {
        #[derive(serde::Deserialize)]
        struct OldClientEntry {
            name: String,
            is_dir: bool,
            size: u64,
            #[serde(default)]
            modified_unix: Option<u64>,
            #[serde(default)]
            ext: String,
        }
        let marked = DirEntryInfo {
            os_hidden: true,
            ..entry("desktop.ini")
        };
        let old: OldClientEntry =
            serde_json::from_value(list_dir_entry_wire(&marked)).expect("old client parse");
        assert_eq!(old.name, "desktop.ini");
        assert!(!old.is_dir);
        assert_eq!(
            (old.size, old.modified_unix, old.ext.as_str()),
            (0, None, "")
        );
    }
}

#[cfg(all(test, unix))]
mod volume_tests {
    use super::listing_volume;

    #[test]
    fn folders_on_one_volume_report_the_same_id() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        std::fs::create_dir(&a).unwrap();
        let root = listing_volume(&dir.path().to_string_lossy());
        assert!(root.is_some());
        assert_eq!(listing_volume(&a.to_string_lossy()), root);
        assert_eq!(
            listing_volume(&dir.path().join("missing").to_string_lossy()),
            None
        );
    }
}
