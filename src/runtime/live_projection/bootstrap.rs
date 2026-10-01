//! Logical bootstrap before the first publication ACK. It does not read payloads or create resources.
use super::*;
use crate::model::{PaneNode, SplitNodeId, SurfaceLayout, WorkspaceCategory};
use tasty_domain::SplitTree;

pub(crate) fn initialize(core: &mut CoreState, model: &JournalModel) -> Result<()> {
    if !core.local_workspaces.is_empty() {
        return Err("bootstrap cannot replace an already published local tree".into());
    }
    let workspaces = model
        .workspace_order
        .iter()
        .map(|id| {
            let source = model
                .workspaces
                .get(id)
                .ok_or("bootstrap workspace missing")?;
            let layout = pane_tree(model, &source.layout)?;
            let mut workspace = Workspace::new_with_pane(*id, source.name.clone(), Pane::default());
            *workspace.pane_layout_mut() = layout;
            workspace.category = source.category;
            workspace.subtitle = source.subtitle.clone();
            workspace.description = source.description.clone();
            workspace.attach_mapping = source.attach_mapping.clone();
            Ok(workspace)
        })
        .collect::<Result<Vec<_>>>()?;
    let categories = model
        .category_order
        .iter()
        .map(|id| {
            Ok(WorkspaceCategory::new(
                *id,
                model
                    .categories
                    .get(id)
                    .ok_or("bootstrap category missing")?
                    .name
                    .clone(),
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    core.replace_local_workspaces(workspaces);
    core.categories = categories;
    if live::core_canonical(core) != Canonical::of_journal(model, &SkipData) {
        return Err("bootstrap live projection differs from journal structure".into());
    }
    core.committed_structure_revision = model.applied.revision;
    Ok(())
}

fn pane_tree(model: &JournalModel, tree: &SplitTree<u32>) -> Result<PaneNode> {
    match tree {
        SplitTree::Leaf(id) => {
            let source = model.panes.get(id).ok_or("bootstrap pane missing")?;
            let tabs = source
                .tabs
                .iter()
                .map(|id| {
                    let source = model.tabs.get(id).ok_or("bootstrap tab missing")?;
                    Ok(Tab {
                        id: *id,
                        name: source.name.clone(),
                        explicit_name: source.explicit_name.clone(),
                        layout_opt: Some(surface_tree(model, &source.layout)?),
                        surface_titles: Default::default(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(PaneNode::Leaf(Pane { id: *id, tabs }))
        }
        SplitTree::Split {
            direction,
            ratio,
            first,
            second,
        } => Ok(PaneNode::Split {
            direction: *direction,
            ratio: ratio.to_f32(),
            first: Box::new(pane_tree(model, first)?),
            second: Box::new(pane_tree(model, second)?),
        }),
    }
}

fn surface_tree(model: &JournalModel, tree: &SplitTree<u32>) -> Result<SurfaceLayout> {
    match tree {
        SplitTree::Leaf(id) => {
            let source = model.surfaces.get(id).ok_or("bootstrap surface missing")?;
            Ok(SurfaceLayout::Leaf(crate::model::SurfaceDescriptor {
                id:*id,kind:source.kind.clone(),activation_generation:source.activation.map(|activation|activation.generation),
            }))
        }
        SplitTree::Split {
            direction,
            ratio,
            first,
            second,
        } => Ok(SurfaceLayout::Split {
            node_id: SplitNodeId::allocate(),
            direction: *direction,
            ratio: ratio.to_f32(),
            first: Box::new(surface_tree(model, first)?),
            second: Box::new(surface_tree(model, second)?),
        }),
    }
}

pub(crate) fn presentation(
    core: &CoreState,
    view: Option<crate::core::layout_persistence::import::ImportedView>,
) -> crate::model::RestoredPresentation {
    let view = view.unwrap_or_default();
    let mut result = crate::model::RestoredPresentation {
        active_workspace: view
            .active_workspace
            .filter(|id| {
                core.local_workspaces
                    .iter()
                    .any(|workspace| workspace.id == *id)
            })
            .or_else(|| core.local_workspaces.first().map(|workspace| workspace.id)),
        selection: crate::model::StructurePresentationSnapshot::default(),
    };
    result.selection.panes.extend(view.focused_panes);
    result.selection.selected_tabs.extend(view.active_tabs);
    result.selection.surfaces.extend(view.selected_surfaces);
    result
        .selection
        .collapsed_categories
        .extend(view.collapsed_categories);
    for workspace in &core.local_workspaces {
        for id in workspace.pane_layout().all_pane_ids() {
            if let Some(pane) = workspace.pane_layout().find_pane(id) {
                for tab in &pane.tabs {
                    if let Some(surface) = tab.first_surface_id() {
                        result.selection.surfaces.entry(tab.id).or_insert(surface);
                    }
                    let mut nodes = Vec::new();
                    tab.layout().split_node_ids(&mut nodes);
                    result
                        .selection
                        .split_hints
                        .extend(nodes.into_iter().map(|node| (node, false)));
                }
            }
        }
    }
    result
}
