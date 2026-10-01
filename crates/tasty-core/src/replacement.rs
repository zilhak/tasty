//! Move an existing subtree into another position, retaining source identities and activations.
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Replacement {
    pub source: EntityId,
    pub target: EntityId,
}
impl Replacement {
    pub fn simulate(
        &self,
        model: &JournalModel,
    ) -> Result<(JournalModel, Vec<EntityId>), Rejection> {
        if self.source == self.target || self.source.kind != self.target.kind {
            return Err(Rejection(
                "replacement requires different targets of the same kind".into(),
            ));
        }
        let mut next = model.clone();
        match self.source.kind {
            IdKind::Surface => {
                model
                    .surfaces
                    .get(&self.source.id)
                    .ok_or_else(|| Rejection("source surface missing".into()))?;
                let target = model
                    .surfaces
                    .get(&self.target.id)
                    .ok_or_else(|| Rejection("target surface missing".into()))?;
                detach_surface(&mut next, self.source.id)?;
                let tab = next
                    .tabs
                    .get_mut(&target.tab)
                    .ok_or_else(|| Rejection("target tab disappeared".into()))?;
                if !replace_leaf(&mut tab.layout, self.target.id, self.source.id) {
                    return Err(Rejection("target surface slot missing".into()));
                }
                next.surfaces.remove(&self.target.id);
                next.surfaces
                    .get_mut(&self.source.id)
                    .ok_or_else(|| Rejection("moved source disappeared".into()))?
                    .tab = target.tab;
            }
            IdKind::Tab => {
                let source = model
                    .tabs
                    .get(&self.source.id)
                    .ok_or_else(|| Rejection("source tab missing".into()))?;
                let target = model
                    .tabs
                    .get(&self.target.id)
                    .ok_or_else(|| Rejection("target tab missing".into()))?;
                detach_tab(&mut next, self.source.id, source.pane)?;
                let pane = next
                    .panes
                    .get_mut(&target.pane)
                    .ok_or_else(|| Rejection("target pane disappeared".into()))?;
                let index = pane
                    .tabs
                    .iter()
                    .position(|id| *id == self.target.id)
                    .ok_or_else(|| Rejection("target tab slot missing".into()))?;
                pane.tabs[index] = self.source.id;
                for surface in target.layout.leaves() {
                    next.surfaces.remove(&surface);
                }
                next.tabs.remove(&self.target.id);
                next.tabs
                    .get_mut(&self.source.id)
                    .ok_or_else(|| Rejection("moved tab disappeared".into()))?
                    .pane = target.pane;
            }
            IdKind::Pane => {
                let source = model
                    .panes
                    .get(&self.source.id)
                    .ok_or_else(|| Rejection("source pane missing".into()))?;
                let target = model
                    .panes
                    .get(&self.target.id)
                    .ok_or_else(|| Rejection("target pane missing".into()))?;
                detach_pane(&mut next, self.source.id, source.workspace)?;
                let workspace = next
                    .workspaces
                    .get_mut(&target.workspace)
                    .ok_or_else(|| Rejection("target workspace disappeared".into()))?;
                if !replace_leaf(&mut workspace.layout, self.target.id, self.source.id) {
                    return Err(Rejection("target pane slot missing".into()));
                }
                for tab in &target.tabs {
                    if let Some(tab) = next.tabs.remove(tab) {
                        for surface in tab.layout.leaves() {
                            next.surfaces.remove(&surface);
                        }
                    }
                }
                next.panes.remove(&self.target.id);
                next.panes
                    .get_mut(&self.source.id)
                    .ok_or_else(|| Rejection("moved pane disappeared".into()))?
                    .workspace = target.workspace;
            }
            _ => return Err(Rejection("unsupported replacement kind".into())),
        }
        let mut removed = Vec::new();
        for (kind, before, after) in [
            (
                IdKind::Workspace,
                model.workspaces.keys().copied().collect::<Vec<_>>(),
                next.workspaces.keys().copied().collect::<Vec<_>>(),
            ),
            (
                IdKind::Pane,
                model.panes.keys().copied().collect(),
                next.panes.keys().copied().collect(),
            ),
            (
                IdKind::Tab,
                model.tabs.keys().copied().collect(),
                next.tabs.keys().copied().collect(),
            ),
            (
                IdKind::Surface,
                model.surfaces.keys().copied().collect(),
                next.surfaces.keys().copied().collect(),
            ),
        ] {
            removed.extend(
                before
                    .into_iter()
                    .filter(|id| !after.contains(id))
                    .map(|id| EntityId { kind, id }),
            );
        }
        Ok((next, removed))
    }
    pub fn surfaces(&self, model: &JournalModel, entity: EntityId) -> Option<Vec<u32>> {
        match entity.kind {
            IdKind::Surface => model
                .surfaces
                .contains_key(&entity.id)
                .then_some(vec![entity.id]),
            IdKind::Tab => Some(model.tabs.get(&entity.id)?.layout.leaves()),
            IdKind::Pane => model
                .panes
                .get(&entity.id)?
                .tabs
                .iter()
                .map(|tab| model.tabs.get(tab).map(|tab| tab.layout.leaves()))
                .collect::<Option<Vec<_>>>()
                .map(|tabs| tabs.into_iter().flatten().collect()),
            _ => None,
        }
    }
}
fn replace_leaf(tree: &mut SplitTree<u32>, target: u32, source: u32) -> bool {
    match tree {
        SplitTree::Leaf(id) => {
            if *id == target {
                *id = source;
                true
            } else {
                false
            }
        }
        SplitTree::Split { first, second, .. } => {
            replace_leaf(first, target, source) || replace_leaf(second, target, source)
        }
    }
}
fn detach_surface(model: &mut JournalModel, id: u32) -> Result<(), Rejection> {
    let tab_id = model
        .surfaces
        .get(&id)
        .ok_or_else(|| Rejection("source surface missing".into()))?
        .tab;
    let tab = model
        .tabs
        .get_mut(&tab_id)
        .ok_or_else(|| Rejection("source tab missing".into()))?;
    match tab.layout.remove_leaf(id) {
        crate::model::RemoveLeaf::Removed => Ok(()),
        crate::model::RemoveLeaf::LastLeaf => {
            let pane = tab.pane;
            detach_tab(model, tab_id, pane)?;
            model.tabs.remove(&tab_id);
            Ok(())
        }
        _ => Err(Rejection("source leaf missing".into())),
    }
}
fn detach_tab(model: &mut JournalModel, id: u32, pane_id: u32) -> Result<(), Rejection> {
    let pane = model
        .panes
        .get_mut(&pane_id)
        .ok_or_else(|| Rejection("source pane missing".into()))?;
    pane.tabs.retain(|tab| *tab != id);
    if pane.tabs.is_empty() {
        let workspace = pane.workspace;
        detach_pane(model, pane_id, workspace)?;
        model.panes.remove(&pane_id);
    }
    Ok(())
}
fn detach_pane(model: &mut JournalModel, id: u32, workspace_id: u32) -> Result<(), Rejection> {
    let workspace = model
        .workspaces
        .get_mut(&workspace_id)
        .ok_or_else(|| Rejection("source workspace missing".into()))?;
    match workspace.layout.remove_leaf(id) {
        crate::model::RemoveLeaf::Removed => Ok(()),
        crate::model::RemoveLeaf::LastLeaf => {
            model.workspaces.remove(&workspace_id);
            model
                .workspace_order
                .retain(|workspace| *workspace != workspace_id);
            Ok(())
        }
        _ => Err(Rejection("source pane slot missing".into())),
    }
}
