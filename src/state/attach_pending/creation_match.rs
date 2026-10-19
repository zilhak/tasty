//! Structural evidence for peers that do not report the created surface ID.
use super::{Creation, Entry};
use crate::core::CoreState;
use crate::model::{BinaryTree, PaneNode, SplitDirection, SurfaceLayout, Workspace};

pub(super) struct CreationBaseline {
    panes: Vec<u32>,
    tabs: Vec<u32>,
}
impl CreationBaseline {
    pub fn capture(workspace: &Workspace) -> Self {
        let panes = workspace.pane_layout().all_pane_ids();
        let tabs = panes
            .iter()
            .filter_map(|id| workspace.pane_layout().find_pane(*id))
            .flat_map(|pane| pane.tabs.iter().map(|tab| tab.id))
            .collect();
        Self { panes, tabs }
    }
    pub fn matches(&self, entry: &Entry, core: &CoreState, surface: u32) -> bool {
        let Some((index, pane_id)) = core.find_workspace_index_for_surface(surface) else {
            return false;
        };
        let Some(workspace) = core
            .workspace_at(index)
            .filter(|ws| ws.id == entry.workspace)
        else {
            return false;
        };
        let Some((_, anchor_pane)) = core.find_workspace_index_for_surface(entry.anchor) else {
            return false;
        };
        if anchor_pane != entry.pane {
            return false;
        }
        let Some(pane) = workspace.pane_layout().find_pane(pane_id) else {
            return false;
        };
        let Some(tab) = pane.tabs.iter().find(|tab| tab.contains_surface(surface)) else {
            return false;
        };
        if tab
            .layout()
            .find_surface(surface)
            .is_none_or(|s| s.kind() != entry.kind)
        {
            return false;
        }
        match entry.creation {
            Some(Creation::Tab) => {
                pane_id == entry.pane
                    && !self.tabs.contains(&tab.id)
                    && tab.layout().leaf_id() == Some(surface)
            }
            Some(Creation::Surface(direction)) => {
                pane_id == entry.pane
                    && tab.id == entry.tab
                    && surface_sibling(tab.layout(), entry.anchor, surface, direction)
            }
            Some(Creation::Pane(direction)) => {
                !self.panes.contains(&pane_id)
                    && pane.tabs.len() == 1
                    && tab.layout().leaf_id() == Some(surface)
                    && pane_sibling(workspace.pane_layout(), entry.pane, pane_id, direction)
            }
            None => false,
        }
    }
}
fn surface_sibling(tree: &SurfaceLayout, anchor: u32, new: u32, axis: SplitDirection) -> bool {
    let Some((direction, _, first, second)) = tree.split_parts() else {
        return false;
    };
    (direction == axis && first.contains_surface(anchor) && second.leaf_id() == Some(new))
        || surface_sibling(first, anchor, new, axis)
        || surface_sibling(second, anchor, new, axis)
}
fn pane_sibling(tree: &PaneNode, anchor: u32, new: u32, axis: SplitDirection) -> bool {
    let Some((direction, _, first, second)) = tree.split_parts() else {
        return false;
    };
    (direction == axis && first.find_pane(anchor).is_some() && second.leaf_id() == Some(new))
        || pane_sibling(first, anchor, new, axis)
        || pane_sibling(second, anchor, new, axis)
}

#[derive(Default)]
pub(super) struct Plan {
    pub target: Option<u32>,
    pub overlap: bool,
}
pub(super) fn plans(
    entries: &[Entry],
    core: &CoreState,
    epoch: &tasty_remote::connection::ConnectionEpoch,
    mapping: &std::collections::HashMap<u32, u32>,
    new_remote: &[u32],
) -> Vec<Plan> {
    let known: std::collections::HashSet<_> = entries
        .iter()
        .filter(|entry| entry.epoch.same(epoch))
        .filter_map(|entry| entry.receipt.created())
        .collect();
    let candidates: Vec<Vec<_>> = entries
        .iter()
        .map(|entry| {
            if !entry.epoch.same(epoch)
                || entry.receipt.op().is_none()
                || entry.receipt.finished()
                || entry.receipt.created().is_some()
            {
                return Vec::new();
            }
            new_remote
                .iter()
                .filter(|id| !known.contains(id))
                .filter_map(|remote| {
                    let local = *mapping.get(remote)?;
                    entry
                        .baseline
                        .as_ref()?
                        .matches(entry, core, local)
                        .then_some(local)
                })
                .collect()
        })
        .collect();
    candidates
        .iter()
        .map(|own| {
            let target = own.first().copied().filter(|local| {
                own.len() == 1
                    && candidates
                        .iter()
                        .filter(|other| other.contains(local))
                        .count()
                        == 1
            });
            Plan {
                target,
                overlap: !own.is_empty(),
            }
        })
        .collect()
}
