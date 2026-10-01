//! Immutable undo references and closed structural values. No runtime object or factory is stored.
use std::collections::BTreeMap;
use serde::{Deserialize,Serialize};
use crate::{CloseTarget,DataRef,OperationId,Workspace,Pane,Tab,Surface,EntityId,IdKind,JournalModel};

#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub struct UndoCapture {
    pub snapshot:DataRef,
    /// Direct payload dependencies of the snapshot, pinned by events and compacted model snapshots.
    pub retained:Vec<DataRef>,
}
impl UndoCapture {
    pub fn data_refs(&self)->impl Iterator<Item=DataRef>+'_ {std::iter::once(self.snapshot).chain(self.retained.iter().copied())}
}
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub struct UndoRecord {
    pub id:OperationId,
    pub target:CloseTarget,
    pub capture:UndoCapture,
}
#[derive(Debug,Clone,PartialEq,Serialize,Deserialize)]
pub struct ClosedSnapshot {
    pub version:u32,
    pub root:EntityId,
    pub origin_workspace:Option<u32>,
    pub workspaces:BTreeMap<u32,Workspace>,
    pub panes:BTreeMap<u32,Pane>,
    pub tabs:BTreeMap<u32,Tab>,
    pub surfaces:BTreeMap<u32,Surface>,
    /// A surface-only close keeps its containing tab's presentation name for restore-as-tab.
    pub tab_name:Option<String>,
}
impl ClosedSnapshot {
    pub fn capture(model:&JournalModel,target:CloseTarget)->Option<Self> {
        let (_,removed,_)=crate::command::retirement::close_facts(model,target)?;
        let root=*removed.first()?;
        let first_surface=removed.iter().find(|entity|entity.kind==IdKind::Surface)?.id;
        let tab=model.surfaces.get(&first_surface)?.tab;
        let pane=model.tabs.get(&tab)?.pane;
        let origin_workspace=(root.kind!=IdKind::Workspace).then_some(model.panes.get(&pane)?.workspace);
        let mut result=Self {version:1,root,origin_workspace,workspaces:Default::default(),panes:Default::default(),tabs:Default::default(),surfaces:Default::default(),tab_name:model.tabs.get(&tab).map(|tab|tab.name.clone())};
        for entity in removed {
            match entity.kind {
                IdKind::Workspace=>{result.workspaces.insert(entity.id,model.workspaces.get(&entity.id)?.clone());},
                IdKind::Pane=>{result.panes.insert(entity.id,model.panes.get(&entity.id)?.clone());},
                IdKind::Tab=>{result.tabs.insert(entity.id,model.tabs.get(&entity.id)?.clone());},
                IdKind::Surface=>{result.surfaces.insert(entity.id,model.surfaces.get(&entity.id)?.clone());},
                IdKind::Category=>return None,
            }
        }
        Some(result)
    }
    pub fn data_refs(&self)->Vec<DataRef> {
        self.surfaces.values().flat_map(|surface|surface.data.into_iter().chain(surface.creation_seed)).collect()
    }
}
