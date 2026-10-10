//! `list_dir_result` 의 항목 하나를 wire 값으로 만든다. 받는 쪽은 `attach_client::wire::list_dir` 다.

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
