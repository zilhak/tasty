//! 포커스와 무관하게 모든 workspace에서 ID의 소속과 객체를 찾는다.

use crate::CoreState;

impl CoreState {
    pub fn find_surface_by_id(&self, surface_id: u32) -> Option<&tasty_model::SurfaceDescriptor> {
        for workspace in &self.workspaces() {
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

    pub(crate) fn find_surface_descriptor_mut(&mut self,id:u32)->Option<&mut tasty_model::SurfaceDescriptor> {
        let pane=self.find_pane_for_surface(id)?;
        self.find_pane_by_id_mut(pane)?.tabs.iter_mut().find_map(|tab|tab.layout_opt.as_mut().and_then(|layout|layout.find_leaf_mut(id)))
    }

    pub fn live_surface_ids(&self) -> std::collections::HashSet<u32> {
        let mut ids = std::collections::HashSet::new();
        for workspace in &self.workspaces() {
            for sid in workspace.all_surface_ids() {
                ids.insert(sid);
            }
        }
        ids
    }

    pub fn find_pane_for_surface(&self, surface_id: u32) -> Option<u32> {
        for workspace in &self.workspaces() {
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
        for workspace in &self.workspaces() {
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
        for (i, workspace) in self.workspaces().into_iter().enumerate() {
            if workspace.pane_layout().find_pane(pane_id).is_some() {
                return Some(i);
            }
        }
        None
    }

    pub fn find_pane_by_id(&self, pane_id: u32) -> Option<&tasty_model::Pane> {
        for workspace in &self.workspaces() {
            if let Some(pane) = workspace.pane_layout().find_pane(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    pub fn find_pane_for_tab(&self, tab_id: u32) -> Option<u32> {
        for workspace in &self.workspaces() {
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

    pub(crate) fn find_pane_by_id_mut(&mut self, pane_id: u32) -> Option<&mut tasty_model::Pane> {
        for workspace in self.workspaces_mut() {
            if let Some(pane) = workspace.pane_layout_mut().find_pane_mut(pane_id) {
                return Some(pane);
            }
        }
        None
    }

    pub fn find_workspace_index_for_surface(&self, surface_id: u32) -> Option<(usize, u32)> {
        for (i, workspace) in self.workspaces().into_iter().enumerate() {
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
        self.workspaces().into_iter().position(|w| w.id == ws_id)
    }

    /// surface가 mirror workspace에 속하는지 확인한다. ID를 못 찾으면 false다.
    pub fn is_mirror_surface(&self, surface_id: u32) -> bool {
        self.find_workspace_index_for_surface(surface_id)
            .and_then(|(idx, _)| self.workspace_at(idx))
            .map(|ws| ws.mirror)
            .unwrap_or(false)
    }

    /// 카테고리 안의 workspace와 전역 인덱스를 함께 반환한다. 인덱스는 카테고리 내부 순번이 아니다.
    pub fn workspaces_in_category(
        &self,
        category: tasty_model::WorkspaceCategoryId,
    ) -> Vec<(usize, &tasty_model::Workspace)> {
        self.workspaces()
            .into_iter()
            .enumerate()
            .filter(|(_, w)| w.category == category)
            .collect()
    }

    pub fn category_index(&self, category_id: tasty_model::WorkspaceCategoryId) -> Option<usize> {
        self.categories.iter().position(|c| c.id == category_id)
    }

    /// surface의 workspace 이름과 탭 표시 이름. 트리에 없으면 None이다.
        pub fn surface_display_path(
        &self,
        surface_id: u32,
        presentation: &dyn tasty_model::StructurePresentation,
    ) -> Option<SurfaceDisplayPath> {
        for workspace in &self.workspaces() {
            for pid in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.contains_surface(surface_id) {
                            return Some(SurfaceDisplayPath {
                                workspace_name: workspace.name.clone(),
                                tab_name: Some(tab.display_name(presentation.surface_id(tab))),
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
pub struct SurfaceDisplayPath {
    pub workspace_name: String,
    pub tab_name: Option<String>,
}


impl crate::CoreState {
    pub fn has_surface(&self, surface_id: u32) -> bool {
        self.workspaces()
            .into_iter()
            .any(|ws| ws.all_surface_ids().contains(&surface_id))
    }

    pub fn has_workspace(&self, workspace_id: u32) -> bool {
        self.workspaces()
            .into_iter()
            .any(|ws| ws.id == workspace_id)
    }

    pub fn has_pane(&self, pane_id: u32) -> bool {
        self.workspaces()
            .into_iter()
            .any(|ws| ws.pane_layout().all_pane_ids().contains(&pane_id))
    }
}

