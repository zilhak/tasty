//! Resolve close compatibility once, then commit its tombstone and exact cleanup obligation.
use super::*;
#[derive(Clone,Copy,Default)]
pub(super) enum Cause {#[default] Ordinary,ProcessExit(tasty_terminal::ResourceGeneration)}
pub(super) struct Request {
    pub engine:EngineId,
    pub binding:crate::runtime::journal_product::EngineBinding,
    pub target:tasty_domain::CloseTarget,
    pub response:JsonRpcResponse,
    pub not_closed:JsonRpcResponse,
    pub is_user_close:bool,
    pub input_ref:Option<tasty_domain::DataRef>,
    pub replacement:bool,
    pub undo:Option<tasty_domain::UndoCapture>,
    pub expected:Vec<tasty_domain::RetiredSurface>,
    pub capture_undo:bool,
}
impl Request {
    pub fn resolve(request:&JsonRpcRequest,session:&EngineSession,cause:Cause)->Result<Self,JsonRpcResponse> {
        if request.method=="intent.close" {
            let target:tasty_domain::CloseTarget=serde_json::from_value(request.params["target"].clone()).map_err(|error|JsonRpcResponse::invalid_params(serde_json::Value::Null,error.to_string()))?;
            let mut normalized=request.clone();
            let (method,field,value)=match target {
                tasty_domain::CloseTarget::Workspace(id)=>("workspace.close","id",id),
                tasty_domain::CloseTarget::Pane(id)=>("pane.close","pane_id",id),
                tasty_domain::CloseTarget::Tab(id)=>("tab.close","tab_id",id),
                tasty_domain::CloseTarget::Surface(id)=>("surface.close_self","surface_id",id),
            };
            normalized.method=method.into();normalized.params=serde_json::json!({});normalized.params[field]=serde_json::json!(value);
            let mut resolved=if matches!(target,tasty_domain::CloseTarget::Workspace(_)) {Self::workspace(&normalized,session,true)?} else {Self::resolve(&normalized,session,cause)?};
            if let Some(expected)=request.params.get("expected_activation") {
                let expected:Option<u64>=serde_json::from_value(expected.clone()).map_err(|error|JsonRpcResponse::invalid_params(serde_json::Value::Null,error.to_string()))?;
                if resolved.expected.first().is_none_or(|target|target.activation_generation!=expected) {return Err(JsonRpcResponse::invalid_params(serde_json::Value::Null,"close belongs to a retired surface activation"));}
            }
            resolved.capture_undo=request.params["capture"].as_bool().unwrap_or(false);
            resolved.is_user_close=request.params["user_close"].as_bool().unwrap_or(false);
            return Ok(resolved);
        }
        if request.method=="workspace.close" {return Self::workspace(request,session,false);}
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
        if let Cause::ProcessExit(generation)=cause {
            if !matches!(target,T::Surface(surface) if engine.runtime.terminals.matches_generation(surface,generation)) {
                return Err(bad("PTY exit belongs to a retired physical owner".into()));
            }
        }
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
        if matches!(cause,Cause::Ordinary) && let Some(surface)=targets.iter().find(|id|engine.live.occupancy.is_hard_occupied(**id)) {
            return Err(bad(format!("Surface {surface} is occupied by a remote attach session (hard-occupied) — someone is working in that terminal right now. Release it from the attaching instance first.")));
        }
        let mut success=serde_json::json!({"closed":true});success[field]=serde_json::json!(value);
        let mut no_op=success.clone();no_op["closed"]=serde_json::json!(false);
        no_op["reason"]=serde_json::json!(match target {T::Tab(_)=>"tab not found or cannot close the last tab",T::Pane(_)=>"cannot close the last pane",_=>"surface not found"});
        Ok(Self {
            engine:session.id,binding:session.journal_binding.clone().ok_or_else(||JsonRpcResponse::internal_error(id.clone(),"engine has no journal binding"))?,target,
            response:JsonRpcResponse::success(id.clone(),success),not_closed:JsonRpcResponse::success(id,no_op),is_user_close:false,input_ref:None,
            replacement:closes_workspace && core.workspaces().len()==1,undo:None,expected:targets.into_iter().filter_map(|id|core_expected(session,id)).collect(),capture_undo:false,
        })
    }
    pub fn workspace(request:&JsonRpcRequest,session:&EngineSession,allow_last:bool)->Result<Self,JsonRpcResponse> {
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
        if !allow_last && engine.workspaces().len()==1 {return Err(bad(crate::ipc::handler::workspace::last_workspace_refusal().into()));}
        Ok(Self {engine:session.id,binding:session.journal_binding.clone().ok_or_else(||JsonRpcResponse::internal_error(id.clone(),"engine has no journal binding"))?,target:tasty_domain::CloseTarget::Workspace(workspace.id),response:JsonRpcResponse::success(id.clone(),serde_json::json!({"closed":true,"id":workspace.id})),not_closed:JsonRpcResponse::success(id,serde_json::json!({"closed":false,"id":workspace.id})),is_user_close:false,input_ref:None,replacement:false,undo:None,expected:workspace.all_surface_ids().into_iter().filter_map(|id|core_expected(session,id)).collect(),capture_undo:false})
    }
    pub fn input(&self,session:&EngineSession,view:Option<&crate::runtime::journal_product::CompletionView>)->Result<Work,String> {
        if !self.capture_undo {return Ok(Work::PutPayload(serde_json::to_vec(&self.target).map_err(|error|error.to_string())?));}
        let core=&session.core_state;
        let selected:std::collections::HashSet<_>=match self.target {
            tasty_domain::CloseTarget::Workspace(id)=>core.find_workspace_index_for_id(id).and_then(|index|core.workspace_at(index)).map(|workspace|workspace.all_surface_ids()).unwrap_or_default(),
            tasty_domain::CloseTarget::Pane(id)=>core.find_pane_by_id(id).map(|pane|pane.tabs.iter().flat_map(|tab|tab.all_surface_ids()).collect()).unwrap_or_default(),
            tasty_domain::CloseTarget::Tab(id)=>core.find_pane_for_tab(id).and_then(|pane|core.find_pane_by_id(pane)).and_then(|pane|pane.tabs.iter().find(|tab|tab.id==id)).map(|tab|tab.all_surface_ids()).unwrap_or_default(),
            tasty_domain::CloseTarget::Surface(id)=>crate::app::services::locate_surface_in_pane(core,id).and_then(|location|core.workspace_at(location.ws_idx)).map(|workspace| {
                // Closing its last leaf cascades through the containing structures, but all removed
                // leaves still consist of this one ID.
                workspace.all_surface_ids().into_iter().filter(|surface|*surface==id).collect()
            }).unwrap_or_default(),
        }.into_iter().collect();
        let display_name=if let tasty_domain::CloseTarget::Surface(id)=self.target {
            core.find_tab_for_surface(id).and_then(|tab|core.find_pane_for_tab(tab).and_then(|pane|core.find_pane_by_id(pane)).and_then(|pane|pane.tabs.iter().find(|candidate|candidate.id==tab))).map(|tab|tab.display_name(view.and_then(|view|view.selected_surfaces.get(&tab.id).copied()).or_else(||tab.first_surface_id())))
        } else {None};
        Ok(Work::CaptureClosed {view:view.cloned().unwrap_or_default(),binding:self.binding.clone(),target:self.target,display_name,surfaces:crate::runtime::surface_capture::capture_selected(session,Some(&selected))?})
    }
    pub fn stored(&self,input:tasty_domain::DataRef)->Work {
        Work::Resolve {changes:vec![StreamCommand {stream:self.binding.stream.clone(),command:tasty_domain::StructuralCommand::Close {
            operation:tasty_domain::OperationId(String::new()),command_id:String::new(),input,target:self.target,expected:self.expected.clone(),undo:self.undo.clone(),is_user_close:self.is_user_close,
        }}],response:Some(ResponsePlan::Closed {success:self.response.clone(),not_closed:self.not_closed.clone()})}
    }
    pub fn weight(&self)->usize {self.binding.stream.len()+serde_json::to_vec(&(&self.response,&self.not_closed,&self.expected,&self.undo)).map_or(usize::MAX,|bytes|bytes.len())}

}
impl JournalApplication {
    pub(super) fn resolve_close(&mut self,ticket:u64,session:&EngineSession) {
        let Some(pending)=self.commands.pending.get(&ticket) else{return;};
        match Request::resolve(&pending.request,session,pending.close_cause) {
            Ok(mut request)=>{
                if !matches!(&pending.reply,Reply::Intent {..}) {request.is_user_close=false;request.capture_undo=false;}
                else if matches!(&pending.reply,Reply::Intent {origin,..} if origin.is_user()) {request.is_user_close=true;}
                let work=match request.input(session,self.completion_views.get(&session.id)) {Ok(work)=>work,Err(error)=>{self.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,error));return;}};
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
                pending.queued=Some(work);pending.closing=Some(request);
                self.refresh_command_weight(ticket);
            },
            Err(response)=>self.reject_resolved_request(ticket,response),
        }
        (self.wake)();
    }
}

fn core_expected(session:&EngineSession,id:u32)->Option<tasty_domain::RetiredSurface> {
    let surface=session.core_state.find_surface_by_id(id)?;
    Some(tasty_domain::RetiredSurface {id,kind:surface.kind.clone(),activation_generation:surface.activation_generation})
}
