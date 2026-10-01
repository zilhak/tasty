//! Map only explicit mirror targets after original-key lookup. No local journal tree is mutated.
use super::*;
use tasty_ipc::stream::{StructuralOp,SplitAxis};

fn local_op(request:&JsonRpcRequest,session:&EngineSession,services:&crate::app::services::AppServices,view:&crate::runtime::journal_product::CompletionView)->Option<StructuralOp> {
    let core=&session.core_state;
    let pane_anchor=|id|core.find_pane_by_id(id).and_then(|pane|pane.tabs.first()).and_then(|tab|tab.first_surface_id());
    let tab_anchor=|id|core.find_pane_for_tab(id).and_then(|pane|core.find_pane_by_id(pane)).and_then(|pane|pane.tabs.iter().find(|tab|tab.id==id)).and_then(|tab|tab.first_surface_id());
    let id=|field|request.params.get(field).and_then(|value|value.as_u64()).and_then(|id|u32::try_from(id).ok());
    match request.method.as_str() {
        "intent.remote-structural"=>serde_json::from_value(request.params["op"].clone()).ok(),
        "surface.close"|"surface.close_self"=>Some(StructuralOp::CloseSurface {surface_id:id("surface_id")?}),
        "tab.close"=>Some(StructuralOp::CloseTab {anchor_surface_id:tab_anchor(id("tab_id")?)?}),
        "pane.close"=>Some(StructuralOp::ClosePane {anchor_surface_id:pane_anchor(id("pane_id")?)?}),
        "intent.close"=>match serde_json::from_value(request.params["target"].clone()).ok()? {
            tasty_domain::CloseTarget::Surface(id)=>Some(StructuralOp::CloseSurface {surface_id:id}),
            tasty_domain::CloseTarget::Tab(id)=>Some(StructuralOp::CloseTab {anchor_surface_id:tab_anchor(id)?}),
            tasty_domain::CloseTarget::Pane(id)=>Some(StructuralOp::ClosePane {anchor_surface_id:pane_anchor(id)?}),_=>None,
        },
        "intent.restore-closed"=>Some(StructuralOp::RestoreClosedItem {anchor_surface_id:pane_anchor(id("pane")?)?}),
        "tab.move"=>Some(StructuralOp::MoveTab {anchor_surface_id:pane_anchor(id("pane_id")?)?,from_index:request.params["from_index"].as_u64()?.try_into().ok()?,to_index:request.params["to_index"].as_u64()?.try_into().ok()?}),
        "tab.create"|"split"|"intent.create"=> {
            let spec=if request.method=="intent.create" {serde_json::from_value::<super::create_spec::Spec>(request.params.clone()).ok()?}else {super::create_spec::Spec::from_public(request,session,view,services).ok()?};
            let axis=|direction|match direction {crate::model::SplitDirection::Horizontal=>SplitAxis::Horizontal,crate::model::SplitDirection::Vertical=>SplitAxis::Vertical};
            match spec.destination {
                super::create_spec::Destination::Tab {pane,..}=>Some(StructuralOp::NewTab {anchor_surface_id:pane_anchor(pane)?,surface_kind:spec.kind,params:spec.params}),
                super::create_spec::Destination::Pane {target,direction}=>Some(StructuralOp::SplitPane {anchor_surface_id:pane_anchor(target)?,direction:axis(direction),surface_kind:spec.kind,params:spec.params}),
                super::create_spec::Destination::Surface {target,direction}=>Some(StructuralOp::SplitSurface {surface_id:target,direction:axis(direction),surface_kind:spec.kind,params:spec.params}),
                super::create_spec::Destination::Convert {surface,respawn:false}=>Some(StructuralOp::ConvertSurface {surface_id:surface,surface_kind:spec.kind,params:spec.params,cwd:spec.cwd.map(|path|path.to_string_lossy().into_owned())}),_=>None,
            }
        },
        _=>None,
    }
}
impl JournalApplication {
    pub(crate) fn admit_remote_intent(&mut self,engine:EngineId,op:StructuralOp,origin:&crate::intent::IntentOrigin,view:Option<IntentViewContinuation>,candidates:Vec<u32>) {
        self.admit_intent_request(engine,"intent.remote-structural",serde_json::json!({"op":op,"close_focus_candidates":candidates}),origin,view);
    }
    pub(crate) fn prepare_forward(&mut self,ticket:u64,draft:super::super::forward::Draft) {
        let Some(pending)=self.commands.pending.get_mut(&ticket) else{return;};
        pending.forward_reserved=draft.weight().saturating_mul(2);
        pending.queued=Some(Work::PutPayload(draft.payload.clone()));pending.forward=Some(draft);pending.request.params=serde_json::Value::Null;
        self.refresh_command_weight(ticket);(self.wake)();
    }
}
impl crate::app::App {
    pub(crate) fn try_resolve_mirror_request(&mut self,ticket:u64,engine:EngineId,request:&JsonRpcRequest)->bool {
        let Some(session)=self.engines.get(engine) else{return false;};
        let view=self.journal.completion_views.get(&engine).cloned().unwrap_or_default();
        let Some(op)=local_op(request,session,&self.services,&view) else{return false;};
        let mirrored=session.core_state.find_workspace_index_for_surface(op.anchor_surface_id()).and_then(|(index,_)|session.core_state.workspace_at(index)).is_some_and(|workspace|workspace.mirror);
        if !mirrored {return false;}
        if let Some(response)=super::close::caller_refusal(request,&session.core_state) {self.journal.reject_resolved_request(ticket,response);return true;}
        let user=self.journal.commands.pending.get(&ticket).is_some_and(|pending|matches!(&pending.reply,Reply::Intent {origin,..} if origin.is_user()));
        let candidates=serde_json::from_value::<Vec<u32>>(request.params["close_focus_candidates"].clone()).unwrap_or_default();
        match self.prepare_journal_forward(engine,&op,user,&candidates) {
            Ok(draft)=>self.journal.prepare_forward(ticket,draft),
            Err(error)=>self.journal.reject_resolved_request(ticket,JsonRpcResponse::internal_error(serde_json::Value::Null,error.to_string())),
        }
        true
    }
}
