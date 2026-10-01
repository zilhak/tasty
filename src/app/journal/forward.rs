//! A committed remote enqueue keeps its exact connection lease until the result is durable.
use super::*;
use crate::runtime::journal_product::EffectLease;
use tasty_domain::{OperationId,OperationOutcome};

pub(crate) struct Draft {
    pub engine:EngineId,
    pub stream:String,
    pub workspace_index:usize,
    pub response:crate::ipc::protocol::JsonRpcResponse,
    pub target:crate::app::attach_client::RemoteTarget,
    pub local_anchor:u32,
    pub remote_anchor:u32,
    pub op_id:u64,
    pub payload:Vec<u8>,
    pub focus:Option<tasty_remote::client_session::PendingOpFocus>,
    pub silent_failure:bool,
}
impl Draft {
    pub fn weight(&self)->usize {serde_json::to_vec(&self.response).map_or(0,|bytes|bytes.len())+self.payload.capacity()+self.stream.len()+self.focus.as_ref().map_or(0,|focus|match focus {tasty_remote::client_session::PendingOpFocus::Close {candidates}=>candidates.capacity()*4,_=>0})+std::mem::size_of::<Self>()}
}
pub(super) struct Forward {
    pub engine:EngineId,
    draft:Option<Draft>,
    queued:Option<Work>,
    stage:Phase,
}
enum Phase {Claim,Ready {lease:EffectLease,payload:Vec<u8>},Sending {lease:EffectLease},Finish}
impl JournalApplication {
    pub(super) fn start_forward(&mut self,draft:Draft,operation:OperationId)->Result<(),String> {
        let ticket=self.next_ticket;self.next_ticket=self.next_ticket.checked_add(1).ok_or("journal ticket exhausted")?;
        self.forwards.insert(ticket,Forward {engine:draft.engine,queued:Some(Work::ClaimForward {stream:draft.stream.clone(),operation}),draft:Some(draft),stage:Phase::Claim});
        (self.wake)();Ok(())
    }
    pub(super) fn submit_forwards(&mut self)->Result<(),String> {
        for (ticket,forward) in &mut self.forwards {
            let Some(work)=forward.queued.take() else {continue;};
            match self.worker.submit_owned(Request {ticket:*ticket,work}) {
                Ok(())=>{},Err((crate::runtime::journal_product::SubmitError::Busy,request))=>forward.queued=Some(request.work),
                Err((error,_))=>return Err(format!("committed remote submission failed: {error:?}")),
            }
        }
        Ok(())
    }
    pub(super) fn answer_forward(&mut self,ticket:u64,result:&Result<ResultValue,String>)->Result<bool,String> {
        let Some(forward)=self.forwards.get_mut(&ticket) else {return Ok(false);};
        match (&forward.stage,result.as_ref().map_err(Clone::clone)?) {
            (Phase::Claim,ResultValue::ForwardClaimed {lease,payload})=>forward.stage=Phase::Ready {lease:lease.clone(),payload:payload.clone()},
            (Phase::Finish,ResultValue::Executed(_)|ResultValue::Stored(_))=>{self.forwards.remove(&ticket);self.refresh_in_progress_commands();},
            _=>return Err("remote effect result is outside its admitted phase".into()),
        }
        (self.wake)();Ok(true)
    }
    pub(crate) fn take_ready_forwards(&mut self)->Vec<(u64,Draft,Vec<u8>)> {
        let mut out=Vec::new();
        for (ticket,forward) in &mut self.forwards {
            if let Phase::Ready {lease,payload}=&mut forward.stage {
                let lease=lease.clone();let payload=std::mem::take(payload);
                if let Some(draft)=forward.draft.take() {out.push((*ticket,draft,payload));forward.stage=Phase::Sending {lease};}
            }
        }
        out
    }
    pub(crate) fn forward_dispatched(&mut self,ticket:u64,outcome:OperationOutcome) {
        if let Some(forward)=self.forwards.get_mut(&ticket) && let Phase::Sending {lease}=&forward.stage {
            forward.queued=Some(Work::ForwardFinished {lease:lease.clone(),outcome});forward.stage=Phase::Finish;(self.wake)();
        }
    }
}

impl crate::app::App {
    pub(crate) fn execute_journal_forwards(&mut self) {
        for (ticket,draft,payload) in self.journal.take_ready_forwards() {
            let current=self.remote_target_is_current(&draft.target)
                && self.remote.sessions.iter().find(|session|session.state.local_workspace==draft.target.workspace).is_some_and(|session|session.state.remote_to_local.get(&draft.remote_anchor)==Some(&draft.local_anchor));
            let outcome=if !current {OperationOutcome::Cancelled {reason:"remote connection or original mapping retired before enqueue".into()}}
                else if payload!=draft.payload {OperationOutcome::Failed {reason:"committed remote instruction differs from its exact connection lease".into()}}
                else {
                    match draft.target.sender.send(tasty_remote::client_session::OutFrame {tag:tasty_ipc::stream::StreamTag::Control,payload}) {
                        Ok(())=> {
                            if let Some(session)=self.remote.sessions.iter_mut().find(|session|session.state.local_workspace==draft.target.workspace && session.transport.frame_tx.epoch().same(&draft.target.sender.epoch())) {
                                if let Some(focus)=draft.focus {session.state.pending_op_focus.insert(draft.op_id,focus);}
                                session.state.agent_requests.note_structural(draft.silent_failure,draft.op_id);
                            }
                            OperationOutcome::Succeeded
                        },
                        Err(_)=>OperationOutcome::Failed {reason:"remote connection rejected the structural enqueue".into()},
                    }
                };
            self.journal.forward_dispatched(ticket,outcome);
        }
    }
}
