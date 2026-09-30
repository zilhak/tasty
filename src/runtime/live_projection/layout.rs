use super::*;
use crate::model::{PaneNode, SurfaceLayout};
use tasty_domain::{Placement, SplitSpec};

pub(super) fn find_tab_mut(node: &mut PaneNode, id: u32) -> Option<&mut Tab> {
    match node {
        PaneNode::Leaf(pane) => pane.tabs.iter_mut().find(|tab| tab.id == id),
        PaneNode::Split { first, second, .. } => {
            find_tab_mut(first, id).or_else(|| find_tab_mut(second, id))
        }
    }
}

pub(super) fn detach_tab(engine: &mut CoreState, id: u32) -> Result<Tab> {
    let pane_id = engine.find_pane_for_tab(id).ok_or("tab missing")?;
    let pane = pane(engine, pane_id)?;
    let index = pane
        .tabs
        .iter()
        .position(|tab| tab.id == id)
        .ok_or("tab missing")?;
    Ok(pane.tabs.remove(index))
}

pub(super) fn apply_event(
    engine: &mut CoreState,
    event: &DomainEvent,
    prepared: &mut PreparedLeaves,
    retired: &mut Vec<Retired>,
) -> Result<()> {
    match event {
        DomainEvent::PaneSplit {
            target,
            pane: id,
            split,
        } => insert_pane(
            engine,
            *target,
            Pane {
                id: *id,
                tabs: Vec::new(),
            },
            split,
        )?,
        DomainEvent::PaneMoved { id, target, split } => {
            let moved = detach_pane(engine, *id)?;
            insert_pane(engine, *target, moved, split)?;
        }
        DomainEvent::PaneClosed { id } => retired.push(Retired::Pane(detach_pane(engine, *id)?)),
        DomainEvent::SurfaceSplit {
            target,
            surface,
            split,
        } => {
            let surface = take_prepared(prepared, surface.id)?;
            insert_surface(engine, *target, surface, split)?;
        }
        DomainEvent::SurfaceMoved { id, target, split } => {
            let surface = detach_surface(engine, *id)?;
            insert_surface(engine, *target, surface, split)?;
        }
        DomainEvent::SurfaceClosed { id } => {
            retired.push(Retired::Surface(detach_surface(engine, *id)?))
        }
        DomainEvent::PaneRatioSet {
            workspace: id,
            path,
            ratio,
        } => {
            let mut node = workspace(engine, *id)?.pane_layout_mut();
            for second in path {
                node = match node {
                    PaneNode::Split {
                        first,
                        second: right,
                        ..
                    } => {
                        if *second {
                            right
                        } else {
                            first
                        }
                    }
                    _ => return Err("pane split path disappeared".into()),
                };
            }
            let PaneNode::Split { ratio: value, .. } = node else {
                return Err("pane split disappeared".into());
            };
            *value = ratio.to_f32();
        }
        DomainEvent::SurfaceRatioSet {
            tab: id,
            path,
            ratio,
        } => {
            let mut node = tab(engine, *id)?.layout_mut();
            for second in path {
                node = match node {
                    SurfaceLayout::Split {
                        first,
                        second: right,
                        ..
                    } => {
                        if *second {
                            right
                        } else {
                            first
                        }
                    }
                    _ => return Err("surface split path disappeared".into()),
                };
            }
            let SurfaceLayout::Split { ratio: value, .. } = node else {
                return Err("surface split disappeared".into());
            };
            *value = ratio.to_f32();
        }
        _ => unreachable!("structural event dispatches its own family"),
    }
    Ok(())
}

fn detach_pane(engine: &mut CoreState, id: u32) -> Result<Pane> {
    let workspace_id = engine
        .local_workspaces
        .iter()
        .find(|workspace| workspace.pane_layout().find_pane(id).is_some())
        .ok_or("pane missing")?
        .id;
    workspace(engine, workspace_id)?
        .detach_pane(id)
        .ok_or_else(|| "cannot detach the last pane".into())
}

fn insert_pane(engine: &mut CoreState, target: u32, pane: Pane, split: &SplitSpec) -> Result<()> {
    let workspace_id = engine
        .local_workspaces
        .iter()
        .find(|workspace| workspace.pane_layout().find_pane(target).is_some())
        .ok_or("pane target missing")?
        .id;
    if workspace(engine, workspace_id)?
        .pane_layout_mut()
        .insert_pane_beside(
            target,
            split.direction,
            split.ratio.to_f32(),
            pane,
            split.placement == Placement::Before,
        )
        .is_some()
    {
        return Err("pane target disappeared".into());
    }
    Ok(())
}

fn detach_surface(engine: &mut CoreState, id: u32) -> Result<Box<dyn Surface>> {
    let tab_id = engine.find_tab_for_surface(id).ok_or("surface missing")?;
    let tab = tab(engine, tab_id)?;
    let (remaining, removed) = tab.take_layout().extract_surface(id);
    tab.put_layout(remaining);
    removed.ok_or_else(|| "cannot detach the last surface".into())
}

fn insert_surface(
    engine: &mut CoreState,
    target: u32,
    surface: Box<dyn Surface>,
    split: &SplitSpec,
) -> Result<()> {
    let tab_id = engine
        .find_tab_for_surface(target)
        .ok_or("surface target missing")?;
    let tab = tab(engine, tab_id)?;
    let id = surface
        .surface_id()
        .ok_or("prepared surface has no identity")?;
    let (mut node, leftover) =
        tab.take_layout()
            .split_with_surface(target, split.direction, surface);
    if leftover.is_some() {
        tab.put_layout(node);
        return Err("surface target disappeared".into());
    }
    set_new_split(&mut node, target, id, split)?;
    tab.put_layout(node);
    Ok(())
}

fn set_new_split(
    node: &mut SurfaceLayout,
    target: u32,
    inserted: u32,
    split: &SplitSpec,
) -> Result<()> {
    match node {
        SurfaceLayout::Split {
            first,
            second,
            ratio,
            ..
        } => {
            if matches!(first.as_ref(), SurfaceLayout::Leaf(s) if s.surface_id() == Some(target))
                && matches!(second.as_ref(), SurfaceLayout::Leaf(s) if s.surface_id() == Some(inserted))
            {
                *ratio = split.ratio.to_f32();
                if split.placement == Placement::Before {
                    std::mem::swap(first, second);
                }
                Ok(())
            } else if first.contains_surface(inserted) {
                set_new_split(first, target, inserted, split)
            } else {
                set_new_split(second, target, inserted, split)
            }
        }
        SurfaceLayout::Leaf(_) => Err("new surface split not found".into()),
    }
}
