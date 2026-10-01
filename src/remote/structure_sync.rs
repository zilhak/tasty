//! forward 외의 구조 변경을 attach holder에 보내고, 찾지 못한 forward 대상의 오류를 정한다.

use crate::runtime::engine_access::EngineRef;
use crate::core::attach::AttachClientId;
use crate::runtime::engine_access::EngineMut;
use tasty_ipc::stream::{StreamFrame, StreamTag, StructuralOp};
use tasty_ipc::stream_hub::PushResult;

/// wire 오류 설명이며 client가 원문을 표시한다. 서버에서 UI 번역을 적용하지 않는다.
fn workspace_not_found_reason() -> String {
    "workspace not found".to_string()
}

/// client가 이 engine의 실제 workspace를 점유하면 공통 surface 오류를 만든다.
/// anchor가 살아 있는지는 호출자가 모든 engine에서 먼저 확인해야 한다.
fn unresolved_anchor_reason(
    engine: &EngineRef<'_>,
    client_id: AttachClientId,
    op: &StructuralOp,
) -> Option<String> {
    let ws = engine.live.occupancy.workspace_held_by(client_id)?;
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
    engines: impl IntoIterator<Item = EngineRef<'a>>,
    client_id: AttachClientId,
    op: &StructuralOp,
) -> String {
    let engines: Vec<EngineRef<'a>> = engines.into_iter().collect();
    let anchor = op.anchor_surface_id();
    if engines
        .iter()
        .any(|e| e.find_workspace_index_for_surface(anchor).is_some())
    {
        return workspace_not_found_reason();
    }
    engines
        .into_iter()
        .find_map(|e| unresolved_anchor_reason(&e, client_id, op))
        .unwrap_or_else(workspace_not_found_reason)
}

impl EngineMut<'_> {
    /// Project transport obligations only after the entire structural batch has been installed.
    /// Sending remains in the ordinary observation pump, behind its publication/result barrier.
    pub(crate) fn observe_committed_structure(&mut self,before:&tasty_core::JournalModel,events:&[tasty_core::RecordedEvent]) {
        use tasty_core::DomainEvent as E;
        let structural=events.iter().any(|record|matches!(record.event,
            E::StructureReplaced {..}|E::WorkspaceCreated {..}|E::WorkspaceRenamed {..}|E::WorkspaceDetailsSet {..}|E::WorkspaceMoved {..}|E::WorkspaceClosed {..}|
            E::PaneSplit {..}|E::PaneMoved {..}|E::PaneClosed {..}|E::TabCreated {..}|E::TabRenamed {..}|E::TabExplicitNameSet {..}|E::TabMoved {..}|E::TabClosed {..}|
            E::SurfaceSplit {..}|E::SurfaceMoved {..}|E::SurfaceClosed {..}|E::SurfaceConverted {..}|E::SurfaceActivationChanged {..}|E::PaneRatioSet {..}|E::SurfaceRatioSet {..}));
        if !structural {return;}
        let workspaces:std::collections::BTreeSet<_>=before.workspace_order.iter().copied().chain(self.core.local_workspaces().iter().map(|workspace|workspace.id)).collect();
        for workspace in workspaces {if self.live.occupancy.workspace_holder(workspace).is_some() {self.remote.mark_structure_changed(workspace);}}
        for record in events {
            let E::SurfaceActivationChanged {id,activation,..}=&record.event else {continue;};
            if activation.phase!=tasty_core::ActivationPhase::Ready {continue;}
            let Some(workspace)=self.find_workspace_index_for_surface(*id).and_then(|(index,_)|self.workspace_at(index)).map(|workspace|workspace.id) else {continue;};
            if self.live.occupancy.workspace_holder(workspace).is_none() {continue;}
            let generation=self.runtime.terminals.generation(*id);
            self.live.occupancy.add_workspace_member(workspace,*id,generation.is_some());
            if let Some(generation)=generation {self.remote.pending_workspace_taps.insert(*id,(workspace,generation));}
        }
    }

    pub(crate) fn flush_committed_workspace_taps(&mut self,workspace:u32,holder:u32,hub:&tasty_ipc::stream_hub::StreamHub) {
        let targets:Vec<_>=self.remote.pending_workspace_taps.iter().filter(|(_,value)|value.0==workspace).map(|(id,value)|(*id,value.1)).collect();
        for (surface,generation) in targets {
            self.remote.pending_workspace_taps.remove(&surface);
            if self.live.occupancy.workspace_holder(workspace)==Some(holder) && self.runtime.terminals.matches_generation(surface,generation) && self.find_workspace_index_for_surface(surface).and_then(|(index,_)|self.workspace_at(index)).is_some_and(|value|value.id==workspace) {
                self.tap_surface_for_stream(surface,holder,hub);
            }
        }
    }

    /// 변경 표시가 있는 workspace의 전체 트리를 holder에 보낸다. 사라진 workspace는 강제 분리한다.
    /// forward 실행은 자체 delta를 보내고 표시를 지워 중복 통지를 피한다.
    /// notifier가 없거나 송신에 실패해도 여기서는 변경 표시를 다시 쌓지 않는다.
    pub(crate) fn push_structure_changes(&mut self) {
        for ws_id in self.remote.take_structure_changed() {
            let Some(holder) = self.live.occupancy.workspace_holder(ws_id) else {
                continue;
            };
            let Some(idx) = self.find_workspace_index_for_id(ws_id) else {
                self.force_detach_workspace(ws_id);
                continue;
            };
            let Some(hub) = self.remote.notifier() else {
                continue;
            };
            let class = self.classify_attach_surfaces(self.workspace_at(idx).expect("workspace index is valid").id);
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
            match hub.push(holder,frame) {
                PushResult::Sent=>self.flush_committed_workspace_taps(ws_id,holder,&hub),
                PushResult::Dropped=>self.remote.mark_structure_changed(ws_id),
                PushResult::Unknown|PushResult::Disconnected=> {
                    self.remote.pending_workspace_taps.retain(|_,value|value.0!=ws_id);
                    tracing::debug!("structure change: holder {holder} of workspace {ws_id} is gone");
                },
            }
        }
    }
}
