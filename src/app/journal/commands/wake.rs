//! Explicit lazy activation joins the ordinary committed resource lifecycle.
use super::*;
impl JournalApplication {
    pub(super) fn resolve_wake(&mut self,ticket:u64,session:&EngineSession) {
        let Some(pending)=self.commands.pending.get(&ticket) else{return;};
        let surface=match crate::ipc::handler::params::require_u32(&pending.request.params,"surface_id",&serde_json::Value::Null) {
            Ok(surface)=>surface,Err(error)=>{self.reject_resolved_request(ticket,error);return;},
        };
        if let Some(receipt)=pending.activation_wait.clone() {
            match receipt.get() {
                None=>{if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.needs_resolution=true;}},
                Some(super::super::creation::ActivationOutcome::Failed(reason))=>self.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,reason.clone())),
                Some(super::super::creation::ActivationOutcome::Ready {activation,physical})=> {
                    let current=session.core_state.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
                    if current!=*activation || physical.is_none_or(|generation|!session.runtime.terminals.matches_generation(surface,generation)) {
                        self.reject_resolved_request(ticket,JsonRpcResponse::invalid_params(serde_json::Value::Null,"joined activation was replaced before completion"));
                    } else {
                        if let Some(pending)=self.commands.pending.get_mut(&ticket) && let Reply::Resume(resume)=&mut pending.reply {resume.generation=*physical;}
                        self.reject_resolved_request(ticket,JsonRpcResponse::success(serde_json::Value::Null,serde_json::json!({"woke":false,"surface_id":surface,"pty_ready":true})));
                    }
                },
            }
            return;
        }
        if let Some(expected)=pending.request.params.get("activation") {
            let current=session.core_state.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
            if serde_json::json!(current)!=*expected {
                self.reject_resolved_request(ticket,JsonRpcResponse::invalid_params(serde_json::Value::Null,"surface activation changed while waiting"));return;
            }
        }
        let activation=session.core_state.find_surface_by_id(surface).and_then(|surface|surface.activation_generation);
        if let Some(receipt)=self.creations.iter().filter(|((engine,_),_)|*engine==session.id).find_map(|(_,creation)|creation.join_restore(surface,activation)) {
            if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.activation_wait=Some(receipt);pending.needs_resolution=true;}
            return;
        }
        if self.has_creation(session.id) {
            if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.needs_resolution=true;}
            return;
        }
        if session.runtime.terminals.contains(surface) {
            self.reject_resolved_request(ticket,JsonRpcResponse::success(serde_json::Value::Null,serde_json::json!({"woke":false,"surface_id":surface,"pty_ready":true})));return;
        }
        let terminal=session.runtime.surfaces.get(&surface).and_then(|surface|surface.as_any().downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()).is_some_and(|placeholder|placeholder.kind=="terminal");
        if !terminal {
            self.reject_resolved_request(ticket,JsonRpcResponse::invalid_params(serde_json::Value::Null,format!("Surface {surface} not found")));return;
        }
        let index=self.restoration_ready.get(&session.id).and_then(|requests|requests.iter().position(|request|request.surface_id==surface));
        let Some(index)=index else {
            let reading=self.restoration_queue.iter().any(|(engine,request)|*engine==session.id && request.surface_id==surface)
                || self.restoration_reads.values().any(|(engine,request)|*engine==session.id && request.surface_id==surface);
            if reading {if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.needs_resolution=true;}}
            else {self.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,"surface restoration has no pending preparation; its previous activation failed or requires recovery"));}
            return;
        };
        let restore=self.restoration_ready[&session.id][index].clone();
        let mut resource=match create::Request::base(session,"terminal",&serde_json::json!({}),restore.input.cwd.clone()) {
            Ok(resource)=>resource,Err(error)=>{self.reject_resolved_request(ticket,error);return;},
        };
        resource.plan=restore.plan;resource.input=Some(restore.input);resource.shape=create::Shape::Wake;resource.activate=false;
        let work=match resource.reservation() {Ok(work)=>work,Err(error)=>{self.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,error));return;}};
        if let Some(pending)=self.commands.pending.get_mut(&ticket) {pending.resource=Some(resource);pending.queued=Some(work);pending.request.params=serde_json::Value::Null;}
        self.refresh_command_weight(ticket);(self.wake)();
    }
}
