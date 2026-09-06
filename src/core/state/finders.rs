//! ID → 컬렉션 entry lookup 헬퍼. surface / pane / tab / terminal / workspace 가
//! 어느 워크스페이스/페인에 속해 있는지 찾아 (&T, &mut T) 또는 인덱스 형태로 반환.

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

    /// Find the tab ID that contains a given surface ID (across all workspaces).
    /// Used by mirror structural-op forwarding to resolve a `CloseTab` from its
    /// anchor surface on the authoritative (remote) instance.
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

    /// 대상 surface 가 mirror(attach 원격 점유) 워크스페이스에 속해 있는지 조회한다.
    /// `apply_explorer_action`(`egui_panels.rs`)의 `OpenFile` mirror 가드가 쓰던 조회를
    /// 재사용 가능한 헬퍼로 뽑아둔 것 — explorer 컨텍스트 메뉴/단축키의 나머지 쓰기
    /// 액션(paste/trash/rename/open_in_system/add_favorite/open_in_new_tab/cut) 가드가
    /// 함께 쓴다. surface 를 못 찾으면 `false`(해당 액션은 대상 자체가 없어 다른
    /// 이유로 이미 no-op).
    pub fn is_mirror_surface(&self, surface_id: u32) -> bool {
        self.find_workspace_index_for_surface(surface_id)
            .and_then(|(idx, _)| self.workspaces.get(idx))
            .map(|ws| ws.mirror)
            .unwrap_or(false)
    }

    /// 구조 변경 `DomainIntent` 의 **대상이 mirror 워크스페이스**에 속하면 그
    /// 워크스페이스 인덱스를 반환한다. mirror 워크스페이스는 원격 워크스페이스의
    /// 뷰(원격 attach client)이므로, 그 안의 구조 변경(split·new-tab·close·이동)은
    /// **로컬에서 실행하면 안 된다** — 로컬 PTY spawn / 로컬 트리 변경은 "workspace
    /// 전체가 remote" 불변식을 깨뜨린다. `Core::apply` 가 이 값이 `Some` 이면 구조
    /// 변경을 거부한다([`super::super::MirrorStructuralBlocked`]). 구조와 무관한
    /// intent 나 대상을 못 찾는 경우 `None`.
    ///
    /// (구조 변경을 원격으로 forward 하는 2단계에서 같은 판별점을 재사용한다.)
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
                // source(떼어내는 쪽) 또는 target(대체되는 쪽) 어느 하나라도 mirror
                // 면 로컬 실행 금지 — 둘 다 검사해 mirror 인 쪽 인덱스를 돌려준다.
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
            _ => return None,
        }?;
        self.workspaces
            .get(ws_idx)
            .filter(|w| w.mirror)
            .map(|_| ws_idx)
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
