//! `markdown.*` IPC 메서드 — 제자리 이동(in-place navigation, 04) + 최근목록 조회.
//!
//! `markdown.navigate`: 주어진 surface 를 **그 자리에서** 다른 파일의 markdown 으로
//! 교체한다(새 탭 아님). 주소창(03) 플러그인이 자기 surface_id + 새 경로로 호출한다.
//! 확장자 무관하게 markdown 으로 연다. 1MB 초과 & !bypass 면 `01` 크기 확인 팝업을
//! 먼저 띄우고, [열기] 확정 시에만 교체한다.
//!
//! 교체는 `ConvertSurface`(kind="markdown", params={file}) 재사용 — 같은 surface_id.
//! egui-mesh re-bootstrap(stale frame drop)은 `SurfaceConverted` cascade 가 처리한다.
//!
//! `markdown.recent`: 최근 연 markdown 파일 목록(최신순)을 반환한다. 주소창(03)
//! 드롭다운이 소비하는 데이터 공급원. **읽기 전용** — `AppState.recent_files` 캐시를
//! 조회할 뿐 사용자 상태(포커스/선택/히스토리)를 건드리지 않는다(불가침 원칙). 임의
//! 경로 read 가 아니라 이미 열었던 목록 반환뿐이라 `FsRead` 가 아닌 `SurfaceRead` 권한.

use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

#[derive(Deserialize)]
struct NavigateReq {
    /// 교체 대상 surface (플러그인 자신의 surface).
    surface_id: u32,
    /// 이동할 파일의 절대 경로.
    path: String,
    /// true 면 대용량 확인 팝업을 건너뛰고 즉시 교체(강제).
    #[serde(default)]
    ignore_size_limit: bool,
}

/// `markdown.navigate { surface_id, path, ignore_size_limit? }`.
pub fn handle_navigate(
    state: &mut AppState,
    id: serde_json::Value,
    params: serde_json::Value,
) -> JsonRpcResponse {
    let req: NavigateReq = match serde_json::from_value(params) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    let path = std::path::PathBuf::from(&req.path);
    if !path.exists() {
        return JsonRpcResponse::error(id, -32602, format!("path not found: {}", req.path));
    }

    // 대용량 확인 게이트 (01 재사용). 초과 & !bypass 면 교체를 보류하고 확인 팝업.
    if !req.ignore_size_limit
        && let Some(size) = crate::file::dispatch::exceeds_md_size_limit(&path)
    {
        state.dialogs.pending_md_open = Some(crate::state::PendingMdOpen {
            path: req.path.clone(),
            size,
            result: None,
            kind: crate::state::PendingMdOpenKind::InPlace {
                surface_id: req.surface_id,
            },
        });
        state.dispatch_intent(
            crate::intent::UiIntent::OpenPopup {
                id: "markdown_size_confirm",
                mode: crate::intent::OpenPopupMode::WithScope(
                    crate::adapters::ui::popup::PopupScope::Surface(req.surface_id),
                ),
            }
            .from_agent_ipc(),
        );
        return JsonRpcResponse::success(id, json!({ "accepted": true, "pending_confirm": true }));
    }

    // 즉시 제자리 변환.
    navigate_now(state, req.surface_id, &req.path);
    JsonRpcResponse::success(id, json!({ "accepted": true, "pending_confirm": false }))
}

/// 같은 surface 를 markdown + 새 file 로 제자리 변환한다.
pub(crate) fn navigate_now(state: &mut AppState, surface_id: u32, path: &str) {
    state.dispatch_intent(
        crate::intent::Intent::ConvertSurface {
            surface_id,
            target: crate::intent::ConvertTarget::Kind {
                cwd: None,
                kind: "markdown".to_string(),
                params: json!({ "file": path }),
            },
        }
        .from_agent_ipc(),
    );
}

/// recent 목록 상한 — `RecentFiles` 캐시가 이미 최신순 10개로 수렴돼 있으나,
/// IPC 경계에서 계약(최대 10개)을 명시적으로 재보장한다.
const RECENT_LIMIT: usize = 10;

/// `markdown.recent` 응답의 한 항목 — 원본 경로 + 표시용 파일명.
#[derive(serde::Serialize)]
struct RecentEntry {
    /// 최근 연 파일의 원본 경로(표시·이동에 그대로 사용).
    path: String,
    /// 경로에서 파생한 basename. 드롭다운의 라벨 표기용(파생 실패 시 경로 그대로).
    file_name: String,
}

/// 최신순 markdown 경로 목록을 표시용 항목으로 변환한다. **순수 함수** —
/// 입력 순서(최신순)를 보존하고 각 경로의 basename 을 파생하며, 상한만 적용한다.
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

/// `markdown.recent {}` → `{ "recent": [{ path, file_name }] }` (최신순, 최대 10개).
///
/// 필터 없이 `AppState.recent_files.markdown` 캐시를 최신순 그대로 반환한다. 조회만
/// 하므로 `&AppState`(불변) 를 받아 사용자 상태 불변을 타입 수준에서 보장한다.
pub fn handle_recent(state: &AppState, id: serde_json::Value) -> JsonRpcResponse {
    let entries = recent_entries(&state.recent_files.markdown);
    JsonRpcResponse::success(id, json!({ "recent": entries }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigate_req_defaults_bypass_false() {
        let req: NavigateReq =
            serde_json::from_value(json!({ "surface_id": 3, "path": "/a.md" })).unwrap();
        assert_eq!(req.surface_id, 3);
        assert!(!req.ignore_size_limit);
    }

    #[test]
    fn navigate_req_parses_bypass() {
        let req: NavigateReq = serde_json::from_value(
            json!({ "surface_id": 5, "path": "/b.md", "ignore_size_limit": true }),
        )
        .unwrap();
        assert!(req.ignore_size_limit);
    }

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
