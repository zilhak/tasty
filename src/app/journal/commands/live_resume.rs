//! Bounded, process-local input continuation after committed lazy activation.
//! The journal only sees the activation request. Original input is never a durable replay payload.
use super::*;

pub(super) struct Resume {
    pub engine:EngineId,
    pub surface:u32,
    checked:crate::ipc::handler::OwnedCheckedRequest,
    reply:Return,
    pub generation:Option<tasty_terminal::ResourceGeneration>,
}
enum Return {Ipc(SyncSender<JsonRpcResponse>),Plugin {plugin:String,call:u64,binding:std::sync::Weak<()>}}
impl Return {
    fn current(&self,manager:Option<&crate::plugin::PluginManager>)->bool {match self {Self::Ipc(_)=>true,Self::Plugin {plugin,binding,..}=>manager.and_then(|manager|manager.processes.get(plugin)).is_some_and(|process|process.reply_binding().ptr_eq(binding))}}
    fn answer(self,response:JsonRpcResponse,manager:Option<&mut crate::plugin::PluginManager>) {
        match self {Self::Ipc(sender)=>crate::ipc::server::send_response(&sender,response),Self::Plugin {plugin,call,binding}=>if let Some(manager)=manager {manager.send_bound_ipc_result(&plugin,&binding,call,response);}}
    }
    fn as_reply(self)->Reply {match self {Self::Ipc(sender)=>Reply::Ipc(sender),Self::Plugin {plugin,call,binding}=>Reply::Plugin {plugin_id:plugin,call_id:call,binding}}}
}
impl Resume {
    pub fn weight(&self)->usize {self.checked.weight()+std::mem::size_of::<Self>()}
    pub fn execute(self,services:&mut crate::app::services::AppServices,state:&mut crate::state::RequestContext,engine:&mut crate::runtime::engine_access::EngineMut<'_>,activation:JsonRpcResponse,manager:Option<&mut crate::plugin::PluginManager>) {
        let id=self.checked.borrow().request().id.clone().unwrap_or_default();
        let response=if !self.reply.current(manager.as_deref()) {
            JsonRpcResponse::invalid_params(id,"input caller process was replaced while waiting")
        } else if let Some(error)=activation.error {
            JsonRpcResponse::error(id,error.code,error.message)
        } else if engine.live.occupancy.is_hard_occupied(self.surface) {
            JsonRpcResponse::invalid_params(id,"surface became hard-occupied before pending input could be applied")
        } else if self.generation.is_none_or(|generation|!engine.runtime.terminals.matches_generation(self.surface,generation)) {
            JsonRpcResponse::invalid_params(id,"surface resource changed before pending input could be applied")
        } else {
            crate::ipc::handler::handle_checked_request(services,state,engine,&self.checked.borrow())
        };
        self.reply.answer(response,manager);
    }
    pub fn error_reply(self,mut response:JsonRpcResponse)->(Reply,JsonRpcResponse) {
        response.id=self.checked.borrow().request().id.clone().unwrap_or_default();(self.reply.as_reply(),response)
    }
    pub fn reject(self,reason:&str,manager:Option<&mut crate::plugin::PluginManager>) {
        let id=self.checked.borrow().request().id.clone().unwrap_or_default();
        self.reply.answer(JsonRpcResponse::internal_error(id,reason),manager);
    }
}

pub(crate) fn target(request:&JsonRpcRequest,engine:&crate::runtime::engine_access::EngineRef<'_>)->Option<u32> {
    if !matches!(request.method.as_str(),"surface.send"|"surface.send_key"|"surface.send_combo"|"surface.send_wait_idle"|"surface.send_to"|"terminal.tell") {return None;}
    let (surface,_)=if request.method=="terminal.tell" {crate::ipc::handler::terminal::decode_tell(&request.params,&serde_json::Value::Null).ok()?}else {crate::ipc::handler::surface::decode_input_header(&request.method,&request.params,&serde_json::Value::Null).ok()?};
    // Preserve the cheap refusal before any materialization work.
    if engine.live.occupancy.is_hard_occupied(surface) || (request.method=="surface.send_wait_idle" && engine.is_typing(surface)) {return None;}
    if engine.runtime.terminals.contains(surface) {return None;}
    engine.runtime.surfaces.get(&surface)?.as_any().downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
        .filter(|placeholder|placeholder.kind=="terminal").map(|_|surface)
}
impl JournalApplication {
    /// Original live-input key arbitration precedes resolving an implicit resource or starting it.
    pub(crate) fn defer_live_ipc(&mut self,command:&crate::ipc::server::IpcCommand,checked:&crate::ipc::handler::CheckedRequest<'_>,id:EngineId,engine:&crate::runtime::engine_access::EngineRef<'_>)->bool {
        if !matches!(command.request.method.as_str(),"surface.send"|"surface.send_key"|"surface.send_combo"|"surface.send_wait_idle"|"surface.send_to"|"terminal.tell") {return false;}
        if let Some(handled)=crate::ipc::handler::idempotency::run_app_layer(checked.caller(),command,true,|handled|*handled,|relayed|self.defer_live_unkeyed(relayed,checked,id,engine)) {return handled;}
        self.defer_live_unkeyed(command,checked,id,engine)
    }
    pub(crate) fn defer_live_unkeyed(&mut self,command:&crate::ipc::server::IpcCommand,checked:&crate::ipc::handler::CheckedRequest<'_>,id:EngineId,engine:&crate::runtime::engine_access::EngineRef<'_>)->bool {
        let Some(surface)=target(&command.request,engine) else{return false;};
        let activation=engine.core.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
        let request=JsonRpcRequest {jsonrpc:"2.0".into(),method:"intent.wake".into(),params:serde_json::json!({"surface_id":surface,"activation":activation}),id:None,idempotency_key:None,session_token:None,response_timeout_ms:None};
        let resume=Resume {engine:id,surface,checked:checked.to_owned_without_key(),reply:Return::Ipc(command.response_tx.clone()),generation:None};
        let receipt=self.creations.iter().filter(|((engine,_),_)|*engine==id).find_map(|(_,creation)|creation.join_restore(surface,activation));
        let ticket=self.next_ticket;
        self.admit_request(&request,Reply::Resume(resume),crate::ipc::handler::idempotency::caller_scope(checked.caller()),"live-input-activation");
        if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.activation_wait=receipt;}
        true
    }
    pub(crate) fn defer_plugin_input(&mut self,checked:&crate::ipc::handler::CheckedRequest<'_>,id:EngineId,engine:&crate::runtime::engine_access::EngineRef<'_>,call:&tasty_host_plugin::manager::PendingPluginCall,manager:Option<&crate::plugin::PluginManager>)->bool {
        let Some(surface)=target(checked.request(),engine) else{return false;};
        let Some(process)=manager.and_then(|manager|manager.processes.get(&call.plugin_id)) else{return false;};
        let activation=engine.core.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
        let request=JsonRpcRequest {jsonrpc:"2.0".into(),method:"intent.wake".into(),params:serde_json::json!({"surface_id":surface,"activation":activation}),id:None,idempotency_key:None,session_token:None,response_timeout_ms:None};
        let resume=Resume {engine:id,surface,checked:checked.to_owned_without_key(),reply:Return::Plugin {plugin:call.plugin_id.clone(),call:call.call_id,binding:process.reply_binding()},generation:None};
        let receipt=self.creations.iter().filter(|((engine,_),_)|*engine==id).find_map(|(_,creation)|creation.join_restore(surface,activation));
        let ticket=self.next_ticket;
        self.admit_request(&request,Reply::Resume(resume),crate::ipc::handler::idempotency::caller_scope(checked.caller()),"plugin-input-activation");
        if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.activation_wait=receipt;}true
    }
    pub(crate) fn finish_headless_live_inputs(&mut self,id:EngineId,services:&mut crate::app::services::AppServices,state:&mut crate::state::RequestContext,engine:&mut crate::runtime::engine_access::EngineMut<'_>,mut manager:Option<&mut crate::plugin::PluginManager>) {
        for (resume,response) in std::mem::take(&mut self.commands.completed_live) {
            if resume.engine==id {resume.execute(services,state,engine,response,manager.as_deref_mut());} else {resume.reject("pending input engine disappeared",manager.as_deref_mut());}
        }
    }
}
#[cfg(feature="gui")]
impl crate::app::App {
    pub(crate) fn defer_live_input(&mut self,command:&crate::ipc::server::IpcCommand,checked:&crate::ipc::handler::CheckedRequest<'_>)->bool {
        if !matches!(command.request.method.as_str(),"surface.send"|"surface.send_key"|"surface.send_combo"|"surface.send_wait_idle"|"surface.send_to"|"terminal.tell") {return false;}
        if let Some(handled)=crate::ipc::handler::idempotency::run_app_layer(checked.caller(),command,true,|handled|*handled,|relayed|self.defer_live_input_miss(relayed,checked)) {return handled;}
        self.defer_live_input_miss(command,checked)
    }
    fn defer_live_input_miss(&mut self,command:&crate::ipc::server::IpcCommand,checked:&crate::ipc::handler::CheckedRequest<'_>)->bool {
        let Some(surface)=command.request.params.get(if command.request.method=="terminal.tell" {"surface"}else{"surface_id"}).and_then(|value|value.as_u64()).and_then(|id|u32::try_from(id).ok()) else{return false;};
        let Some(session)=self.engines.all_sessions().find(|session|session.core_state.has_surface(surface)) else{return false;};
        self.journal.defer_live_unkeyed(command,checked,session.id,&session.as_ref())
    }
    pub(crate) fn defer_live_plugin(&mut self,checked:&crate::ipc::handler::CheckedRequest<'_>,call:&tasty_host_plugin::manager::PendingPluginCall)->bool {
        let Some(surface)=checked.request().params.get(if checked.request().method=="terminal.tell" {"surface"}else{"surface_id"}).and_then(|value|value.as_u64()).and_then(|id|u32::try_from(id).ok()) else{return false;};
        let Some(session)=self.engines.all_sessions().find(|session|session.core_state.has_surface(surface)) else{return false;};
        self.journal.defer_plugin_input(checked,session.id,&session.as_ref(),call,self.plugin_manager.as_ref())
    }
    pub(crate) fn finish_live_inputs(&mut self) {
        for (resume,response) in std::mem::take(&mut self.journal.commands.completed_live) {
            let services=&mut self.services;
            let manager=self.plugin_manager.as_mut();
            let Some(context)=crate::app::window_access::engines_mut!(self).resolve(resume.engine) else {resume.reject("pending input engine disappeared",manager);continue;};
            resume.execute(services,context.state,&mut context.engine,response,manager);
        }
    }
}
