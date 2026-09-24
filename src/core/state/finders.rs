//! 포커스와 무관하게 모든 workspace에서 ID의 소속과 객체를 찾는다.

use super::CoreState;

impl CoreState {
    pub fn find_surface_by_id(&self, surface_id: u32) -> Option<&dyn crate::model::Surface> {
        for workspace in &self.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if !tab.contains_surface(surface_id) {
                            continue;
                        }
                        if let Some(s) = tab.layout().find_surface(surface_id) {
                            return Some(s);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn live_surface_ids(&self) -> std::collections::HashSet<u32> {
        let mut ids = std::collections::HashSet::new();
        for workspace in &self.workspaces {
            for sid in workspace.all_surface_ids() {
                ids.insert(sid);
            }
        }
        ids
    }

    /// 현재 트리에 없는 자식 등록을 정리한다. 변경이 있으면 저장을 시도한다.
    pub fn reconcile_child_terminals(&mut self) {
        let live = self.live_surface_ids();
        let summary = self.child_terminals.reconcile_with_live_surfaces(&live);
        if summary.changed() {
            self.child_terminals.save();
        }
    }

    pub fn find_pane_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.workspaces {
            let pane_ids = workspace.pane_layout().all_pane_ids();
            for pid in pane_ids {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some(pid);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn find_tab_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some(tab.id);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn find_workspace_index_for_pane(&self, pane_id: u32) -> Option<usize> {
        for (i, workspace) in self.workspaces.iter().enumerate() {
            if workspace.pane_layout().find_pane(pane_id).is_some() {
                return Some(i);
            }
        }
        None
    }

    pub fn find_pane_by_id(&self, pane_id: u32) -> Option<&crate::model::Pane> {
        for workspace in &self.workspaces {
            if let Some(pane) = workspace.pane_layout().find_pane(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    pub fn find_pane_for_tab(&self, tab_id: u32) -> Option<u32> {
        for workspace in &self.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid)
                    && pane.tabs.iter().any(|t| t.id == tab_id)
                {
                    return Some(pid);
                }
            }
        }
        None
    }

    pub fn find_pane_by_id_mut(&mut self, pane_id: u32) -> Option<&mut crate::model::Pane> {
        for workspace in &mut self.workspaces {
            if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    pub fn find_workspace_index_for_surface(&self, surface_id: u32) -> Option<(usize, u32)> {
        for (i, workspace) in self.workspaces.iter().enumerate() {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some((i, pid));
                        }
                    }
                }
            }
        }
        None
    }

    pub fn find_workspace_index_for_id(&self, ws_id: u32) -> Option<usize> {
        self.workspaces.iter().position(|w| w.id == ws_id)
    }

    /// surface가 mirror workspace에 속하는지 확인한다. ID를 못 찾으면 false다.
    pub fn is_mirror_surface(&self, surface_id: u32) -> bool {
        self.find_workspace_index_for_surface(surface_id)
            .and_then(|(idx, _)| self.workspaces.get(idx))
            .map(|ws| ws.mirror)
            .unwrap_or(false)
    }

    /// 로컬 구조 변경을 막을 mirror workspace를 찾는다. 비구조 요청이나 없는 대상은 None이다.
    pub(crate) fn mirror_workspace_index_for_structural(
        &self,
        intent: &crate::core::intent::DomainIntent,
    ) -> Option<usize> {
        use crate::core::intent::DomainIntent as D;
        let ws_idx = match intent {
            D::SplitSurface {
                target_surface_id: sid,
                ..
            }
            | D::CloseSurface {
                surface_id: sid, ..
            }
            | D::ConvertSurface {
                surface_id: sid, ..
            } => self.find_workspace_index_for_surface(*sid).map(|(i, _)| i),
            D::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => {
                // 이동·교체는 양쪽 중 하나라도 mirror이면 로컬에서 실행하지 않는다.
                self.find_workspace_index_for_surface(*source_surface_id)
                    .map(|(i, _)| i)
                    .filter(|&i| self.workspaces.get(i).is_some_and(|w| w.mirror))
                    .or_else(|| {
                        self.find_workspace_index_for_surface(*target_surface_id)
                            .map(|(i, _)| i)
                    })
            }
            D::SplitPane {
                target_pane_id: pid,
                ..
            }
            | D::CreateTab { pane_id: pid, .. }
            | D::ClosePane { pane_id: pid }
            | D::MoveTab { pane_id: pid, .. } => self.find_workspace_index_for_pane(*pid),
            D::CloseTab { tab_id } => self
                .find_pane_for_tab(*tab_id)
                .and_then(|pid| self.find_workspace_index_for_pane(pid)),
            D::RestoreClosedItem { target_pane_id, .. } => {
                self.find_workspace_index_for_pane((*target_pane_id)?)
            }
            _ => return None,
        }?;
        self.workspaces
            .get(ws_idx)
            .filter(|w| w.mirror)
            .map(|_| ws_idx)
    }

    /// 카테고리 안의 workspace와 전역 인덱스를 함께 반환한다. 인덱스는 카테고리 내부 순번이 아니다.
    pub fn workspaces_in_category(
        &self,
        category: crate::model::WorkspaceCategoryId,
    ) -> Vec<(usize, &crate::model::Workspace)> {
        self.workspaces
            .iter()
            .enumerate()
            .filter(|(_, w)| w.category == category)
            .collect()
    }

    pub fn category_index(&self, category_id: crate::model::WorkspaceCategoryId) -> Option<usize> {
        self.categories.iter().position(|c| c.id == category_id)
    }

    /// surface의 workspace 이름과 탭 표시 이름. 트리에 없으면 None이다.
    #[cfg(any(feature = "gui", test))]
    pub fn surface_display_path(&self, surface_id: u32) -> Option<SurfaceDisplayPath> {
        for workspace in &self.workspaces {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some(SurfaceDisplayPath {
                                workspace_name: workspace.name.clone(),
                                tab_name: Some(tab.display_name()),
                            });
                        }
                    }
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
#[cfg(any(feature = "gui", test))]
pub struct SurfaceDisplayPath {
    pub workspace_name: String,
    pub tab_name: Option<String>,
}
