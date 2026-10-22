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
    /// 링크 상태(`"valid"`·`"broken"`). 옛 server 는 싣지 않고, 모르는 값과 함께 링크 아님으로 읽는다.
    #[serde(default)]
    link: Option<String>,
}

fn remote_link(wire: Option<&str>) -> tasty_remote::client_session::RemoteLink {
    use tasty_remote::client_session::RemoteLink;
    match wire {
        Some("valid") => RemoteLink::Valid,
        Some("broken") => RemoteLink::Broken,
        _ => RemoteLink::NotALink,
    }
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
    #[serde(default)]
    volume: Option<u64>,
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
                link: remote_link(e.link.as_deref()),
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
        volume: wire.volume,
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

    fn links(payload: serde_json::Value) -> Vec<tasty_remote::client_session::RemoteLink> {
        let bytes = serde_json::to_vec(&payload).expect("encode");
        match parse_list_dir_result(&bytes) {
            Some(MirrorEvent::ListDirResult {
                ok: true,
                entries: Some(entries),
                ..
            }) => entries.iter().map(|e| e.link).collect(),
            _ => panic!("not a successful list_dir_result"),
        }
    }

    #[test]
    fn a_new_server_link_state_is_read() {
        use tasty_remote::client_session::RemoteLink;
        let read = links(serde_json::json!({
            "event": "list_dir_result", "request_id": 1, "ok": true, "dir": "/srv",
            "entries": [
                {"name": "docs", "is_dir": true, "size": 0, "link": "valid"},
                {"name": "notes.md", "is_dir": false, "size": 3, "link": "valid"},
                {"name": "gone", "is_dir": false, "size": 0, "link": "broken"},
                {"name": "plain.txt", "is_dir": false, "size": 1},
            ],
        }));
        assert_eq!(
            read,
            [
                RemoteLink::Valid,
                RemoteLink::Valid,
                RemoteLink::Broken,
                RemoteLink::NotALink
            ]
        );
    }

    /// 옛 server 는 링크 상태를 싣지 않는다. 오류 없이 링크 아님으로 읽는다. 앞으로 생길 모르는 값도 같다.
    #[test]
    fn an_old_server_or_an_unknown_link_value_reads_as_not_a_link() {
        use tasty_remote::client_session::RemoteLink;
        let read = links(serde_json::json!({
            "event": "list_dir_result", "request_id": 1, "ok": true, "dir": "/srv",
            "entries": [
                {"name": "gone", "is_dir": false, "size": 0},
                {"name": "odd", "is_dir": false, "size": 0, "link": "junction"},
            ],
        }));
        assert_eq!(read, [RemoteLink::NotALink, RemoteLink::NotALink]);
    }

    /// 새 server 는 폴더의 디스크 식별값을 싣는다. 옛 server 의 응답에는 없다.
    #[test]
    fn the_listing_volume_is_read_when_present() {
        let volume = |payload: serde_json::Value| match parse_list_dir_result(
            &serde_json::to_vec(&payload).unwrap(),
        ) {
            Some(MirrorEvent::ListDirResult { volume, .. }) => volume,
            _ => panic!("not a list_dir_result"),
        };
        let base = serde_json::json!({"event": "list_dir_result", "request_id": 1, "ok": true, "entries": []});
        assert_eq!(volume(base.clone()), None);
        let mut new = base;
        new["volume"] = 42.into();
        assert_eq!(volume(new), Some(42));
    }
}
