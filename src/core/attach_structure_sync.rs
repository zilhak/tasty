//! 점유 워크스페이스의 구조를 holder 와 맞추는 자리 중 **forward 가 아닌 원인**의 몫.
//!
//! forward 실행 자체는 [`attach_runtime`](crate::core::attach_runtime) 에 있고, 여기는 그
//! 바깥에서 생긴 구조 변경을 기존 `StructuralDelta` 로 holder 에게 보내는 일(ADR-0481)과,
//! forward 가 anchor 를 못 풀었을 때 회신할 사유를 정하는 일(ADR-0482)을 맡는다.

use crate::core::CoreState;
use crate::core::attach::AttachClientId;
use tasty_ipc::stream::{StreamFrame, StreamTag, StructuralOp};
use tasty_ipc::stream_hub::PushResult;

impl CoreState {
    /// 점유 워크스페이스의 구조가 **forward 가 아닌 원인**으로 바뀌었으면 그 holder 에게
    /// 기존 역반영 메시지(`StreamControl::StructuralDelta`, 실행 후 전체 트리)를 보낸다
    /// (ADR-0481). 표시는 `OccupancyRegistry` 가 쌓는다 — 멤버 surface 가 닫혀 잊힐 때와
    /// 로컬 경로로 멤버가 편입될 때. forward 실행은 자기 delta 를 직접 보내고 표시를 지우므로
    /// 여기서 두 번 나가지 않는다.
    ///
    /// 워크스페이스가 통째로 사라졌으면(마지막 surface 의 PTY 가 끝나 cascade 가 워크스페이스를
    /// purge) 보낼 트리가 없다 — forward 경로의 같은 상황과 똑같이 `force_detach_workspace` 로
    /// holder 를 끊고 lock 을 정리한다.
    ///
    /// 부르는 자리: PTY 종료 처리 끝(`app::process_exit::handle`) · 로컬 멤버 편입
    /// (`tap_new_workspace_member`) · 두 빌드의 `StreamReady` 처리 끝(그 밖의 원인이 남긴
    /// 표시를 다음 스트림 활동에서 비운다).
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
            // best-effort — 끊긴 holder 는 다음 배치의 끊김 정리가 lock 을 푼다.
            if let PushResult::Unknown | PushResult::Disconnected = hub.push(holder, frame) {
                tracing::debug!("structure change: holder {holder} of workspace {ws_id} is gone");
            }
        }
    }
}

/// anchor 를 풀 점유 워크스페이스 자체가 없을 때의 forward 회신 사유. 사람에게 보이는
/// UI 문구가 아니라 wire 의 `StructuralResult.reason` 이다(client 가 토스트 끝에 원문 그대로
/// 붙인다) — IPC 거절 문구와 같은 이유로 `t()` 를 거치지 않는다.
fn workspace_not_found_reason() -> String {
    "workspace not found".to_string()
}

/// forward op 의 anchor 가 어느 점유 워크스페이스에서도 안 풀릴 때의 회신 사유.
///
/// 요청한 client 가 이 engine 에서 **아직 살아 있는 워크스페이스를 점유 중이면** 사라진 것은
/// 워크스페이스가 아니라 anchor surface 다(서버에서 PTY 가 끝나 이미 닫힌 surface 를 client 가
/// 늦게 지목한 경우 — 닫힌 surface 는 점유 역매핑에서 먼저 빠진다). 그때는 IPC 가 같은 상황
/// (살아 있지 않은 id 지목)에 싣는 문구를 **같은 생성기로** 쓴다(`no live surface N` 로
/// 시작한다). 요청 이름 자리에는 anchor 를 지목한 것, 즉 이 forward op 의 wire 이름
/// (`structural_op.<kind>`)을 넣는다. 점유 중인 워크스페이스가 없거나 이 engine 에 없으면
/// `None` — 호출자가 종전 문구(`workspace not found`)를 쓴다(ADR-0482).
///
/// anchor 가 어딘가에 **살아 있는지는 여기서 안 본다** — 호출자 [`unresolved_forward_reason`]
/// 이 모든 engine 을 먼저 보고 살아 있으면 이 함수를 부르지 않는다.
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

/// anchor 를 못 푼 forward 의 회신 사유 — 호출자가 가진 engine 들을 차례로 물어
/// [`unresolved_anchor_reason`] 이 처음 답한 것을, 아무도 답하지 않으면 종전 문구를 쓴다.
/// GUI(창 engine 여럿 + parked engine)와 headless(engine 하나)가 같은 판정을 쓰게 한 자리.
///
/// "no live surface" 는 anchor 가 **어디에도** 살아 있지 않을 때만 참이다. 점유 워크스페이스
/// 밖 — 같은 engine 의 다른 워크스페이스든 다른 engine(GUI 의 다른 창)이든 — 에 살아 있으면
/// 그 사유는 거짓이 되므로, 먼저 모든 engine 에서 anchor 를 찾고 어디든 있으면 종전 문구를
/// 쓴다(ADR-0482). engine 하나씩 물으면 점유한 engine 은 다른 engine 의 surface 를 모른다.
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
