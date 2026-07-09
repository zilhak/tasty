//! `recent.query {kind}` IPC — generic per-kind 최근 파일 목록 조회.
//!
//! host 는 특정 surface_kind 이름을 모른다. plugin(예: 주소창 드롭다운)이 자기 kind 를
//! 넘겨 최근 목록을 조회한다. **읽기 전용** — `AppState.recent_files` 캐시를 조회할 뿐
//! 사용자 상태(포커스/선택/히스토리)를 건드리지 않는다(불가침 원칙). 임의 경로 read 가
//! 아니라 이미 열었던 목록 반환뿐이라 `FsRead` 가 아닌 `SurfaceRead` 권한.
//!
//! 순수 데이터 조회라 gui feature 없이도 동작한다(headless 포함).

use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// recent 목록 상한 — `RecentFiles` 캐시가 이미 kind 별 최신순 10개로 수렴돼 있으나,
/// IPC 경계에서 계약(최대 10개)을 명시적으로 재보장한다.
const RECENT_LIMIT: usize = 10;

#[derive(Deserialize)]
struct RecentReq {
    /// 조회할 surface_kind (예: "markdown"). caller 가 채운다 — host 는 kind 를 모른다.
    kind: String,
}

/// `recent.query` 응답의 한 항목 — 원본 경로 + 표시용 파일명.
#[derive(serde::Serialize)]
struct RecentEntry {
    /// 최근 연 파일의 원본 경로(표시·이동에 그대로 사용).
    path: String,
    /// 경로에서 파생한 basename. 드롭다운의 라벨 표기용(파생 실패 시 경로 그대로).
    file_name: String,
}

/// 최신순 경로 목록을 표시용 항목으로 변환한다. **순수 함수** — 입력 순서(최신순)를
/// 보존하고 각 경로의 basename 을 파생하며, 상한만 적용한다.
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

/// `recent.query { kind }` → `{ "recent": [{ path, file_name }] }` (최신순, 최대 10개).
///
/// 필터 없이 `AppState.recent_files.get(kind)` 캐시를 최신순 그대로 반환한다. 조회만
/// 하므로 `&AppState`(불변) 를 받아 사용자 상태 불변을 타입 수준에서 보장한다.
pub fn handle_query(
    state: &AppState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: RecentReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    let entries = recent_entries(state.recent_files.get(&req.kind));
    JsonRpcResponse::success(id, json!({ "recent": entries }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_entries_preserves_order_and_derives_file_name() {
        #[cfg(windows)]
        let paths = vec![r"E:\notes\a.md".to_string(), r"E:\docs\b.md".to_string()];
        #[cfg(not(windows))]
        let paths = vec!["/notes/a.md".to_string(), "/docs/b.md".to_string()];

        let entries = recent_entries(&paths);
        assert_eq!(entries.len(), 2);
        // 최신순(입력 순서) 보존.
        assert_eq!(entries[0].path, paths[0]);
        assert_eq!(entries[1].path, paths[1]);
        // basename 파생.
        assert_eq!(entries[0].file_name, "a.md");
        assert_eq!(entries[1].file_name, "b.md");
    }

    #[test]
    fn recent_entries_caps_at_limit() {
        let paths: Vec<String> = (0..25).map(|i| format!("/n/{i}.md")).collect();
        let entries = recent_entries(&paths);
        assert_eq!(entries.len(), RECENT_LIMIT);
        // 최신순 상위 RECENT_LIMIT 개만 — 첫 항목 보존.
        assert_eq!(entries[0].path, "/n/0.md");
    }

    #[test]
    fn recent_entries_empty_is_empty() {
        assert!(recent_entries(&[]).is_empty());
    }

    #[test]
    fn recent_entries_falls_back_to_path_when_no_basename() {
        // 파일명 파생이 불가능한 입력은 경로 그대로를 라벨로 쓴다.
        let paths = vec!["..".to_string()];
        let entries = recent_entries(&paths);
        assert_eq!(entries[0].file_name, "..");
    }

    #[test]
    fn recent_req_requires_kind() {
        // kind 누락 시 파싱 실패(핸들러가 -32602 로 응답).
        let missing = serde_json::from_value::<RecentReq>(json!({}));
        assert!(missing.is_err());
        let ok = serde_json::from_value::<RecentReq>(json!({ "kind": "markdown" })).unwrap();
        assert_eq!(ok.kind, "markdown");
    }

    #[test]
    fn handle_recent_shape_serializes_recent_array() {
        // 핸들러 응답이 `{ "recent": [...] }` 형태로 직렬화되는지 확인(순수 shape).
        let entries = recent_entries(&["/n/a.md".to_string()]);
        let body = json!({ "recent": entries });
        let arr = body["recent"].as_array().expect("recent is array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["path"], "/n/a.md");
        assert_eq!(arr[0]["file_name"], "a.md");
    }
}
