//! kind별 최근 파일을 읽는다. 전 창이 공유하는 기록을 조회하며 순서나 사용자 상태는 바꾸지 않는다.
//! 파일 자체를 읽지 않으므로 FsRead 대신 SurfaceRead 권한을 사용한다. 헤드리스에서도 제공한다.

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

/// 응답도 캐시와 같은 최신 10개로 제한한다.
const RECENT_LIMIT: usize = 10;

#[derive(Deserialize)]
struct RecentReq {
    kind: String,
}

#[derive(serde::Serialize)]
struct RecentEntry {
    path: String,
    /// 파일명을 얻을 수 없으면 원래 경로를 쓴다.
    file_name: String,
}

fn recent_entries(paths: &[String]) -> Vec<RecentEntry> {
    paths
        .iter()
        .take(RECENT_LIMIT)
        .map(|p| RecentEntry {
            path: p.clone(),
            file_name: std::path::Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone()),
        })
        .collect()
}

/// 공용 창 포트에서 kind별 최근 파일을 받아 최신순으로 반환한다.
pub fn handle_query(
    window: &dyn crate::ipc::window_port::IpcWindow,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: RecentReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    let entries = recent_entries(&window.recent_files(&req.kind));
    JsonRpcResponse::success(id, json!({ "recent": entries }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[test]
    fn recent_query_agrees_across_states_after_either_window_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = crate::db::Db::open(&dir.path().join("state.db")).unwrap();
        let (mut first, _first_engine) = crate::state::tests::test_state();
        let (mut second, _second_engine) = crate::state::tests::test_state();
        first.recent_files = crate::recent_files::RecentFiles::for_db(&mut db);
        second.recent_files = crate::recent_files::RecentFiles::for_db(&mut db);
        first.record_recent("markdown", &json!({"file": "/notes/one.md"}));
        second.record_recent("markdown", &json!({"file": "/notes/two.md"}));
        let query = |state: &AppState| {
            handle_query(state, json!(1), json!({"kind": "markdown"}))
                .result
                .unwrap()
        };
        let expected = json!({"recent": [
            {"path": "/notes/two.md", "file_name": "two.md"},
            {"path": "/notes/one.md", "file_name": "one.md"}
        ]});
        assert_eq!(query(&first), expected);
        assert_eq!(query(&second), expected);
        assert_eq!(query(&first), expected);
    }

    #[test]
    fn recent_entries_preserves_order_and_derives_file_name() {
        #[cfg(windows)]
        let paths = vec![r"E:\notes\a.md".to_string(), r"E:\docs\b.md".to_string()];
        #[cfg(not(windows))]
        let paths = vec!["/notes/a.md".to_string(), "/docs/b.md".to_string()];

        let entries = recent_entries(&paths);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, paths[0]);
        assert_eq!(entries[1].path, paths[1]);
        assert_eq!(entries[0].file_name, "a.md");
        assert_eq!(entries[1].file_name, "b.md");
    }

    #[test]
    fn recent_entries_caps_at_limit() {
        let paths: Vec<String> = (0..25).map(|i| format!("/n/{i}.md")).collect();
        let entries = recent_entries(&paths);
        assert_eq!(entries.len(), RECENT_LIMIT);
        assert_eq!(entries[0].path, "/n/0.md");
    }

    #[test]
    fn recent_entries_empty_is_empty() {
        assert!(recent_entries(&[]).is_empty());
    }

    #[test]
    fn recent_entries_falls_back_to_path_when_no_basename() {
        let paths = vec!["..".to_string()];
        let entries = recent_entries(&paths);
        assert_eq!(entries[0].file_name, "..");
    }

    #[test]
    fn recent_req_requires_kind() {
        let missing = serde_json::from_value::<RecentReq>(json!({}));
        assert!(missing.is_err());
        let ok = serde_json::from_value::<RecentReq>(json!({ "kind": "markdown" })).unwrap();
        assert_eq!(ok.kind, "markdown");
    }

    #[test]
    fn handle_recent_shape_serializes_recent_array() {
        let entries = recent_entries(&["/n/a.md".to_string()]);
        let body = json!({ "recent": entries });
        let arr = body["recent"].as_array().expect("recent is array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "/n/a.md");
        assert_eq!(arr[0]["file_name"], "a.md");
    }
}
