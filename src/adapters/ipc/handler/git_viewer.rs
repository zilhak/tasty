//! `git_viewer.*` IPC — git-viewer plugin이 mirror(attach) workspace에서 원격 git
//! 조회를 트리거하는 진입점(`docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`).
//!
//! `fs.pick_file`(ADR-0042)과 달리 이 호출은 **동기로 결과를 내지 않는다** — 실제
//! 조회가 attach Control 채널 왕복(다른 프로세스·다른 머신일 수 있는 원격 tasty)을
//! 거쳐야 하므로, 이 핸들러는 요청을 `CoreState::pending_git_query_forward` 에 큐잉하고
//! `request_id` 만 즉시 회신한다(accept). 실제 결과는 attach 응답 도착 후
//! `PluginManager::emit_host_event_to_plugin` 으로 `git_viewer.query_result` 이벤트를
//! plugin 에 unicast 한다(`src/app/attach_client.rs::apply_attach_client_output` 의
//! `MirrorEvent::GitQueryResult` 처리, `event.dispatch` 재사용 — `popup.set_context` 는
//! 임의 `context` 필드가 없어 이 용도로 쓸 수 없다).

use serde::Deserialize;
use serde_json::json;

use tasty_ipc::protocol::JsonRpcResponse;

use crate::adapters::production::stream_hub::GitQueryKind;
use crate::core::{CoreState, PendingGitQueryForward};

/// `git_viewer.query { kind, local_surface_id, worktree_path?, diff_path? }` 요청.
#[derive(Deserialize)]
struct GitViewerQueryReq {
    /// `"snapshot"` | `"diff"`.
    kind: String,
    /// popup 이 anchor 된 **로컬** mirror surface id — `popup.open` context 로 받은
    /// 값을 그대로 echo(host 가 attach 세션 매핑으로 원격 id 로 치환한다).
    local_surface_id: u32,
    /// worktree 전환/새로고침 — 이전 `git_viewer.query_result` 가 돌려준 opaque 서버
    /// 경로 echo. 없으면 서버가 `local_surface_id` 의 원격 cwd 로 새로 discover.
    #[serde(default)]
    worktree_path: Option<String>,
    /// `kind = "diff"` 전용 — 대상 파일의 repo-relative 경로.
    #[serde(default)]
    diff_path: Option<String>,
}

/// `git_viewer.query` — 원격 git 조회를 큐잉하고 `request_id` 만 즉시 회신한다
/// (비동기 accept, 위 모듈 doc 참고).
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
