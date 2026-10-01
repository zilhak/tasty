use crate::runtime::engine_access::{EngineMut, EngineRef};
impl EngineRef<'_> {
    /// 로컬 구조 변경을 막을 mirror workspace를 찾는다. 비구조 요청이나 없는 대상은 None이다.
    pub(crate) fn mirror_workspace_index_for_structural(
        &self,
        intent: &crate::app::command::DomainIntent,
    ) -> Option<usize> {
        use crate::app::command::DomainIntent as D;
        let ws_idx = match intent {
            D::CloseWorkspace {workspace_id}=>self.find_workspace_index_for_id(*workspace_id),
            D::RetireExitedSurface {surface_id,..}=>self.find_workspace_index_for_surface(*surface_id).map(|(index,_)|index),
            D::SplitSurface {
                target_surface_id: sid,
                ..
            }
            | D::CloseSurface {
                surface_id: sid, ..
            }
            | D::ConvertSurface {
                surface_id: sid, ..
            }
            // 원격 터미널 자리를 로컬 PTY로 바꾸므로 forward하지 않고 막는다.
            | D::RespawnTerminal {
                surface_id: sid, ..
            } => self.find_workspace_index_for_surface(*sid).map(|(i, _)| i),
            D::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => {
                // 이동·교체는 양쪽 중 하나라도 mirror이면 로컬에서 실행하지 않는다.
                self.find_workspace_index_for_surface(*source_surface_id)
                    .map(|(i, _)| i)
                    .filter(|&i| self.workspace_at(i).is_some_and(|w| w.mirror))
                    .or_else(|| {
                        self.find_workspace_index_for_surface(*target_surface_id)
                            .map(|(i, _)| i)
                    })
            }
            D::ReplaceTabWithTab {
                source_tab_id,
                target_tab_id,
            } => {
                // 탭 이동도 양쪽 중 하나라도 mirror이면 로컬에서 실행하지 않는다.
                let ws_of_tab = |tab_id: u32| {
                    self.find_pane_for_tab(tab_id)
                        .and_then(|pid| self.find_workspace_index_for_pane(pid))
                };
                ws_of_tab(*source_tab_id)
                    .filter(|&i| self.workspace_at(i).is_some_and(|w| w.mirror))
                    .or_else(|| ws_of_tab(*target_tab_id))
            }
            D::ReplacePaneWithPane {
                source_pane_id,
                target_pane_id,
            } => {
                // 페인 이동도 양쪽 중 하나라도 mirror이면 로컬에서 실행하지 않는다.
                self.find_workspace_index_for_pane(*source_pane_id)
                    .filter(|&i| self.workspace_at(i).is_some_and(|w| w.mirror))
                    .or_else(|| self.find_workspace_index_for_pane(*target_pane_id))
            }
            D::SplitPane {
                target_pane_id: pid,
                ..
            }
            | D::CreateTab { pane_id: pid, .. }
            // 입양한 로컬 PTY 탭은 원격 트리에 없다.
            | D::AdoptTerminal { pane_id: pid, .. }
            | D::ClosePane { pane_id: pid }
            | D::MoveTab { pane_id: pid, .. } => self.find_workspace_index_for_pane(*pid),
            D::CloseTab { tab_id } => self
                .find_pane_for_tab(*tab_id)
                .and_then(|pid| self.find_workspace_index_for_pane(pid)),
            D::RestoreClosedItem { target_pane_id, .. } => {
                self.find_workspace_index_for_pane((*target_pane_id)?)
            }
            // 이름·카테고리·attach 매핑은 원격 트리 구조가 아니다. mirror에서도 로컬에만 적용하며 forward하지 않는다.
             D::SetWorkspaceCategory { .. }
            | D::SetWorkspaceAttachMapping { .. }
            | D::CreateCategory { .. }
            | D::RenameCategory { .. }
            | D::DeleteCategory { .. }
            | D::ReorderCategory { .. }
            => return None,
            _ => return None,
        }?;
        self.workspace_at(ws_idx)
            .filter(|w| w.mirror)
            .map(|_| ws_idx)
    }
}
impl EngineMut<'_> {
    /// 현재 트리에 없는 자식 등록을 정리한다. 변경이 있으면 저장을 시도한다.
    pub fn reconcile_child_terminals(&mut self) {
        let live = self.live_surface_ids();
        let summary = self
            .runtime
            .child_terminals
            .reconcile_with_live_surfaces(&live);
        if summary.changed() {
            self.runtime.child_terminals.save();
        }
    }
}
