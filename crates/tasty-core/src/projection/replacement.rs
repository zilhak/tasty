//! Apply a replacement to descriptors while retaining surviving layout nodes and runtime IDs.
use super::*;
use tasty_model::{PaneNode,SurfaceLayout};
pub(super) fn apply(core:&mut CoreState,replacement:crate::Replacement,retired:&mut Vec<Retired>)->Result<()> {
    let source=replacement.source.id;let target=replacement.target.id;
    match replacement.source.kind {
        crate::IdKind::Surface=> {
            let source_tab=core.find_tab_for_surface(source).ok_or("source surface missing")?;
            let moved=if tab(core,source_tab)?.all_surface_ids().len()==1 {
                let mut old=take_tab(core,source_tab)?;
                let SurfaceLayout::Leaf(surface)=old.take_layout() else {return Err("single surface was not a leaf".into());};surface
            } else {
                let source_tab=tab(core,source_tab)?;
                let (remaining,removed)=source_tab.take_layout().extract_surface(source);source_tab.put_layout(remaining);removed.ok_or("source surface could not detach")?
            };
            let target_tab=core.find_tab_for_surface(target).ok_or("replacement target disappeared")?;
            let slot=tab(core,target_tab)?.layout_mut().find_leaf_mut(target).ok_or("replacement target slot missing")?;
            retired.push(Retired::Surface(std::mem::replace(slot,moved)));
        },
        crate::IdKind::Tab=> {
            let moved=take_tab(core,source)?;
            let parent=core.find_pane_for_tab(target).ok_or("replacement tab parent missing")?;
            let pane=pane(core,parent)?;let index=pane.tabs.iter().position(|tab|tab.id==target).ok_or("replacement tab missing")?;
            retired.push(Retired::Tab(std::mem::replace(&mut pane.tabs[index],moved)));
        },
        crate::IdKind::Pane=> {
            let moved=take_pane(core,source)?;
            let index=core.find_workspace_index_for_pane(target).ok_or("replacement pane parent missing")?;
            let old=core.workspace_at_mut(index).ok_or("replacement workspace missing")?.pane_layout_mut().replace_pane(target,moved).map_err(|_|"replacement pane slot missing")?;
            retired.push(Retired::Pane(old));
        },
        _=>return Err("unsupported replacement kind".into()),
    }
    Ok(())
}
fn take_tab(core:&mut CoreState,id:u32)->Result<Tab> {
    let parent=core.find_pane_for_tab(id).ok_or("source tab parent missing")?;
    let moved=layout::detach_tab(core,id)?;
    if pane(core,parent)?.tabs.is_empty() {drop(take_pane(core,parent)?);}
    Ok(moved)
}
fn take_pane(core:&mut CoreState,id:u32)->Result<Pane> {
    let index=core.find_workspace_index_for_pane(id).ok_or("source pane parent missing")?;
    let workspace=core.workspace_at_mut(index).ok_or("source workspace missing")?;
    if workspace.pane_layout().all_pane_ids().len()>1 {return workspace.detach_pane(id).ok_or_else(||"source pane cannot detach".into());}
    let mut workspace=core.remove_workspace_at(index);
    let node=std::mem::replace(workspace.pane_layout_mut(),PaneNode::Leaf(Pane::default()));
    match node {PaneNode::Leaf(pane) if pane.id==id=>Ok(pane),_=>Err("sole pane was not a leaf".into())}
}
