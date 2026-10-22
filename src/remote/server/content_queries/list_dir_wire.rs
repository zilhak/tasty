//! `list_dir_result` 의 항목·목록·볼륨을 wire 값으로 만든다. 받는 쪽은 `attach_client::wire::list_dir` 다.

/// modified는 Unix epoch 초로 보낸다. 사람이 읽는 날짜 표기는 client가 만든다.
/// `os_hidden` 은 OS 가 숨김으로 표시한 항목에만 싣는다. 이 필드를 모르는 옛 client 는 무시하고,
/// 이 필드가 없는 옛 server 의 응답은 받는 쪽에서 거짓으로 읽는다.
/// `os_hidden` 은 GUI 조합의 server 만 싣는다.
/// `link` 는 링크 항목에만 `"valid"`(대상 있음)·`"broken"`(대상 없음)으로 싣는다. headless server 도 싣는다.
/// 받는 규칙은 `os_hidden` 과 같고, 없으면 링크 아님이다.
pub(super) fn list_dir_entry_wire(e: &crate::core::fs_list::DirEntryInfo) -> serde_json::Value {
    let modified_unix = e
        .modified
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let mut wire = serde_json::json!({
        "name": e.name,
        "is_dir": e.is_dir,
        "size": e.size,
        "modified_unix": modified_unix,
        "ext": e.ext,
    });
    #[cfg(feature = "gui")]
    if e.os_hidden {
        wire["os_hidden"] = serde_json::Value::Bool(true);
    }
    let link = match e.link {
        crate::core::fs_list::EntryLink::NotALink => None,
        crate::core::fs_list::EntryLink::Valid => Some("valid"),
        crate::core::fs_list::EntryLink::Broken => Some("broken"),
    };
    if let Some(link) = link {
        wire["link"] = link.into();
    }
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

/// `os_hidden`·`link` 가 gui 조합에만 있으므로 시험도 그 조합에서만 돈다.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fs_list::DirEntryInfo;

    fn entry(name: &str) -> DirEntryInfo {
        DirEntryInfo {
            #[cfg(feature = "gui")]
            path: name.into(),
            name: name.into(),
            ..Default::default()
        }
    }

    #[cfg(feature = "gui")]
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

    #[test]
    fn only_a_link_carries_its_state() {
        use crate::core::fs_list::EntryLink;
        let with = |link| DirEntryInfo { link, ..entry("a") };
        assert_eq!(
            list_dir_entry_wire(&with(EntryLink::Valid))["link"],
            "valid"
        );
        assert_eq!(
            list_dir_entry_wire(&with(EntryLink::Broken))["link"],
            "broken"
        );
        assert!(
            list_dir_entry_wire(&with(EntryLink::NotALink))
                .get("link")
                .is_none()
        );
    }

    /// 원격 호스트의 실제 링크를 같은 목록 함수로 읽어 보낸다. 폴더 링크는 폴더, 파일 링크와 대상이 없는
    /// 링크는 파일 종류로 가고 링크 상태가 함께 간다.
    #[cfg(unix)]
    #[test]
    fn listed_links_go_out_with_their_kind_and_state() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::create_dir(d.join("dir")).unwrap();
        std::fs::write(d.join("file.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("dir"), d.join("dir-link")).unwrap();
        std::os::unix::fs::symlink(d.join("file.txt"), d.join("file-link")).unwrap();
        std::os::unix::fs::symlink(d.join("missing"), d.join("gone")).unwrap();
        let mut entries = crate::core::fs_list::read_dir_entries(d).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        let wire: Vec<(String, bool, Option<String>)> = entries
            .iter()
            .map(list_dir_entry_wire)
            .map(|w| {
                (
                    w["name"].as_str().unwrap().to_owned(),
                    w["is_dir"].as_bool().unwrap(),
                    w.get("link").and_then(|l| l.as_str()).map(str::to_owned),
                )
            })
            .collect();
        let s = |v: &str| Some(v.to_owned());
        assert_eq!(
            wire,
            [
                ("dir".into(), true, None),
                ("dir-link".into(), true, s("valid")),
                ("file-link".into(), false, s("valid")),
                ("file.txt".into(), false, None),
                ("gone".into(), false, s("broken")),
            ]
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
            #[cfg(feature = "gui")]
            os_hidden: true,
            link: crate::core::fs_list::EntryLink::Broken,
            ..entry("desktop.ini")
        };
        assert_eq!(list_dir_entry_wire(&marked)["link"], "broken");
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
