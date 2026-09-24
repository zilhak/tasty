//! mirror workspace의 원격 git 조회. request_id를 즉시 반환하고
//! attach 응답이 오면 git_viewer.query_result 이벤트를 요청 플러그인에게만 보낸다(ADR-0022).

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use crate::core::{CoreState, PendingGitQueryForward};
use tasty_ipc::stream_hub::GitQueryKind;

/// `git_viewer.query { kind, local_surface_id, worktree_path?, diff_path? }` 요청.
#[derive(Deserialize)]
struct GitViewerQueryReq {
    /// `"snapshot"` | `"diff"`.
    kind: String,
    /// 팝업을 연 로컬 mirror surface ID. 호스트가 원격 ID로 바꿔 전달한다.
    local_surface_id: u32,
    /// 이전 응답에서 받은 서버 경로. 없으면 원격 surface의 cwd에서 저장소를 찾는다.
    #[serde(default)]
    worktree_path: Option<String>,
    /// `kind = "diff"` 전용 — 대상 파일의 repo-relative 경로.
    #[serde(default)]
    diff_path: Option<String>,
}

pub fn handle_query(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let req: GitViewerQueryReq = match serde_json::from_value(params.clone()) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::error(id, -32602, format!("invalid params: {e}")),
    };
    let kind = match req.kind.as_str() {
        "snapshot" => GitQueryKind::Snapshot,
        "diff" => GitQueryKind::Diff,
        other => {
            return JsonRpcResponse::error(
                id,
                -32602,
                format!("invalid params: unknown kind '{other}'"),
            );
        }
    };
    let request_id = crate::core::next_git_query_request_id();
    engine
        .pending_git_query_forward
        .push(PendingGitQueryForward {
            local_surface_id: req.local_surface_id,
            request_id,
            kind,
            worktree_path: req.worktree_path,
            diff_path: req.diff_path,
        });
    JsonRpcResponse::success(id, json!({ "request_id": request_id }))
}
