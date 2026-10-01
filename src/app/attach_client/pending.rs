//! Application values identify the sole target; Remote retains pending connection resources.
use super::*;
use tasty_remote::pending_connection::ConnectionTicket;
#[derive(Clone)]
pub(crate) struct PendingMirrorInstall {
    pub engine:EngineId,
    pub window:Option<winit::window::WindowId>,
    pub view:Option<std::sync::Weak<()>>,
    pub selection:Option<std::sync::Weak<()>>,
    pub activate:bool,
    pub anchor:Option<u32>,
    pub mapping:Option<crate::model::WorkspaceAttachMapping>,
    pub mapping_token:Option<std::sync::Weak<()>>,
    pub reconnect:Option<(u32,tasty_remote::connection::ConnectionEpoch)>,
}
impl App {
    pub(crate) fn mirror_install_target(&self,explicit:Option<EngineId>,anchor:Option<u32>,reconnect:Option<usize>,activate:bool)->anyhow::Result<PendingMirrorInstall> {
        let old=reconnect.and_then(|index|self.remote.sessions.get(index)).map(|session|(session.state.local_workspace,session.transport.frame_tx.epoch()));
        let engine=explicit.or_else(||old.as_ref().and_then(|(workspace,_)|self.engines.all_sessions().find(|engine|engine.core_state.has_workspace(*workspace)).map(|engine|engine.id)))
            .or_else(||anchor.and_then(|workspace|self.engines.all_sessions().find(|engine|engine.core_state.has_workspace(workspace)).map(|engine|engine.id)))
            .or_else(||self.focused_window().and_then(|main|self.engines.of_window(main.base.winit.id())))
            .ok_or_else(||anyhow::anyhow!("no engine is available to receive a mirror"))?;
        let window=self.engines.window_of(engine);
        let main=window.and_then(|window|self.view.views.get(&window)).and_then(|view|view.as_main());
        if old.is_none() && main.is_none() {anyhow::bail!("mirror creation has no originating View");}
        let owner=self.engines.get(engine).ok_or_else(||anyhow::anyhow!("mirror target engine missing"))?;
        let mapping=anchor.and_then(|id|owner.find_workspace_index_for_id(id).and_then(|index|owner.workspace_at(index)).and_then(|workspace|workspace.attach_mapping.clone()));
        let mapping_token=anchor.and_then(|id|owner.remote.attach_mapping_tokens.get(&id)).map(Arc::downgrade);
        Ok(PendingMirrorInstall {mapping,mapping_token,engine,window,view:main.map(|main|main.base.state.identity()),selection:main.map(|main|main.state.navigation.generation()),activate,anchor,reconnect:old})
    }
    pub(crate) fn mirror_install_target_is_current(&self,target:&PendingMirrorInstall)->bool {
        let Some(engine)=self.engines.get(target.engine) else {return false;};
        if let Some(anchor)=target.anchor {
            let current=engine.find_workspace_index_for_id(anchor).and_then(|index|engine.workspace_at(index));
            if current.is_none_or(|workspace|workspace.attach_mapping!=target.mapping)
                || target.mapping_token.as_ref().is_none_or(|token|engine.remote.attach_mapping_tokens.get(&anchor).is_none_or(|current|!token.ptr_eq(&Arc::downgrade(current)))) {return false;}
        }
        if let Some((workspace,epoch))=&target.reconnect {
            return engine.has_workspace(*workspace) && self.remote.sessions.iter().any(|session|session.state.local_workspace==*workspace && session.transport.frame_tx.epoch().same(epoch));
        }
        target.window.is_some_and(|window|self.engines.of_window(window)==Some(target.engine)
            && self.view.views.get(&window).and_then(|view|view.as_main()).is_some_and(|main|target.view.as_ref().is_some_and(|identity|main.base.state.matches_identity(identity))))
    }
    pub(crate) fn queue_mirror_connection(&mut self,target:PendingMirrorInstall,port:u16,workspace:u32,tunnel:Option<tasty_ssh::SshTunnel>)->anyhow::Result<()> {
        if !self.mirror_install_target_is_current(&target) {anyhow::bail!("mirror origin was retired before connection");}
        let stale:Vec<_>=self.state.pending_mirror_installs.iter().filter_map(|(ticket,pending)|
            ((target.anchor.is_some() || target.reconnect.is_some()) && pending.engine==target.engine && pending.reconnect.as_ref().map(|(id,_)|*id)==target.reconnect.as_ref().map(|(id,_)|*id) && pending.anchor==target.anchor).then_some(*ticket)).collect();
        for ticket in stale {self.state.pending_mirror_installs.remove(&ticket);self.remote.cancel_connection(ticket);}
        let mapping=target.mapping.clone();
        let ticket=self.remote.queue_connection(port,workspace,tunnel,target.anchor,mapping,attach_wake(&self.view.proxy),mirror_event_from_control).map_err(anyhow::Error::msg)?;
        self.state.pending_mirror_installs.insert(ticket,target);
        Ok(())
    }
    pub(crate) fn poll_pending_mirror_installs(&mut self) {
        self.remote.collect_connections();
        let pending:Vec<_>=self.state.pending_mirror_installs.iter().map(|(ticket,target)|(*ticket,target.clone())).collect();
        for (ticket,target) in pending {
            if !self.mirror_install_target_is_current(&target) {
                self.state.pending_mirror_installs.remove(&ticket);self.remote.cancel_connection(ticket);continue;
            }
            if let Some(error)=self.remote.connection_error(ticket) {self.fail_pending_mirror(ticket,&target,error);continue;}
            if target.reconnect.is_some() && self.engines.window_of(target.engine).is_none() {continue;}
            let Some(prepared)=self.remote.prepared_connection(ticket) else {continue;};
            let Some(engine)=self.engines.get(target.engine) else {continue;};
            let admission=mirror_id_needs(&prepared.tree,prepared.surfaces.len(),target.reconnect.is_none()).and_then(|needed|engine.runtime.ids.ensure(&needed).map_err(anyhow::Error::new));
            if let Err(error)=admission {
                if matches!(error.downcast_ref::<crate::runtime::id_reservations::ReservationError>(),Some(crate::runtime::id_reservations::ReservationError::Pending)) {
                    (engine.runtime.waker)();continue;
                }
                self.fail_pending_mirror(ticket,&target,error.to_string());continue;
            }
            let Some(prepared)=self.remote.take_connection(ticket) else {continue;};
            self.state.pending_mirror_installs.remove(&ticket);
            let installed=if let Some((workspace,epoch))=&target.reconnect {
                let index=self.remote.sessions.iter().position(|session|session.state.local_workspace==*workspace && session.transport.frame_tx.epoch().same(epoch));
                index.ok_or_else(||anyhow::anyhow!("reconnect target retired")).and_then(|index|self.install_reconnected_mirror(index,&target,prepared)).map(|_|*workspace)
            } else {self.install_new_mirror(&target,prepared)};
            match installed {
                Ok(workspace)=>{
                    if target.activate
                        && let Some(window)=target.window
                        && let Some((main,engine))=self.engines().window_pair(window)
                        && target.view.as_ref().is_some_and(|view|main.base.state.matches_identity(view))
                        && target.selection.as_ref().is_some_and(|selection|main.state.navigation.matches_generation(selection))
                        && engine.has_workspace(workspace) {self.focus_mirror_workspace(workspace);}
                    if let Some(anchor)=target.anchor {self.remote.reconnect.remove(&anchor);}
                },
                Err(error)=>self.fail_pending_mirror(ticket,&target,error.to_string()),
            }
        }
    }
    fn fail_pending_mirror(&mut self,ticket:ConnectionTicket,target:&PendingMirrorInstall,error:String) {
        self.state.pending_mirror_installs.remove(&ticket);self.remote.cancel_connection(ticket);
        tracing::warn!(engine=?target.engine,"mirror connection could not be installed: {error}");
        if let Some(anchor)=target.anchor {
            self.remote.active.remove(&anchor);
            if target.reconnect.is_some() {self.on_reconnect_attempt_failed(anchor,&anyhow::anyhow!(error));}
        }
    }
}
