//! Resolve close compatibility once, then commit its tombstone and exact cleanup obligation.
use super::*;
pub(super) struct Request {
    pub engine:EngineId,
    pub binding:crate::runtime::journal_product::EngineBinding,
    pub target:tasty_domain::CloseTarget,
    pub response:JsonRpcResponse,
    pub not_closed:JsonRpcResponse,
    pub is_user_close:bool,
    pub input_ref:Option<tasty_domain::DataRef>,
    pub replacement:bool,
}
impl Request {
    pub fn resolve(request:&JsonRpcRequest,session:&EngineSession)->Result<Self,JsonRpcResponse> {
        if request.method=="workspace.close" {return Self::workspace(request,session);}
        use tasty_domain::CloseTarget as T;
        use crate::ipc::handler::params;
        let id=serde_json::Value::Null;
        let bad=|reason:String|JsonRpcResponse::invalid_params(id.clone(),reason);
        let (field,target)=match request.method.as_str() {
            "tab.close" => ("tab_id",0),
            "pane.close" => ("pane_id",1),
            "surface.close"|"surface.close_self" => ("surface_id",2),
            _=>return Err(bad("unsupported close request".into())),
        };
        let value=params::opt_int::<u32>(&request.params,field,&id)?.ok_or_else(||bad(format!("Missing required '{field}' parameter")))?;
        let target=match target {0=>T::Tab(value),1=>T::Pane(value),_=>T::Surface(value)};
        let engine=session.as_ref();
        let core=engine.core;
        let (workspace,targets,closes_workspace)=match target {
            T::Tab(tab_id)=> {
                let pane=core.find_pane_for_tab(tab_id).and_then(|id|core.find_pane_by_id(id));
                let tab=pane.and_then(|pane|pane.tabs.iter().find(|tab|tab.id==tab_id));
                let workspace=pane.and_then(|pane|core.find_workspace_index_for_pane(pane.id)).and_then(|index|core.workspace_at(index));
                (workspace,tab.map(|tab|tab.all_surface_ids()).unwrap_or_default(),false)
            },
            T::Pane(pane_id)=> {
                let pane=core.find_pane_by_id(pane_id).ok_or_else(||bad(format!("Pane {pane_id} not found")))?;
                let workspace=core.find_workspace_index_for_pane(pane_id).and_then(|index|core.workspace_at(index));
                (workspace,pane.tabs.iter().flat_map(|tab|tab.all_surface_ids()).collect(),false)
            },
            T::Surface(surface)=> {
                let workspace=core.find_workspace_index_for_surface(surface).and_then(|(index,_)|core.workspace_at(index));
                let closes=workspace.is_some_and(|workspace|workspace.all_surface_ids()==vec![surface]);
                (workspace,core.has_surface(surface).then_some(surface).into_iter().collect(),closes)
            },
            T::Workspace(_)=>unreachable!("workspace resolver is separate"),
        };
        if let Some(caller)=crate::ipc::handler::caller_surface_id(&request.params)
            && request.method!="surface.close_self" && targets.contains(&caller) {
            return Err(bad(match target {
                T::Tab(_)=>"Cannot close a tab that contains your own surface. Use 'tasty close self' instead.",
                T::Pane(_)=>"Cannot close a pane that contains your own surface. Close all other surfaces in the pane first, then use 'tasty close self'.",
                _=>"Cannot close your own surface with 'close surface'. Use 'tasty close self' instead.",
            }.into()));
        }
        // Remote structural effects are admitted separately; a mirror is never a local journal fact.
        if workspace.is_some_and(|workspace|workspace.mirror) {return Err(bad("mirror close requires the bound remote structural effect".into()));}
        if let Some(surface)=targets.iter().find(|id|engine.live.occupancy.is_hard_occupied(**id)) {
            return Err(bad(format!("Surface {surface} is occupied by a remote attach session (hard-occupied) — someone is working in that terminal right now. Release it from the attaching instance first.")));
        }
        let mut success=serde_json::json!({"closed":true});success[field]=serde_json::json!(value);
        let mut no_op=success.clone();no_op["closed"]=serde_json::json!(false);
        no_op["reason"]=serde_json::json!(match target {T::Tab(_)=>"tab not found or cannot close the last tab",T::Pane(_)=>"cannot close the last pane",_=>"surface not found"});
        Ok(Self {
            engine:session.id,binding:session.journal_binding.clone().ok_or_else(||JsonRpcResponse::internal_error(id.clone(),"engine has no journal binding"))?,target,
            response:JsonRpcResponse::success(id.clone(),success),not_closed:JsonRpcResponse::success(id,no_op),is_user_close:false,input_ref:None,
            replacement:closes_workspace && core.workspaces().len()==1,
        })
    }
    pub fn workspace(request:&JsonRpcRequest,session:&EngineSession)->Result<Self,JsonRpcResponse> {
        use crate::ipc::handler::params;
        let engine=session.as_ref();let id=serde_json::Value::Null;
        let bad=|reason:String|JsonRpcResponse::invalid_params(id.clone(),reason);
        let explicit=params::optional_u32(&request.params,"id",&id)?;
        let index=if let Some(workspace)=explicit {
            engine.find_workspace_index_for_id(workspace).ok_or_else(||bad(format!("Workspace {workspace} not found")))?
        } else if let Some(index)=params::opt_int::<u64>(&request.params,"index",&id)? {
            usize::try_from(index).map_err(|_|bad("Workspace index out of range".into()))?
        } else {return Err(bad("Missing required 'id' or 'index' parameter".into()));};
        let workspace=engine.workspace_at(index).ok_or_else(||bad(format!("Workspace index {index} out of range (0..{})",engine.workspaces().len())))?;
        if let Some(caller)=crate::ipc::handler::caller_surface_id(&request.params)
            && workspace.all_surface_ids().contains(&caller) {
            return Err(bad("Cannot close a workspace that contains your own surface. Move elsewhere first, or use 'tasty close self' to close just your surface.".into()));
        }
        if workspace.mirror {return Err(bad("Workspace is a mirror of a remote attach session — detach it from that session instead of closing it here".into()));}
        if let Some(surface)=workspace.all_surface_ids().into_iter().find(|id|engine.live.occupancy.is_hard_occupied(*id)) {
            return Err(bad(format!("Workspace holds surface {surface}, which is occupied by a remote attach session (hard-occupied) — someone is working in that terminal right now. Release it from the attaching instance first.")));
        }
        if engine.workspaces().len()==1 {return Err(bad(crate::ipc::handler::workspace::last_workspace_refusal().into()));}
        Ok(Self {engine:session.id,binding:session.journal_binding.clone().ok_or_else(||JsonRpcResponse::internal_error(id.clone(),"engine has no journal binding"))?,target:tasty_domain::CloseTarget::Workspace(workspace.id),response:JsonRpcResponse::success(id.clone(),serde_json::json!({"closed":true,"id":workspace.id})),not_closed:JsonRpcResponse::success(id,serde_json::json!({"closed":false,"id":workspace.id})),is_user_close:false,input_ref:None,replacement:false})
    }
    pub fn input(&self)->Work {Work::PutPayload(serde_json::to_vec(&self.target).expect("close target serializes"))}
    pub fn stored(&self,input:tasty_domain::DataRef)->Work {
        Work::Resolve {changes:vec![StreamCommand {stream:self.binding.stream.clone(),command:tasty_domain::StructuralCommand::Close {
            operation:tasty_domain::OperationId(String::new()),command_id:String::new(),input,target:self.target,undo:None,is_user_close:self.is_user_close,
        }}],response:Some(ResponsePlan::Closed {success:self.response.clone(),not_closed:self.not_closed.clone()})}
    }
    pub fn weight(&self)->usize {self.binding.stream.len()+serde_json::to_vec(&(&self.response,&self.not_closed)).map_or(usize::MAX,|bytes|bytes.len())}

}
impl JournalApplication {
    pub(super) fn resolve_close(&mut self,ticket:u64,session:&EngineSession) {
        let Some(pending)=self.commands.pending.get(&ticket) else{return;};
        match Request::resolve(&pending.request,session) {
            Ok(request)=>{
                let pending=self.commands.pending.get_mut(&ticket).expect("resolved request remains pending");
                if request.replacement {
                    let spec=super::create_spec::Spec {
                        destination:super::create_spec::Destination::Workspace {name:Some("Workspace 1".into()),subtitle:None,description:None,category:Some(0)},
                        kind:"terminal".into(),cwd:None,params:serde_json::json!({}),
                    };
                    match create::Request::from_spec(spec,session) {
                        Ok(mut replacement)=>{replacement.activate=false;pending.resource=Some(replacement);},
                        Err(response)=>{self.reject_resolved_request(ticket,response);return;},
                    }
                }
                pending.request.params=serde_json::Value::Null;
                pending.queued=Some(request.input());pending.closing=Some(request);
                self.refresh_command_weight(ticket);
            },
            Err(response)=>self.reject_resolved_request(ticket,response),
        }
        (self.wake)();
    }
}
