//! ID → 컬렉션 entry lookup 헬퍼. surface / pane / tab / terminal / workspace 가
//! 어느 워크스페이스/패인에 속해 있는지 찾아 (&T, &mut T) 또는 인덱스 형태로 반환.

use super::CoreState;

impl CoreState {
    /// Find a surface (any type) by ID across all workspaces.
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

    /// 전 워크스페이스의 살아있는 surface id 집합. child-terminal registry self-heal
    /// (`reconcile_child_terminals`) 이 stale 판정에 쓴다 — 포커스 독립(전 워크스페이스
    /// 순회). parent surface 도 자식 surface 도 이 집합 기준으로 대조된다.
    pub fn live_surface_ids(&self) -> std::collections::HashSet<u32> {
        let mut ids = std::collections::HashSet::new();
        for workspace in &self.workspaces {
            for sid in workspace.all_surface_ids() {
                ids.insert(sid);
            }
        }
        ids
    }

    /// child-terminal registry 를 라이브 surface 트리와 대조해 stale 항목을 정리한다.
    /// 호스트는 라이브 트리를 직접 소유하므로 이벤트 구독 없이 접근 시점마다 동기
    /// reconcile 로 self-heal 한다(부팅 후 첫 접근이 이전 세션 잔재를 회수, surface
    /// 닫힘도 다음 접근에서 정리). 실제 제거가 있었을 때만 디스크 save.
    pub fn reconcile_child_terminals(&mut self) {
        let live = self.live_surface_ids();
        let summary = self.child_terminals.reconcile_with_live_surfaces(&live);
        if summary.changed() {
            self.child_terminals.save();
        }
    }

    /// Find the pane ID that contains a given surface ID.
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

    /// Find the workspace index containing a given pane ID.
    pub fn find_workspace_index_for_pane(&self, pane_id: u32) -> Option<usize> {
        for (i, workspace) in self.workspaces.iter().enumerate() {
            if workspace.pane_layout().find_pane(pane_id).is_some() {
                return Some(i);
            }
        }
        None
    }

    /// Find a pane by ID across all workspaces (immutable).
    pub fn find_pane_by_id(&self, pane_id: u32) -> Option<&crate::model::Pane> {
        for workspace in &self.workspaces {
            if let Some(pane) = workspace.pane_layout().find_pane(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    /// Find the pane ID containing a given tab ID.
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

    /// Find a pane by ID across all workspaces (mutable).
    pub fn find_pane_by_id_mut(&mut self, pane_id: u32) -> Option<&mut crate::model::Pane> {
        for workspace in &mut self.workspaces {
            if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    /// Find the workspace index and pane ID containing a given surface ID.
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

    /// Find a workspace index by its workspace ID.
    pub fn find_workspace_index_for_id(&self, ws_id: u32) -> Option<usize> {
        self.workspaces.iter().position(|w| w.id == ws_id)
    }

    /// 주어진 카테고리에 속한 워크스페이스들을 **전역 인덱스 동반** 으로 반환.
    /// 전역 인덱스 = `self.workspaces` 의 0-based 위치 — 카테고리-로컬 단축키/
    /// 사이드바 매핑을 기존 전역 `switch_workspace` 로 변환할 때 필수.
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

    /// 카테고리 id → `categories` Vec 내 인덱스(섹션 표시 순서).
    pub fn category_index(&self, category_id: crate::model::WorkspaceCategoryId) -> Option<usize> {
        self.categories.iter().position(|c| c.id == category_id)
    }

    /// Resolve a surface to its display path (workspace name + tab display name).
    /// Returns `None` if the surface does not belong to any workspace.
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

/// Display-friendly path for a surface: the workspace it lives in, plus the
/// tab name when known. Used by UI surfaces that label cross-workspace data
/// (e.g. the port scanner popup) by human-readable name rather than ID.
#[derive(Clone, Debug)]
pub struct SurfaceDisplayPath {
    pub workspace_name: String,
    pub tab_name: Option<String>,
}
