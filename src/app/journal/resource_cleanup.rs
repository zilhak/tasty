//! Application scheduling of committed close obligations. The resources stay on EngineSession.
use super::*;
use tasty_domain::{OperationId,OperationOutcome};
#[derive(Clone,Copy)]
enum Phase {Claim,Running,Finish {uncertain:bool}}
pub(super) struct Cleanup {
    pub(super) engine:EngineId,
    operation:OperationId,
    phase:Phase,
    queued:Option<Work>,
}
impl JournalApplication {
    pub(crate) fn has_resource_cleanup(&self,engine:EngineId)->bool {self.resource_cleanups.values().any(|cleanup|cleanup.engine==engine)}

    pub(super) fn queue_resource_retirement(&mut self,engine:EngineId,stream:String,operation:OperationId)->Result<(),String> {
        if self.resource_cleanups.values().any(|entry|entry.engine==engine && entry.operation==operation) {return Err("duplicate retirement publication".into());}
        let ticket=self.next_ticket;
        self.next_ticket=ticket.checked_add(1).ok_or("journal ticket range exhausted")?;
        self.resource_cleanups.insert(ticket,Cleanup {engine,operation:operation.clone(),phase:Phase::Claim,queued:Some(Work::ClaimRetirement {stream,operation})});
        (self.wake)();Ok(())
    }
    pub(super) fn poll_resource_cleanup(&mut self,sessions:&mut [&mut EngineSession])->Result<(),String> {
        for (ticket,entry) in &mut self.resource_cleanups {
            if matches!(entry.phase,Phase::Running) {
                let session=sessions.iter_mut().find(|session|session.id==entry.engine).ok_or("retirement session disappeared")?;
                let Some(mut owner)=session.pending_resource_retirements.remove(&entry.operation) else {return Err("retirement owner disappeared before completion".into());};
                let outcome=owner.outcome().map(|outcome| {
                    if matches!(outcome,OperationOutcome::Succeeded) {
                        if let Err(reason)=owner.finish_metadata(&mut session.borrow_mut()) {return OperationOutcome::Uncertain {reason};}
                    }
                    outcome
                });
                let lease=owner.lease().cloned();
                session.pending_resource_retirements.insert(entry.operation.clone(),owner);
                if let Some(outcome)=outcome {
                    let lease=lease.ok_or("retirement has no Running lease")?;
                    entry.phase=Phase::Finish {uncertain:matches!(outcome,OperationOutcome::Uncertain {..})};
                    entry.queued=Some(Work::RetirementFinished {lease,outcome});
                }
            }
            let Some(work)=entry.queued.take() else {continue;};
            match self.worker.submit_owned(Request {ticket:*ticket,work}) {
                Ok(())=>{},
                Err((crate::runtime::journal_product::SubmitError::Busy,request))=>entry.queued=Some(request.work),
                Err((error,_))=>return Err(format!("committed cleanup submission failed: {error:?}")),
            }
        }
        Ok(())
    }
    pub(super) fn answer_resource_cleanup(&mut self,ticket:u64,result:&Result<ResultValue,String>,sessions:&mut [&mut EngineSession],plugins:Option<&mut crate::plugin::PluginManager>)->Result<bool,String> {
        let Some(entry)=self.resource_cleanups.get_mut(&ticket) else {return Ok(false);};
        let session=sessions.iter_mut().find(|session|session.id==entry.engine).ok_or("retirement session disappeared")?;
        match entry.phase {
            Phase::Claim=>{
                let ResultValue::RetirementClaimed(claim)=result.as_ref().map_err(Clone::clone)? else {return Err("cleanup claim returned another result".into());};
                if claim.lease.operation!=entry.operation {return Err("cleanup completion belongs to another operation".into());}
                let mut owner=session.pending_resource_retirements.remove(&entry.operation).ok_or("retirement resources missing")?;
                let start=owner.start(claim.clone(),&mut session.borrow_mut(),plugins);
                let has_lease=owner.lease().is_some();
                session.pending_resource_retirements.insert(entry.operation.clone(),owner);
                if let Err(error)=start {
                    if !has_lease {return Err(error);}
                    // Partial host publication cannot be reported as a known cancelled close.
                    entry.phase=Phase::Finish {uncertain:true};
                    entry.queued=Some(Work::RetirementFinished {lease:claim.lease.clone(),outcome:OperationOutcome::Uncertain {reason:error}});
                } else {entry.phase=Phase::Running;}
                (self.wake)();
            },
            Phase::Finish {uncertain}=>{
                match result.as_ref().map_err(Clone::clone)? {ResultValue::Executed(_)|ResultValue::Stored(_)=>{},_=>return Err("cleanup result commit returned another value".into())}
                if uncertain {return Err("committed resource retirement requires reconciliation".into());}
                if let Some(owner)=session.pending_resource_retirements.remove(&entry.operation) {
                    owner.notify_completed(&mut session.borrow_mut());
                }
                self.resource_cleanups.remove(&ticket);
                self.refresh_in_progress_commands();
            },
            Phase::Running=>return Err("unexpected completion while awaiting cleanup receipts".into()),
        }
        Ok(true)
    }
}
