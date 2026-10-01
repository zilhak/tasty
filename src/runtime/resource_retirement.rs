//! Sole ownership of resources detached by a committed tombstone, before cleanup is claimed.
use crate::runtime::engine_session::EngineSession;
use crate::runtime::journal_product::{ClaimedRetirement,EffectLease};
use tasty_domain::{Operation,RetirementPlan};
use tasty_terminal::{Terminal,Pty,PtyRetirement,PtyPhase};
struct RemovedOwner {
    surface:Box<dyn crate::model::Surface>,
    pair:Option<(Terminal,Option<Pty>)>,
    scrollback:Option<String>,
}
pub(crate) struct ResourceRetirement {
    plan:RetirementPlan,
    engine_incarnation:u64,
    runtime_epoch:u64,
    owners:Vec<RemovedOwner>,
    receipts:Vec<PtyRetirement>,
    started:Option<std::time::Instant>,
    lease:Option<EffectLease>,
    start_complete:bool,
    failure:Option<String>,
    metadata_complete:bool,
}
impl ResourceRetirement {
    /// Called only while publishing an already committed close, before the batch ACK.
    pub(crate) fn capture(session:&mut EngineSession,operation:&Operation)->Result<Self,String> {
        let plan=operation.retirement.clone().ok_or("retirement plan missing")?;
        let binding=session.journal_binding.as_ref().ok_or("retirement engine is unbound")?;
        if binding.incarnation!=operation.engine_incarnation {return Err("retirement belongs to another engine incarnation".into());}
        let runtime_epoch=binding.runtime_epoch;
        for target in &plan.surfaces {
            let descriptor=session.core_state.find_surface_by_id(target.id).ok_or("retirement descriptor missing")?;
            if descriptor.kind!=target.kind || descriptor.activation_generation!=target.activation_generation {
                return Err("retirement descriptor differs from the committed old owner".into());
            }
            if !session.runtime.surfaces.contains_key(&target.id) {return Err("retirement runtime owner missing".into());}
        }
        let owners=plan.surfaces.iter().map(|target| {
            let scrollback=session.runtime.terminals.scrollback_persist_id(target.id).map(str::to_owned);
            RemovedOwner {surface:session.runtime.surfaces.remove(&target.id).expect("preflight retained the owner"),pair:session.runtime.terminals.remove(target.id),scrollback}
        }).collect();
        Ok(Self {plan,engine_incarnation:operation.engine_incarnation,runtime_epoch,owners,receipts:Vec::new(),started:None,lease:None,start_complete:false,failure:None,metadata_complete:false})
    }
    pub(crate) fn start(&mut self,claim:ClaimedRetirement,engine:&mut crate::runtime::engine_access::EngineMut<'_>,mut plugins:Option<&mut crate::plugin::PluginManager>)->Result<(),String> {
        if self.started.is_some() || claim.plan!=self.plan || claim.engine_incarnation!=self.engine_incarnation || claim.lease.runtime_epoch!=self.runtime_epoch {
            return Err("cleanup claim does not own the detached resources".into());
        }
        self.lease=Some(claim.lease);
        self.started=Some(std::time::Instant::now());
        let result=self.start_owned(engine,plugins);
        self.start_complete=result.is_ok();
        self.failure=result.as_ref().err().cloned();
        result
    }
    fn start_owned(&mut self,engine:&mut crate::runtime::engine_access::EngineMut<'_>,mut plugins:Option<&mut crate::plugin::PluginManager>)->Result<(),String> {
        if self.owners.iter().any(|owner|owner.surface.as_any().is::<crate::plugin_bridge::remote_surface::RemoteSurface>()) && plugins.is_none() {
            return Err("plugin host is absent for committed retirement".into());
        }
        // The original boxes and Pty handles are private to this operation. No ID lookup can
        // retarget destruction to a replacement installed later under the same logical identity.
        for owner in &self.owners {
            if let Some(remote)=owner.surface.as_any().downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>() {
                plugins.as_deref_mut().ok_or("plugin host disappeared")?.enqueue_bound_remote_retirement(remote.id,remote.handles().binding())?;
            }
        }
        for mut owner in self.owners.drain(..) {
            let id=owner.surface.surface_id().ok_or("retiring kind has no identity")?;
            if let Some((terminal,pty))=owner.pair.take() {drop(terminal);if let Some(pty)=pty {self.receipts.push(pty.retire());}}
            if let Some(reference)=owner.scrollback {crate::scrollback_store::delete(&reference);}
            engine.cleanup_surface_observations(id);
            drop(owner.surface);
        }
        Ok(())
    }
    pub(crate) fn outcome(&self)->Option<tasty_domain::OperationOutcome> {
        use tasty_domain::OperationOutcome;
        let started=self.started?;
        if let Some(reason)=&self.failure {return Some(OperationOutcome::Uncertain {reason:reason.clone()});}
        if !self.start_complete {return None;}
        if self.receipts.iter().any(|receipt|receipt.observation().phase==PtyPhase::WaitFailed) {
            return Some(OperationOutcome::Uncertain {reason:"closed PTY owner has no confirmed reap".into()});
        }
        if self.receipts.iter().all(|receipt|receipt.observation().phase==PtyPhase::Reaped) {return Some(OperationOutcome::Succeeded);}
        if started.elapsed()>=std::time::Duration::from_secs(5) {return Some(OperationOutcome::Uncertain {reason:"closed PTY owner reap deadline elapsed".into()});}
        None
    }
    pub(crate) fn finish_metadata(&mut self,engine:&mut crate::runtime::engine_access::EngineMut<'_>)->Result<(),String> {
        if self.metadata_complete {return Ok(());}
        if !matches!(self.outcome(),Some(tasty_domain::OperationOutcome::Succeeded)) {return Err("cleanup receipt is not complete".into());}
        for target in &self.plan.removed {
            let scope=match target.kind {
                tasty_domain::IdKind::Surface=> {
                    if engine.core.find_surface_by_id(target.id).is_some() {return Err("retired surface identity is live again; refusing metadata cleanup".into());}
                    tasty_memory::Scope::Surface(target.id)
                },
                tasty_domain::IdKind::Workspace=> {
                    if engine.core.has_workspace(target.id) {return Err("retired workspace identity is live again; refusing metadata cleanup".into());}
                    tasty_memory::Scope::Workspace(target.id)
                },
                _=>continue,
            };
            engine.purge_closed_memory_scope(&scope)?;
        }
        self.metadata_complete=true;Ok(())
    }
    /// Notification delivery follows durable completion. It performs no destruction and is not
    /// replayed from historical operations during recovery.
    pub(crate) fn notify_completed(&self,engine:&mut crate::runtime::engine_access::EngineMut<'_>) {
        use crate::core::host_event::{PendingHostEvent,PendingSurfaceClosed};
        for surface in &self.plan.surfaces {
            engine.runtime.pending_lifecycle_events.push(PendingSurfaceClosed {
                surface_id:surface.id,kind:Some(surface.kind.clone()),is_user_close:self.plan.is_user_close,
            });
        }
        for (tab_id,pane_id) in &self.plan.tab_parents {
            engine.enqueue_host_event(PendingHostEvent::TabClosed {tab_id:*tab_id,pane_id:*pane_id});
        }
        for entity in &self.plan.removed {
            match entity.kind {
                tasty_domain::IdKind::Pane=>engine.enqueue_host_event(PendingHostEvent::PaneClosed {pane_id:entity.id}),
                tasty_domain::IdKind::Workspace=>engine.enqueue_host_event(PendingHostEvent::WorkspaceClosed {workspace_id:entity.id}),
                _=>{},
            }
        }
    }
    pub(crate) fn lease(&self)->Option<&EffectLease> {self.lease.as_ref()}
}
