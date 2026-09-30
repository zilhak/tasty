//! Logical bootstrap before the first publication ACK. It does not read payloads or create resources.
use super::*;
use crate::model::{PaneNode, SplitNodeId, SurfaceLayout, WorkspaceCategory};
use tasty_domain::SplitTree;

/// Exists only behind the bootstrap read/render barrier. SurfaceRestorer replaces this with the
/// selected kind or its ordinary lazy/plugin placeholder after reading the referenced payload.
pub(crate) struct JournalPlaceholder {
    pub(crate) id: u32,
    pub(crate) kind: String,
    pub(crate) data: Option<tasty_domain::DataRef>,
    pub(crate) creation_seed: Option<tasty_domain::DataRef>,
    pub(crate) activation: Option<tasty_domain::Activation>,
}

impl Surface for JournalPlaceholder {
    tasty_model::impl_surface_any!();
    fn kind(&self) -> &'static str {
        "empty"
    }
    fn type_name(&self) -> &'static str {
        "Pending"
    }
    fn surface_id(&self) -> Option<u32> {
        Some(self.id)
    }
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }
}

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
            Ok(SurfaceLayout::Leaf(Box::new(JournalPlaceholder {
                id: *id,
                kind: source.kind.clone(),
                data: source.data,
                creation_seed: source.creation_seed,
                activation: source.activation,
            })))
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
