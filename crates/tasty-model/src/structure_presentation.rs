//! Read-only selection used when projecting a structure into legacy wire data.
//! The structure owns none of these choices. The caller supplies its View,
//! restore or command-default context explicitly.

use crate::{Pane, SurfaceId, Tab, Workspace};

pub trait StructurePresentation {
    fn pane_id(&self, workspace: &Workspace) -> Option<u32>;
    fn tab_index(&self, pane: &Pane) -> usize;
    fn surface_id(&self, tab: &Tab) -> Option<SurfaceId>;
    fn category_collapsed(&self, category: u32) -> bool;
    fn split_focus_second(&self, node: crate::SplitNodeId) -> bool;
}

/// Immutable choices captured when a command is admitted. This value travels
/// with an undo-capture request; it is never a writable navigation owner.
#[derive(Clone, Debug, Default)]
pub struct StructurePresentationSnapshot {
    pub split_hints: std::collections::HashMap<crate::SplitNodeId, bool>,
    pub collapsed_categories: std::collections::HashSet<u32>,
    pub panes: std::collections::HashMap<u32, u32>,
    pub selected_tabs: std::collections::HashMap<u32, u32>,
    pub surfaces: std::collections::HashMap<u32, SurfaceId>,
}

impl StructurePresentationSnapshot {
    pub fn capture(
        workspaces: &[Workspace],
        categories: &[crate::WorkspaceCategory],
        presentation: &dyn StructurePresentation,
    ) -> Self {
        let mut result = Self::default();
        result.collapsed_categories.extend(
            categories
                .iter()
                .filter(|c| presentation.category_collapsed(c.id))
                .map(|c| c.id),
        );
        for ws in workspaces {
            if let Some(id) = presentation.pane_id(ws) {
                result.panes.insert(ws.id, id);
            }
            for id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(id) {
                    if let Some(tab) = pane.tabs.get(presentation.tab_index(pane)) {
                        result.selected_tabs.insert(id, tab.id);
                    }
                    for tab in &pane.tabs {
                        let mut nodes = Vec::new();
                        if let Some(layout) = tab.layout_if_initialized() {
                            layout.split_node_ids(&mut nodes);
                        }
                        result.split_hints.extend(
                            nodes
                                .into_iter()
                                .map(|node| (node, presentation.split_focus_second(node))),
                        );
                        if let Some(id) = presentation.surface_id(tab) {
                            result.surfaces.insert(tab.id, id);
                        }
                    }
                }
            }
        }
        result
    }
}

impl StructurePresentation for StructurePresentationSnapshot {
    fn split_focus_second(&self, node: crate::SplitNodeId) -> bool {
        self.split_hints.get(&node).copied().unwrap_or(true)
    }
    fn pane_id(&self, workspace: &Workspace) -> Option<u32> {
        self.panes
            .get(&workspace.id)
            .copied()
            .filter(|id| workspace.pane_layout().find_pane(*id).is_some())
            .or_else(|| workspace.pane_layout().first_pane().map(|p| p.id))
    }
    fn tab_index(&self, pane: &Pane) -> usize {
        self.selected_tabs
            .get(&pane.id)
            .and_then(|id| pane.tabs.iter().position(|t| t.id == *id))
            .unwrap_or(0)
    }
    fn surface_id(&self, tab: &Tab) -> Option<SurfaceId> {
        self.surfaces
            .get(&tab.id)
            .copied()
            .filter(|id| tab.contains_surface(*id))
            .or_else(|| tab.first_surface_id())
    }
    fn category_collapsed(&self, category: u32) -> bool {
        self.collapsed_categories.contains(&category)
    }
}

/// Presentation output of a legacy layout restore, using the newly allocated IDs.
#[derive(Clone, Debug, Default)]
pub struct RestoredPresentation {
    pub active_workspace: Option<u32>,
    pub selection: StructurePresentationSnapshot,
}
