//! Pure target resolution for live commands that may wait for lazy resources.
use super::*;
#[derive(Clone)]
pub(crate) struct ChildTarget {pub surface:u32,parent:u32,index:u32,mirror:Option<(u32,std::sync::Weak<()>)>}
pub(crate) enum FixedInput {Broadcast(Vec<ChildTarget>),Respawn(ChildTarget)}
impl FixedInput {
    pub(crate) fn targets(&self)->Vec<u32> {match self {Self::Broadcast(targets)=>targets.iter().map(|target|target.surface).collect(),Self::Respawn(target)=>vec![target.surface]}}
    pub(crate) fn weight(&self)->usize {match self {Self::Broadcast(targets)=>targets.capacity()*std::mem::size_of::<ChildTarget>(),Self::Respawn(_)=>std::mem::size_of::<ChildTarget>()}}
    pub(crate) fn permits_partial(&self)->bool {matches!(self,Self::Broadcast(_))}
    pub(crate) fn execute(&self,core:&mut AppServices,engine:&mut EngineMut<'_>,request:&crate::ipc::protocol::JsonRpcRequest,bindings:&std::collections::BTreeMap<u32,Option<tasty_terminal::ResourceGeneration>>)->JsonRpcResponse {
        let id=request.id.clone().unwrap_or_default();
        let valid=|target:&ChildTarget,engine:&EngineMut<'_>|engine.runtime.child_terminals.find_child(target.parent,target.index).is_some_and(|entry|entry.child_surface_id==target.surface)
            && target.mirror.as_ref().is_none_or(|(workspace,token)|engine.matches_mirror_projection(*workspace,token))
            && bindings.get(&target.surface).copied().flatten().is_some_and(|generation|engine.runtime.terminals.matches_generation(target.surface,generation));
        match self {
            Self::Broadcast(targets)=> {
                let text=request.params.get("text").and_then(|value|value.as_str()).unwrap_or_default();
                let (body,submit)=build_broadcast_payload(text);
                let mut changed=false;
                for target in targets {
                    if valid(target,engine) && send_broadcast_to_child(core,engine,&id,target.surface,&body,submit) {changed|=clear_idle_for_new_prompt(&mut engine.runtime.child_terminals,target.surface);}
                }
                if changed {engine.runtime.child_terminals.save();}
                JsonRpcResponse::success(id,json!({"sent_count":targets.len(),"children":targets.iter().map(|target|target.surface).collect::<Vec<_>>()}))
            },
            Self::Respawn(target)=> {
                if !valid(target,engine) {return JsonRpcResponse::invalid_params(id,"child terminal target changed while awaiting activation");}
                let mut params=request.params.clone();params["surface"]=json!(target.parent);params["child"]=json!(target.index);
                handle_respawn(core,engine,id,&params)
            },
        }
    }
}
pub(crate) fn decode_fixed_input(engine:&EngineRef<'_>,request:&crate::ipc::protocol::JsonRpcRequest)->Result<Option<FixedInput>,JsonRpcResponse> {
    let id=request.id.clone().unwrap_or_default();
    if !matches!(request.method.as_str(),"terminal.broadcast"|"terminal.respawn") {return Ok(None);}
    let parent=resolve_parent(engine,&request.params,&id)?;
    let mirror=|surface|engine.find_workspace_index_for_surface(surface).and_then(|(index,_)|engine.workspace_at(index)).filter(|workspace|workspace.mirror).and_then(|workspace|engine.mirror_projection_token(workspace.id).map(|token|(workspace.id,token)));
    if request.method=="terminal.broadcast" {
        require_str(&request.params,"text",&id)?;
        let role=optional_str(&request.params,"role");
        let targets=engine.runtime.child_terminals.list_children(parent).iter().filter(|entry|role.as_ref().is_none_or(|role|entry.role.as_ref()==Some(role))).map(|entry|ChildTarget {surface:entry.child_surface_id,parent,index:entry.index,mirror:mirror(entry.child_surface_id)}).collect();
        return Ok(Some(FixedInput::Broadcast(targets)));
    }
    if request.params.get("cwd").is_some_and(|value|value.is_string()) {return Ok(None);}
    let index=require_u32(&request.params,"child",&id)?;
    let entry=engine.runtime.child_terminals.find_child(parent,index).ok_or_else(||JsonRpcResponse::invalid_params(id,child_not_found_message(&engine.runtime.child_terminals,parent,index)))?;
    Ok(Some(FixedInput::Respawn(ChildTarget {surface:entry.child_surface_id,parent,index,mirror:mirror(entry.child_surface_id)})))
}
