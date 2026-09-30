//! ID-based navigation. The owner decides whether these values represent a local
//! View's selection or the defaults of a headless command context. The domain
//! tree is read only; reconciling it never performs a domain mutation.

use std::collections::HashMap;

use crate::model::{Pane, SurfaceId, Tab, Workspace, WorkspaceId};

/// The prior order is a repair baseline, not another selected index. It is used
/// only when the selected ID has disappeared, preserving the legacy next-slot
/// (or previous-last-slot) deletion policy.
#[derive(Clone, Debug, Default)]
struct Selection {
    selected: Option<u32>,
    previous_order: Vec<u32>,
}

impl Selection {
    fn resolve(&self, ids: &[u32]) -> Option<u32> {
        if let Some(id) = self.selected.filter(|id| ids.contains(id)) {
            return Some(id);
        }
        if let Some(index) = self
            .selected
            .and_then(|id| self.previous_order.iter().position(|old| *old == id))
        {
            // A snapshot may remove several siblings at once. Looking up the
            // old numeric slot in the new list would skip surviving neighbours.
            return self.previous_order[index + 1..]
                .iter()
                .find(|id| ids.contains(id))
                .or_else(|| {
                    self.previous_order[..index]
                        .iter()
                        .rev()
                        .find(|id| ids.contains(id))
                })
                .copied()
                .or_else(|| ids.first().copied());
        }
        ids.first().copied()
    }

    fn reconcile(&mut self, ids: Vec<u32>) {
        self.selected = self.resolve(&ids);
        self.previous_order = ids;
    }

    fn select(&mut self, id: u32, ids: Vec<u32>) -> bool {
        if !ids.contains(&id) {
            return false;
        }
        let changed = self.resolve(&ids) != Some(id);
        self.selected = Some(id);
        self.previous_order = ids;
        changed
    }
}

/// One owner's navigation, independent from the shared structural model.
#[derive(Clone, Debug, Default)]
pub(crate) struct NavigationState {
    pub(crate) split_hints: HashMap<crate::model::SplitNodeId, bool>,
    pub(crate) collapsed_categories: std::collections::HashSet<u32>,
    workspace: Selection,
    panes: HashMap<WorkspaceId, u32>,
    selected_tabs: HashMap<u32, Selection>,
    surfaces: HashMap<u32, SurfaceId>,
}

impl NavigationState {
    #[cfg(feature = "gui")]
    pub(crate) fn restore(
        &mut self,
        workspaces: &[Workspace],
        restored: &crate::model::RestoredPresentation,
    ) {
        self.collapsed_categories = restored.selection.collapsed_categories.clone();
        self.split_hints = restored.selection.split_hints.clone();
        self.reconcile(workspaces);
        if let Some(id) = restored.active_workspace {
            self.select_workspace(workspaces, id);
        }
        self.apply_snapshot(workspaces, &restored.selection);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn apply_snapshot(
        &mut self,
        workspaces: &[Workspace],
        selection: &crate::model::StructurePresentationSnapshot,
    ) {
        self.split_hints
            .extend(selection.split_hints.iter().map(|(id, hint)| (*id, *hint)));
        self.reconcile(workspaces);
        for ws in workspaces {
            if let Some(id) = selection.panes.get(&ws.id) {
                self.select_pane(ws, *id);
            }
            for id in ws.pane_layout().all_pane_ids() {
                let Some(pane) = ws.pane_layout().find_pane(id) else {
                    continue;
                };
                if let Some(id) = selection.selected_tabs.get(&pane.id) {
                    self.select_tab(pane, *id);
                }
                for tab in &pane.tabs {
                    if let Some(id) = selection.surfaces.get(&tab.id) {
                        self.select_surface(tab, *id);
                    }
                }
            }
        }
    }

    pub(crate) fn workspace_id(&self, workspaces: &[Workspace]) -> Option<WorkspaceId> {
        self.workspace.resolve(&workspace_ids(workspaces))
    }

    pub(crate) fn workspace_index(&self, workspaces: &[Workspace]) -> usize {
        self.workspace_id(workspaces)
            .and_then(|id| workspaces.iter().position(|ws| ws.id == id))
            .unwrap_or(0)
    }

    pub(crate) fn pane_id(&self, workspace: &Workspace) -> Option<u32> {
        self.panes
            .get(&workspace.id)
            .copied()
            .filter(|id| workspace.pane_layout().find_pane(*id).is_some())
            .or_else(|| workspace.pane_layout().first_pane().map(|pane| pane.id))
    }

    pub(crate) fn tab_id(&self, pane: &Pane) -> Option<u32> {
        self.selected_tabs
            .get(&pane.id)
            .and_then(|selection| selection.resolve(&tab_ids(pane)))
            .or_else(|| pane.tabs.first().map(|tab| tab.id))
    }

    pub(crate) fn tab_index(&self, pane: &Pane) -> usize {
        self.tab_id(pane)
            .and_then(|id| pane.tabs.iter().position(|tab| tab.id == id))
            .unwrap_or(0)
    }

    pub(crate) fn surface_id(&self, tab: &Tab) -> Option<SurfaceId> {
        self.surfaces
            .get(&tab.id)
            .copied()
            .filter(|id| tab.contains_surface(*id))
            .or_else(|| tab.layout_if_initialized()?.first_surface_id())
    }

    pub(crate) fn select_workspace(&mut self, workspaces: &[Workspace], id: WorkspaceId) -> bool {
        self.workspace.select(id, workspace_ids(workspaces))
    }

    pub(crate) fn select_pane(&mut self, workspace: &Workspace, id: u32) -> bool {
        if workspace.pane_layout().find_pane(id).is_none() {
            return false;
        }
        let changed = self.pane_id(workspace) != Some(id);
        self.panes.insert(workspace.id, id);
        changed
    }

    pub(crate) fn select_tab(&mut self, pane: &Pane, id: u32) -> bool {
        self.selected_tabs
            .entry(pane.id)
            .or_default()
            .select(id, tab_ids(pane))
    }

    pub(crate) fn select_surface(&mut self, tab: &Tab, id: SurfaceId) -> bool {
        if !tab.contains_surface(id) {
            return false;
        }
        let changed = self.surface_id(tab) != Some(id);
        self.surfaces.insert(tab.id, id);
        changed
    }

    pub(crate) fn goto_tab(&mut self, pane: &Pane, index: usize) -> crate::model::TabSwitch {
        let Some(tab) = pane.tabs.get(index) else {
            return crate::model::TabSwitch::OutOfRange {
                tabs: pane.tabs.len(),
            };
        };
        if self.select_tab(pane, tab.id) {
            crate::model::TabSwitch::Switched
        } else {
            crate::model::TabSwitch::AlreadyActive
        }
    }

    // Mirror snapshots initialize only missing/deleted selections from the wire.
    // Surviving local choices win; a pending user close may then choose its neighbour.
    #[cfg(feature = "gui")]
    pub(crate) fn initialize_pane(&mut self, workspace: &Workspace, pane: u32) {
        if !self
            .panes
            .get(&workspace.id)
            .is_some_and(|id| workspace.pane_layout().find_pane(*id).is_some())
        {
            self.select_pane(workspace, pane);
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn initialize_tab(&mut self, pane: &Pane, index: usize) {
        let selection = self.selected_tabs.entry(pane.id).or_default();
        if !selection
            .selected
            .is_some_and(|id| pane.tabs.iter().any(|tab| tab.id == id))
            && let Some(tab) = pane.tabs.get(index)
        {
            selection.select(tab.id, tab_ids(pane));
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn initialize_surface(&mut self, tab: &Tab, surface: u32) {
        if !self
            .surfaces
            .get(&tab.id)
            .is_some_and(|id| tab.contains_surface(*id))
        {
            self.select_surface(tab, surface);
        }
    }

    pub(crate) fn apply_result(
        &mut self,
        workspaces: &[Workspace],
        event: &crate::core::intent::CoreEvent,
    ) {
        use crate::core::intent::CoreEvent;
        match event {
            CoreEvent::MoveSurfaceApplied {
                replacement: Some((removed, replacement)),
                ..
            } => self.remap_surface_selection(*removed, *replacement),
            CoreEvent::ContainerMoveApplied {
                replaced_tab,
                replaced_pane,
                ..
            } => {
                if let Some((removed, replacement)) = replaced_tab {
                    self.remap_tab_selection(*removed, *replacement);
                }
                if let Some((removed, replacement)) = replaced_pane {
                    self.remap_pane_selection(*removed, *replacement);
                }
            }
            _ => {}
        }
        self.reconcile(workspaces);
    }

    pub(crate) fn remap_surface_selection(&mut self, removed: u32, replacement: u32) {
        for selected in self.surfaces.values_mut() {
            if *selected == removed {
                *selected = replacement;
            }
        }
    }
    pub(crate) fn remap_tab_selection(&mut self, removed: u32, replacement: u32) {
        for selection in self.selected_tabs.values_mut() {
            if selection.selected == Some(removed) {
                selection.selected = Some(replacement);
            }
        }
    }
    pub(crate) fn remap_pane_selection(&mut self, removed: u32, replacement: u32) {
        for selected in self.panes.values_mut() {
            if *selected == removed {
                *selected = replacement;
            }
        }
    }

    /// Apply the current structural result. Surviving IDs are retained; missing
    /// selections alone are replaced. New children are initialized without
    /// selecting them in their already existing parent.
    pub(crate) fn reconcile(&mut self, workspaces: &[Workspace]) {
        self.workspace.reconcile(workspace_ids(workspaces));
        self.panes
            .retain(|ws, _| workspaces.iter().any(|w| w.id == *ws));
        let mut pane_ids = Vec::new();
        let mut live_tabs = Vec::new();
        let mut split_nodes = Vec::new();
        for ws in workspaces {
            if let Some(id) = self.pane_id(ws) {
                self.panes.insert(ws.id, id);
            }
            for id in ws.pane_layout().all_pane_ids() {
                let Some(pane) = ws.pane_layout().find_pane(id) else {
                    continue;
                };
                pane_ids.push(id);
                self.selected_tabs
                    .entry(id)
                    .or_default()
                    .reconcile(tab_ids(pane));
                for tab in &pane.tabs {
                    live_tabs.push(tab.id);
                    if let Some(layout) = tab.layout_if_initialized() {
                        layout.split_node_ids(&mut split_nodes);
                    }
                    if let Some(id) = self.surface_id(tab) {
                        self.surfaces.insert(tab.id, id);
                    } else {
                        self.surfaces.remove(&tab.id);
                    }
                }
            }
        }
        self.split_hints.retain(|id, _| split_nodes.contains(id));
        self.selected_tabs.retain(|id, _| pane_ids.contains(id));
        self.surfaces.retain(|id, _| live_tabs.contains(id));
    }
}

fn workspace_ids(workspaces: &[Workspace]) -> Vec<u32> {
    workspaces.iter().map(|ws| ws.id).collect()
}

fn tab_ids(pane: &Pane) -> Vec<u32> {
    pane.tabs.iter().map(|tab| tab.id).collect()
}

#[cfg(test)]
mod tests;

impl crate::model::StructurePresentation for NavigationState {
    fn split_focus_second(&self, node: crate::model::SplitNodeId) -> bool {
        self.split_hints.get(&node).copied().unwrap_or(true)
    }
    fn category_collapsed(&self, category: u32) -> bool {
        self.collapsed_categories.contains(&category)
    }
    fn pane_id(&self, workspace: &Workspace) -> Option<u32> {
        self.pane_id(workspace)
    }
    fn tab_index(&self, pane: &Pane) -> usize {
        self.tab_index(pane)
    }
    fn surface_id(&self, tab: &Tab) -> Option<SurfaceId> {
        self.surface_id(tab)
    }
}
