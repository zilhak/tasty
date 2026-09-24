//! forward 외의 구조 변경을 attach holder에 보내고, 찾지 못한 forward 대상의 오류를 정한다.

use crate::core::CoreState;
use crate::core::attach::AttachClientId;
use tasty_ipc::stream::{StreamFrame, StreamTag, StructuralOp};
use tasty_ipc::stream_hub::PushResult;

impl CoreState {
    /// 변경 표시가 있는 workspace의 전체 트리를 holder에 보낸다. 사라진 workspace는 강제 분리한다.
    /// forward 실행은 자체 delta를 보내고 표시를 지워 중복 통지를 피한다.
    /// notifier가 없거나 송신에 실패해도 여기서는 변경 표시를 다시 쌓지 않는다.
    pub(crate) fn push_structure_changes(&mut self) {
        for ws_id in self.attach.take_structure_changed() {
            let Some(holder) = self.attach.workspace_holder(ws_id) else {
                continue;
            };
            let Some(idx) = self.find_workspace_index_for_id(ws_id) else {
                self.attach.force_detach_workspace(ws_id);
                continue;
            };
            let Some(hub) = self.attach.notifier() else {
                continue;
            };
            let class = self.workspaces[idx].classify_attach_surfaces();
            let (tree, surfaces) = self.build_workspace_tree_surfaces(idx, &class);
            let delta = tasty_ipc::stream::StreamControl::StructuralDelta {
                workspace_id: ws_id,
                tree,
                surfaces,
            };
            let frame = StreamFrame::new(
                StreamTag::Control,
                serde_json::to_vec(&delta).unwrap_or_default(),
            );
            if let PushResult::Unknown | PushResult::Disconnected = hub.push(holder, frame) {
                tracing::debug!("structure change: holder {holder} of workspace {ws_id} is gone");
            }
        }
    }
}

/// wire 오류 설명이며 client가 원문을 표시한다. 서버에서 UI 번역을 적용하지 않는다.
fn workspace_not_found_reason() -> String {
    "workspace not found".to_string()
}

/// client가 이 engine의 실제 workspace를 점유하면 공통 surface 오류를 만든다.
/// anchor가 살아 있는지는 호출자가 모든 engine에서 먼저 확인해야 한다.
fn unresolved_anchor_reason(
    engine: &CoreState,
    client_id: AttachClientId,
    op: &StructuralOp,
) -> Option<String> {
    let ws = engine.attach.workspace_held_by(client_id)?;
    engine.find_workspace_index_for_id(ws)?;
    Some(crate::core::request_target::unowned_target_message(
        crate::core::request_target::ResourceId {
            kind: crate::core::request_target::Kind::Surface,
            id: u64::from(op.anchor_surface_id()),
        },
        &format!("structural_op.{}", op.wire_kind()),
    ))
}

/// 모든 engine에서 anchor를 먼저 찾는다. 하나라도 살아 있으면 surface가 사라졌다고 안내하지 않는다.
/// 어디에도 없고 점유한 workspace가 남아 있으면 surface 오류, 그 외에는 workspace 오류다.
pub(crate) fn unresolved_forward_reason<'a>(
    engines: impl IntoIterator<Item = &'a CoreState>,
    client_id: AttachClientId,
    op: &StructuralOp,
) -> String {
    let engines: Vec<&CoreState> = engines.into_iter().collect();
    let anchor = op.anchor_surface_id();
    if engines
        .iter()
        .any(|e| e.find_workspace_index_for_surface(anchor).is_some())
    {
        return workspace_not_found_reason();
    }
    engines
        .into_iter()
        .find_map(|e| unresolved_anchor_reason(e, client_id, op))
        .unwrap_or_else(workspace_not_found_reason)
}
