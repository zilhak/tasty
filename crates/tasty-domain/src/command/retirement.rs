use super::*;
use crate::{CloseTarget,RetirementPlan,RetiredSurface,EntityId,IdKind,Operation,OperationOutcome,StructuralEffect};
pub(super) fn decide(model:&JournalModel,command:&StructuralCommand)->Result<StructuralDecision,Rejection> {
    match command {
        StructuralCommand::Close {operation,command_id,input,target,expected,undo,is_user_close}=>{
            let Some((event,removed,surfaces))=close_facts(model,*target) else {
                return Ok(StructuralDecision {events:Vec::new(),effects:Vec::new(),result:StructuralResult::Closed {closed:false},completed_command:None});
            };
            let surfaces=surfaces.into_iter().map(|id| {
                let surface=model.surfaces.get(&id).ok_or_else(||Rejection(format!("surface {id} is missing")))?;
                Ok(RetiredSurface {id,kind:surface.kind.clone(),activation_generation:surface.activation.map(|activation|activation.generation)})
            }).collect::<Result<Vec<_>,Rejection>>()?;
            if &surfaces!=expected {return Err(Rejection("close target instance changed before commit".into()));}
            let tab_parents=removed.iter().filter(|entity|entity.kind==IdKind::Tab).map(|entity| {
                let pane=model.panes.iter().find(|(_,pane)|pane.tabs.contains(&entity.id)).map(|(id,_)|*id)
                    .ok_or_else(||Rejection("closed tab has no parent".into()))?;
                Ok((entity.id,pane))
            }).collect::<Result<Vec<_>,Rejection>>()?;
            let plan=RetirementPlan {target:*target,removed:removed.clone(),surfaces,undo:undo.clone(),tab_parents,is_user_close:*is_user_close};
            let record=Operation {id:operation.clone(),command_id:command_id.clone(),engine_incarnation:model.engine_incarnation,creation:None,assembly:None,retirement:Some(plan.clone()),targets:removed,reserved:Vec::new(),input:*input,activation_generation:0,outcome:None,pending_outcome:None,cleanup:None,prepared_data:None,prepared_deferred:false,reconciliation_evidence:None};
            let mut events=vec![DomainEvent::OperationPrepared {operation:record}];
            if let Some(capture)=undo {
                if !is_user_close {return Err(Rejection("only user close can append an undo record".into()));}
                if model.undo_records.len()>=10 {
                    events.push(DomainEvent::UndoRecordEvicted {id:model.undo_records[0].id.clone()});
                }
                events.push(DomainEvent::UndoRecordAdded {record:crate::UndoRecord {id:operation.clone(),target:*target,capture:capture.clone()}});
            }
            events.push(event);
            Ok(StructuralDecision {
                events,
                effects:vec![StructuralEffect::RetireSurfaces {operation:operation.clone(),plan}],
                result:StructuralResult::Pending {operation:operation.clone()},completed_command:None,
            })
        }
        StructuralCommand::FinishRetirement {operation,outcome}=>{
            let record=model.operations.get(operation).ok_or_else(||Rejection("retirement operation missing".into()))?;
            if record.retirement.is_none() || record.outcome.is_some() {return Err(Rejection("retirement cannot be completed from this state".into()));}
            let result=match outcome {
                OperationOutcome::Succeeded=>StructuralResult::Closed {closed:true},
                OperationOutcome::Uncertain {reason}=>StructuralResult::Failed {reason:reason.clone()},
                _=>return Err(Rejection("committed close cleanup requires success or explicit uncertainty".into())),
            };
            Ok(StructuralDecision {events:vec![DomainEvent::OperationFinished {id:operation.clone(),outcome:outcome.clone()}],effects:Vec::new(),result,completed_command:Some(record.command_id.clone())})
        }
        _=>Err(Rejection("not a retirement command".into())),
    }
}

pub(crate) fn close_facts(model:&JournalModel,target:CloseTarget)->Option<(DomainEvent,Vec<EntityId>,Vec<u32>)> {
    let entity=|kind,id|EntityId {kind,id};
    let tab_parent=|id|model.panes.iter().find(|(_,pane)|pane.tabs.contains(&id)).map(|(id,pane)|(*id,pane));
    let pane_parent=|id|model.workspaces.iter().find(|(_,workspace)|workspace.layout.leaves().contains(&id)).map(|(id,workspace)|(*id,workspace));
    let actual=match target {
        CloseTarget::Tab(id)=> {let (_,pane)=tab_parent(id)?;if pane.tabs.len()<=1 {return None;} target},
        CloseTarget::Pane(id)=> {let (_,workspace)=pane_parent(id)?;if workspace.layout.leaves().len()<=1 {return None;} target},
        CloseTarget::Workspace(id)=> {model.workspaces.get(&id)?;target},
        CloseTarget::Surface(id)=> {
            let (tab_id,tab)=model.tabs.iter().find(|(_,tab)|tab.layout.leaves().contains(&id))?;
            if tab.layout.leaves().len()>1 {target} else {
                let (pane_id,pane)=tab_parent(*tab_id)?;
                if pane.tabs.len()>1 {CloseTarget::Tab(*tab_id)} else {
                    let (workspace_id,workspace)=pane_parent(pane_id)?;
                    if workspace.layout.leaves().len()>1 {CloseTarget::Pane(pane_id)} else {CloseTarget::Workspace(workspace_id)}
                }
            }
        },
    };
    let mut removed=Vec::new();let mut surfaces=Vec::new();
    let event=match actual {
        CloseTarget::Surface(id)=>{removed.push(entity(IdKind::Surface,id));surfaces.push(id);DomainEvent::SurfaceClosed {id}},
        CloseTarget::Tab(id)=>{collect_tab(model,id,&mut removed,&mut surfaces)?;DomainEvent::TabClosed {id}},
        CloseTarget::Pane(id)=>{collect_pane(model,id,&mut removed,&mut surfaces)?;DomainEvent::PaneClosed {id}},
        CloseTarget::Workspace(id)=>{
            removed.push(entity(IdKind::Workspace,id));
            for pane in model.workspaces.get(&id)?.layout.leaves() {collect_pane(model,pane,&mut removed,&mut surfaces)?;}
            DomainEvent::WorkspaceClosed {id}
        },
    };
    Some((event,removed,surfaces))
}
fn collect_tab(model:&JournalModel,id:u32,removed:&mut Vec<EntityId>,surfaces:&mut Vec<u32>)->Option<()> {
    removed.push(EntityId {kind:IdKind::Tab,id});
    for id in model.tabs.get(&id)?.layout.leaves() {removed.push(EntityId {kind:IdKind::Surface,id});surfaces.push(id);}
    Some(())
}
fn collect_pane(model:&JournalModel,id:u32,removed:&mut Vec<EntityId>,surfaces:&mut Vec<u32>)->Option<()> {
    removed.push(EntityId {kind:IdKind::Pane,id});
    for tab in &model.panes.get(&id)?.tabs {collect_tab(model,*tab,removed,surfaces)?;}
    Some(())
}
