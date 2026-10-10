//! `list_dir_result` 를 `MirrorEvent::ListDirResult` 로 읽는다. 보내는 쪽은 server 의
//! `content_queries::list_dir_wire` 다.

use serde_json::Value;
use tasty_remote::client_session::MirrorEvent;

/// modified_unix는 epoch 초이며 DirEntryInfo의 SystemTime으로 변환한다.
#[derive(serde::Deserialize)]
struct ListDirEntryWire {
    name: String,
    is_dir: bool,
    size: u64,
    #[serde(default)]
    modified_unix: Option<u64>,
    #[serde(default)]
    ext: String,
    /// OS 숨김 표시. 이 필드를 싣지 않는 옛 server 의 응답은 거짓으로 읽어 점 규칙만 쓴다.
    #[serde(default)]
    os_hidden: bool,
}

#[derive(serde::Deserialize)]
struct ListDirResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    entries: Option<Vec<ListDirEntryWire>>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

pub(super) fn parse_list_dir_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("list_dir_result") {
        return None;
    }
    let wire: ListDirResultWire = serde_json::from_value(value).ok()?;
    let entries = wire.entries.map(|es| {
        es.into_iter()
            .map(|e| tasty_remote::client_session::RemoteDirEntry {
                name: e.name,
                is_dir: e.is_dir,
                size: e.size,
                modified: e
                    .modified_unix
                    .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
                ext: e.ext,
                os_hidden: e.os_hidden,
            })
            .collect()
    });
    Some(MirrorEvent::ListDirResult {
        request_id: wire.request_id,
        ok: wire.ok,
        dir: wire.dir,
        entries,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden_marks(payload: serde_json::Value) -> Vec<bool> {
        let bytes = serde_json::to_vec(&payload).expect("encode");
        match parse_list_dir_result(&bytes) {
            Some(MirrorEvent::ListDirResult {
                ok: true,
                entries: Some(entries),
                ..
            }) => entries.iter().map(|e| e.os_hidden).collect(),
            _ => panic!("not a successful list_dir_result"),
        }
    }

    #[test]
    fn a_new_server_mark_is_read_as_os_hidden() {
        let marks = hidden_marks(serde_json::json!({
            "event": "list_dir_result", "request_id": 1, "ok": true, "dir": "C:/Users/me",
            "entries": [
                {"name": "desktop.ini", "is_dir": false, "size": 0, "modified_unix": null, "ext": "ini", "os_hidden": true},
                {"name": "notes.txt", "is_dir": false, "size": 3, "modified_unix": 1, "ext": "txt"},
            ],
        }));
        assert_eq!(marks, [true, false]);
    }

    /// 옛 server 는 `os_hidden` 을 싣지 않는다. 오류 없이 거짓으로 읽어 점 규칙만 남는다.
    #[test]
    fn an_old_server_listing_reads_without_the_mark() {
        let marks = hidden_marks(serde_json::json!({
            "event": "list_dir_result", "request_id": 1, "ok": true, "dir": "/home/me",
            "entries": [
                {"name": ".env", "is_dir": false, "size": 0, "modified_unix": null, "ext": ""},
                {"name": "a.txt", "is_dir": false, "size": 0},
            ],
        }));
        assert_eq!(marks, [false, false]);
    }
}
