//! Application side of the data-home worker. Engine bindings live on their EngineSession.
use crate::runtime::engine_session::{EngineId, EngineSession};
use crate::runtime::journal_product::{
    Completion, EngineSelection, JournalWorker, Request, ResultValue, Work,
};
use crate::runtime::live_projection;
pub(crate) mod commands;
mod creation;
mod resource_cleanup;
mod capture;
mod id_reservations;
#[cfg(feature = "gui")]
mod retirement;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

struct Opening {
    selection: EngineSelection,
    ticket: Option<u64>,
    projected: bool,
    surface_floor: u32,
    select_available_slot: bool,
}

pub(crate) struct JournalApplication {
    worker: JournalWorker,
    commands: commands::Commands,
    captures:std::collections::BTreeMap<u64,capture::PendingCapture>,
    capture_requests:HashMap<EngineId,bool>,
    execution_id_requests:HashMap<u64,EngineId>,
    changed_engines: std::collections::HashSet<EngineId>,
    completion_views:HashMap<EngineId,crate::runtime::journal_product::CompletionView>,
    wake: Arc<dyn Fn() + Send + Sync>,
    opening: HashMap<EngineId, Opening>,
    creations: HashMap<(EngineId,u64), creation::Creation>,
    resource_cleanups:std::collections::BTreeMap<u64,resource_cleanup::Cleanup>,
    restoration_reads: HashMap<u64, (EngineId, crate::runtime::surface_restorer::RestoreInput)>,
    restoration_queue: VecDeque<(EngineId, crate::runtime::surface_restorer::RestoreInput)>,
    restoration_ready: HashMap<EngineId, Vec<crate::runtime::surface_restorer::RestoreInput>>,
    restoration_boot_done: std::collections::HashSet<EngineId>,
    restored_views: HashMap<EngineId, crate::model::RestoredPresentation>,
    #[cfg(feature = "gui")]
    queued_view_writes: HashMap<String, crate::runtime::journal_product::view_record::StoredView>,
    #[cfg(feature = "gui")]
    view_writes: HashMap<u64, crate::runtime::journal_product::view_record::StoredView>,
    #[cfg(feature = "gui")]
    failed_view_writes: HashMap<
        String,
        (
            crate::runtime::journal_product::view_record::StoredView,
            String,
        ),
    >,
    #[cfg(feature = "gui")]
    latest_view_sequence: u64,
    #[cfg(feature = "gui")]
    retirements: retirement::Retirements,
    known_slots: std::collections::BTreeMap<u32, bool>,
    started: bool,
    next_ticket: u64,
    halted: Option<String>,
}

impl JournalApplication {
    pub(crate) fn update_completion_view(&mut self,id:EngineId,core:&crate::core::CoreState,presentation:&dyn crate::model::StructurePresentation) {
        let mut focused_panes=std::collections::BTreeMap::new();
        let mut selected_tabs=std::collections::BTreeMap::new();
        let mut selected_surfaces=std::collections::BTreeMap::new();
        for workspace in &core.workspaces() {
            if let Some(pane)=presentation.pane_id(workspace) {focused_panes.insert(workspace.id,pane);}
            for pane in workspace.pane_layout().all_pane_ids() {
                if let Some(pane)=workspace.pane_layout().find_pane(pane) {
                    if let Some(tab)=pane.tabs.get(presentation.tab_index(pane)) {selected_tabs.insert(pane.id,tab.id);}
                    for tab in &pane.tabs {if let Some(surface)=presentation.surface_id(tab) {selected_surfaces.insert(tab.id,surface);}}
                }
            }
        }
        self.completion_views.insert(id,crate::runtime::journal_product::CompletionView {focused_panes,mirror_count:core.mirror_workspaces.len(),selected_tabs,selected_surfaces});
    }

    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<Self> {
        let home = tasty_utils::path::tasty_home()
            .ok_or_else(|| anyhow::anyhow!("data home unavailable for structure journal"))?;
        let worker = JournalWorker::spawn(home, wake.clone()).map_err(anyhow::Error::msg)?;
        Ok(Self {
            worker,
            commands: Default::default(),
            captures:Default::default(),
            capture_requests:Default::default(),
            execution_id_requests:Default::default(),
            changed_engines: Default::default(),
            completion_views:Default::default(),
            wake,
            opening: HashMap::new(),
            creations: HashMap::new(),
            resource_cleanups:Default::default(),
            restoration_reads: HashMap::new(),
            restoration_queue: VecDeque::new(),
            restoration_ready: HashMap::new(),
            restoration_boot_done: Default::default(),
            restored_views: Default::default(),
            #[cfg(feature = "gui")]
            queued_view_writes: Default::default(),
            #[cfg(feature = "gui")]
            view_writes: Default::default(),
            #[cfg(feature = "gui")]
            failed_view_writes: Default::default(),
            #[cfg(feature = "gui")]
            latest_view_sequence: 0,
            #[cfg(feature = "gui")]
            retirements: Default::default(),
            known_slots: Default::default(),
            started: false,
            next_ticket: 1,
            halted: None,
        })
    }

    pub(crate) fn begin_engine(
        &mut self,
        session: &EngineSession,
        selection: EngineSelection,
    ) -> Result<(), String> {
        if self.opening.contains_key(&session.id) {
            return Ok(());
        }
        let scopes = session
            .runtime.memory
            .lock()
            .map_err(|error| error.to_string())?
            .scopes()
            .map_err(|error| format!("cannot establish existing surface ID floor: {error}"))?;
        let surface_floor = scopes
            .iter()
            .filter_map(|scope| match tasty_memory::Scope::parse(scope) {
                Ok(tasty_memory::Scope::Surface(id))
                    if crate::runtime::terminal_store::is_surface_id_space(id) =>
                {
                    Some(id)
                }
                _ => None,
            })
            .max()
            .unwrap_or(0);
        self.opening.insert(
            session.id,
            Opening {
                selection,
                ticket: None,
                projected: false,
                surface_floor,
                select_available_slot: false,
            },
        );
        Ok(())
    }

    /// The caller holds newly opening engines behind its startup read/render barrier.
    pub(crate) fn poll_bootstrap(
        &mut self,
        sessions: &mut [&mut EngineSession],
        plugins:Option<&mut crate::plugin::PluginManager>,
    ) -> Result<(), String> {
        if let Some(reason) = &self.halted {
            return Err(reason.clone());
        }
        let result = self.poll_initial(sessions,plugins);
        #[cfg(feature = "gui")]
        for session in sessions {
            if session
                .journal_binding
                .as_ref()
                .is_some_and(|binding| self.failed_view_writes.contains_key(&binding.stream))
            {
                session.core_state.layout_dirty.mark_dirty();
            }
        }
        if let Err(reason) = &result {
            self.halted = Some(reason.clone());
            self.fail_pending_commands(reason);
        }
        result
    }

    fn poll_initial(&mut self,sessions:&mut [&mut EngineSession],mut plugins:Option<&mut crate::plugin::PluginManager>)->Result<(),String> {
        const MAX_COMPLETIONS: usize = 16;
        for _ in 0..MAX_COMPLETIONS {
            if self.started {
                self.submit_openings()?;
                self.submit_commands()?;
                self.poll_resource_cleanup(sessions)?;
                self.submit_captures(sessions)?;
                self.refill_execution_ids(sessions)?;
                self.submit_restore_reads()?;
                #[cfg(feature = "gui")]
                self.submit_retirements()?;
                #[cfg(feature = "gui")]
                self.submit_view_writes()?;
                for ((engine,_), creation) in &mut self.creations {
                    let mirror_count = sessions
                        .iter()
                        .find(|session| session.id == *engine)
                        .ok_or("materializing engine disappeared")?
                        .core_state
                        .mirror_workspaces
                        .len();
                    let mut view=self.completion_views.get(engine).cloned().unwrap_or_default();
                    view.mirror_count=mirror_count;
                    creation.poll_cleanup(&self.worker,view)?;
                }
            }
            let completion = match self.worker.try_recv() {
                Ok(value) => value,
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("structure journal worker stopped during bootstrap".into());
                }
            };
            match completion {
                Completion::Ready {
                    cut, mut bootstrap, ..
                } => {
                    self.known_slots.extend(bootstrap.streams.iter().filter_map(
                        |(stream, model)| {
                            stream
                                .strip_prefix("structure:slot-")
                                .and_then(|slot| slot.parse::<u32>().ok())
                                .map(|slot| (slot, model.engine_retired))
                        },
                    ));
                    let occupied: Vec<_> = sessions
                        .iter()
                        .map(|session| (session.id, session.core_state.layout_slot))
                        .collect();
                    for session in sessions.iter_mut() {
                        let Some(opening) = self.opening.get_mut(&session.id) else {
                            continue;
                        };
                        if opening.select_available_slot
                            && let EngineSelection::Slot { slot, .. } = &mut opening.selection
                        {
                            if let Some(available) = self
                                .known_slots
                                .iter()
                                .filter(|(_, retired)| !**retired)
                                .map(|(slot, _)| slot)
                                .find(|candidate| {
                                    !occupied.iter().any(|(id, used)| {
                                        *id != session.id && *used == Some(**candidate)
                                    })
                                })
                            {
                                *slot = *available;
                                session.core_state.layout_slot = Some(*available);
                            }
                        }
                        let model = match opening.selection {
                            EngineSelection::Slot { slot, resume: true } => bootstrap
                                .streams
                                .remove(&format!("structure:slot-{slot}"))
                                .unwrap_or_default(),
                            _ => tasty_domain::JournalModel::default(),
                        };
                        live_projection::bootstrap::initialize(&mut session.core_state, &model)?;
                        crate::runtime::surface_restorer::initialize_instances(session,&model);
                        opening.projected = matches!(
                            opening.selection,
                            EngineSelection::Slot { resume: true, .. }
                        );
                    }
                    drop(bootstrap);
                    self.worker
                        .acknowledge(cut.unwrap_or(0), Ok(()))
                        .map_err(|error| format!("bootstrap ACK: {error:?}"))?;
                    self.started = true;
                }
                Completion::StartupFailed(error) | Completion::Halted(error) => return Err(error),
                Completion::Publish {
                    batch,
                    before,
                    engine_binding,
                } => {
                    for session in sessions.iter_mut() {
                        let opening = self.opening.get_mut(&session.id);
                        let stream = match opening.as_ref().map(|opening| &opening.selection) {
                            Some(EngineSelection::Slot { slot, .. }) => {
                                Some(format!("structure:slot-{slot}"))
                            }
                            Some(EngineSelection::FreshHeadless) => engine_binding
                                .as_ref()
                                .map(|binding| binding.stream.clone()),
                            None => session
                                .journal_binding
                                .as_ref()
                                .map(|binding| binding.stream.clone()),
                        };
                        let Some(stream) = stream else {
                            continue;
                        };
                        if let Some(slot) = stream
                            .strip_prefix("structure:slot-")
                            .and_then(|slot| slot.parse::<u32>().ok())
                        {
                            self.known_slots.entry(slot).or_insert(false);
                        }
                        let Some(events) = batch.streams.get(&stream) else {
                            continue;
                        };
                        let predecessor =
                            before.get(&stream).ok_or("bootstrap predecessor missing")?;
                        let domain = tasty_domain::DomainBatch {
                            batch_id: batch.batch_id,
                            events: events.clone(),
                        };
                        if let Some(opening) = opening
                            && (!opening.projected
                                || (session.core_state.local_workspaces.is_empty()
                                    && predecessor.workspaces.is_empty()))
                        {
                            let mut after = predecessor.clone();
                            tasty_domain::evolve(&mut after, &domain)
                                .map_err(|error| error.to_string())?;
                            live_projection::bootstrap::initialize(
                                &mut session.core_state,
                                &after,
                            )?;
                            crate::runtime::surface_restorer::initialize_instances(session,&after);
                            opening.projected = true;
                        } else {
                            let mut prepared = Vec::new();
                            let mut installations = Vec::new();
                            for (key,creation) in self.creations.iter_mut().filter(|((engine,_),_)|*engine==session.id) {
                                if let Some(installation)=creation.authorize_installation(session,events)? {installations.push((*key,installation));}
                                if let Some(leaf)=creation.leaf_for_publication(session,events)? {prepared.push(leaf);}
                            }
                            for event in &domain.events {
                                if let tasty_domain::DomainEvent::OperationPrepared {operation}=&event.event
                                    && matches!(operation.creation.as_ref().map(|plan|&plan.destination),Some(tasty_domain::CreationDestination::Assembly {..})) {
                                    let ticket=self.next_ticket;self.next_ticket=self.next_ticket.checked_add(1).ok_or("journal ticket exhausted")?;
                                    let binding=session.journal_binding.clone().ok_or("assembly engine binding missing")?;
                                    let creation=creation::Creation::committed(ticket,binding,&self.worker,operation.id.clone())?;
                                    self.creations.insert((session.id,ticket),creation);
                                }
                                if let tasty_domain::DomainEvent::OperationPrepared {operation}=&event.event
                                    && operation.retirement.is_some() {
                                    let retirement=crate::runtime::resource_retirement::ResourceRetirement::capture(session,operation)?;
                                    session.pending_resource_retirements.insert(operation.id.clone(),retirement);
                                    self.queue_resource_retirement(session.id,stream.clone(),operation.id.clone())?;
                                }
                            }
                            live_projection::apply(
                                &mut session.core_state,
                                predecessor,
                                &domain,
                                &mut Vec::new(),
                            )?;
                            for leaf in prepared {
                                let id=leaf.surface.surface_id().ok_or("materialized kind has no ID")?;
                                let descriptor=session.core_state.find_surface_by_id(id).ok_or("materialized leaf has no committed descriptor")?;
                                if descriptor.kind!=leaf.logical_kind {return Err("materialized kind differs from committed descriptor".into());}
                                let deferred=leaf.surface.as_any().is::<crate::runtime::surface_restorer::JournalPlaceholder>();
                                drop(session.runtime.surfaces.insert(id,leaf.surface));
                                if deferred {
                                    let mut after=predecessor.clone();
                                    tasty_domain::evolve(&mut after,&domain).map_err(|error|error.to_string())?;
                                    if let Some(placeholder)=session.runtime.surfaces.get_mut(&id).and_then(|surface|surface.as_any_mut().downcast_mut::<crate::runtime::surface_restorer::JournalPlaceholder>()) {
                                        let value=after.surfaces.get(&id).ok_or("deferred assembly leaf missing")?;
                                        placeholder.data=value.data;placeholder.creation_seed=value.creation_seed;placeholder.activation=value.activation;
                                    }
                                    if let Some(request)=crate::runtime::surface_restorer::describe(&session.as_ref()).into_iter().find(|request|request.surface_id==id) {
                                        if request.reference.is_some() {self.restoration_queue.push_back((session.id,request));} else {self.restoration_ready.entry(session.id).or_default().push(request);}
                                    }
                                }
                            }
                            for (key,installation) in installations {
                                let retiring = session
                                    .as_ref()
                                    .find_surface_by_id(installation.surface_id())
                                    .map(crate::runtime::effect_runner::RetiringKind::capture);
                                let installed = installation
                                    .install(&mut session.borrow_mut(), plugins.as_deref_mut(), retiring)
                                    .map_err(|error| error.to_string())?;
                                self.creations
                                    .get_mut(&key)
                                    .expect("materialization request")
                                    .installed(installed);
                            }
                        }
                        for recorded in events {
                            match &recorded.event {
                                tasty_domain::DomainEvent::WorkspaceAttachMappingSet {id,..}=>{session.remote.attach_mapping_tokens.insert(*id,Arc::new(()));},
                                tasty_domain::DomainEvent::WorkspaceClosed {id}=>{session.remote.attach_mapping_tokens.remove(id);},
                                _=>{},
                            }
                        }
                        if let Some(slot) = stream
                            .strip_prefix("structure:slot-")
                            .and_then(|slot| slot.parse::<u32>().ok())
                        {
                            let mut retired = predecessor.engine_retired;
                            for event in events {
                                match event.event {
                                    tasty_domain::DomainEvent::EngineRetired { .. } => {
                                        retired = true
                                    }
                                    tasty_domain::DomainEvent::EngineIncarnationStarted {
                                        ..
                                    } => retired = false,
                                    _ => {}
                                }
                            }
                            self.known_slots.insert(slot, retired);
                        }
                        if events.iter().any(|recorded|!matches!(recorded.event,tasty_domain::DomainEvent::SurfaceDataRecorded {..})) {
                            session.core_state.mark_layout_dirty();
                        }
                        self.changed_engines.insert(session.id);
                        if let Some(binding) = session.journal_binding.as_mut() {
                            binding.published_cut = Some(batch.batch_id);
                            if let Some(last) = events.last() {
                                binding.revision = Some(last.revision);
                            }
                        }
                    }
                    self.creations.retain(|_,creation|!creation.publication_released());
                    self.worker
                        .acknowledge(batch.batch_id, Ok(()))
                        .map_err(|error| format!("bootstrap publication ACK: {error:?}"))?;
                }
                Completion::Finished { ticket, result } => {
                    if self.answer_execution_ids(ticket,&result,sessions)? {continue;}
                    if self.answer_capture(ticket,&result,sessions) {continue;}
                    if self.answer_resource_cleanup(ticket,&result,sessions,plugins.as_deref_mut())? {continue;}
                    if self.answer_command(ticket, &result, sessions)? {
                        continue;
                    }
                    #[cfg(feature = "gui")]
                    if self.finish_retirement(ticket, &result)? {
                        continue;
                    }
                    #[cfg(feature = "gui")]
                    if let Some(view) = self.view_writes.remove(&ticket) {
                        match result {
                            Ok(ResultValue::ViewSaved) => {}
                            Err(error) => {
                                tracing::warn!("View snapshot write failed: {error}");
                                self.failed_view_writes
                                    .insert(view.binding.stream.clone(), (view, error));
                            }
                            _ => return Err("View snapshot returned another completion".into()),
                        }
                        continue;
                    }

                    if let Some((engine, mut restoration)) = self.restoration_reads.remove(&ticket)
                    {
                        #[cfg(feature = "gui")]
                        if self.retirements.pending.contains_key(&engine) {
                            continue;
                        }
                        let ResultValue::Payload { reference, bytes } = result? else {
                            return Err("restore payload request returned another value".into());
                        };
                        let session = sessions
                            .iter()
                            .find(|session| session.id == engine)
                            .ok_or("restoring engine disappeared")?;
                        let current = session
                            .as_ref()
                            .find_surface_by_id(restoration.surface_id)
                            .and_then(|surface| {
                                surface
                                    .as_any()
                                    .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>(
                                    )
                            })
                            .ok_or("restoring surface no longer has its pending capture")?;
                        if current.data.or(current.creation_seed) != restoration.reference {
                            return Err("restoring surface capture changed while reading".into());
                        }
                        crate::runtime::surface_restorer::accept_payload(
                            &mut restoration,
                            reference,
                            &bytes,
                        )?;
                        self.restoration_ready
                            .entry(engine)
                            .or_default()
                            .push(restoration);
                        continue;
                    }
                    if let Some(key) = self
                        .creations
                        .iter()
                        .find_map(|(key, creation)| (creation.ticket == ticket).then_some(*key))
                    {
                        let id=key.0;
                        let session = sessions
                            .iter_mut()
                            .find(|session| session.id == id)
                            .ok_or("creating engine disappeared")?;
                        if self
                            .creations
                            .get_mut(&key)
                            .expect("creation exists")
                            .answered(&self.worker, session, result?)?
                        {
                            self.creations.remove(&key);
                            self.refresh_in_progress_commands();
                        }
                        continue;
                    }
                    let id = self
                        .opening
                        .iter()
                        .find_map(|(id, opening)| (opening.ticket == Some(ticket)).then_some(*id))
                        .ok_or("bootstrap completion belongs to another request")?;
                    let opening = self.opening.remove(&id).expect("matched opening");
                    let session = sessions
                        .iter_mut()
                        .find(|session| session.id == id)
                        .ok_or("opening engine disappeared")?;
                    let ResultValue::Bound(bound) = result? else {
                        return Err("bootstrap did not return an engine binding".into());
                    };
                    if !opening.projected {
                        live_projection::bootstrap::initialize(
                            &mut session.core_state,
                            &bound.model,
                        )?;
                        crate::runtime::surface_restorer::initialize_instances(session,&bound.model);
                    }
                    self.restored_views.insert(
                        session.id,
                        live_projection::bootstrap::presentation(
                            &session.core_state,
                            bound.imported_view,
                        ),
                    );
                    session.core_state.pending_layout_restore = None;
                    for workspace in &session.core_state.local_workspaces {
                        if workspace.attach_mapping.is_some() {session.remote.attach_mapping_tokens.entry(workspace.id).or_insert_with(||Arc::new(()));}
                    }
                    session.journal_binding = Some(bound.binding);
                    if session.core_state.local_workspaces.is_empty() {
                        let ticket = self.next_ticket;
                        self.next_ticket = ticket
                            .checked_add(1)
                            .ok_or("journal ticket range exhausted")?;
                        let creation =
                            creation::Creation::default_workspace(ticket, session, &self.worker)?;
                        self.creations.insert((session.id,creation.ticket), creation);
                    } else {
                        for restoration in
                            crate::runtime::surface_restorer::describe(&session.as_ref())
                        {
                            if restoration.reference.is_some() {
                                self.restoration_queue.push_back((session.id, restoration));
                            } else {
                                self.restoration_ready
                                    .entry(session.id)
                                    .or_default()
                                    .push(restoration);
                            }
                        }
                    }
                }
            }
        }
        // A continuously producing worker must not monopolize the event loop. Request another
        // turn even if its earlier wake was coalesced while this bounded batch was consumed.
        (self.wake)();
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(crate) fn halt_reason(&self) -> Option<&str> {
        self.halted.as_deref()
    }

    #[cfg(feature = "gui")]
    pub(crate) fn abandon_halted_engine(&mut self, id: EngineId) {
        assert!(
            self.is_halted(),
            "only halted bootstrap continuations can be abandoned without a result command"
        );
        self.opening.remove(&id);
        self.creations.retain(|(engine,_),_|*engine!=id);
        self.restored_views.remove(&id);
        self.restoration_boot_done.remove(&id);
        self.restoration_ready.remove(&id);
        self.restoration_queue.retain(|(engine, _)| *engine != id);
        self.restoration_reads
            .retain(|_, (engine, _)| *engine != id);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn input_generation(
        &self,
        engine: EngineId,
        surface: u32,
    ) -> Option<Option<tasty_terminal::ResourceGeneration>> {
        self.creations.iter().filter(|((id,_),_)|*id==engine).find_map(|(_,creation)|creation.input_generation(surface))
    }

    pub(crate) fn pauses_observation(&self) -> bool {
        self.commands.has_closing() || !self.resource_cleanups.is_empty() || self.creations
            .values()
            .any(creation::Creation::pauses_observation)
    }

    pub(crate) fn cleanup_poll_deadline(&self)->Option<std::time::Instant> {
        (!self.resource_cleanups.is_empty() || self.creations.values().any(creation::Creation::needs_cleanup_poll)).then(||std::time::Instant::now()+std::time::Duration::from_millis(10))
    }

    pub(crate) fn is_halted(&self) -> bool {
        self.halted.is_some()
    }

    pub(crate) fn reject_halted_request(
        &self,
        request: &crate::ipc::protocol::JsonRpcRequest,
    ) -> Option<crate::ipc::protocol::JsonRpcResponse> {
        if !self.is_halted() || request.method == "system.shutdown" {
            return None;
        }
        Some(crate::ipc::protocol::JsonRpcResponse::error(
            request.id.clone().unwrap_or(serde_json::Value::Null),
            -32000,
            "structure publication is halted; no live projection is available",
        ))
    }

    /// Stage selected startup resources while leaving inactive terminals and missing kinds pending.
    pub(crate) fn poll_restore_bootstrap(&mut self, session: &EngineSession) -> Result<(), String> {
        let id = session.id;
        #[cfg(feature = "gui")]
        if self.retirements.pending.contains_key(&id) {
            return Ok(());
        }
        if self.opening.contains_key(&id)
            || session.journal_binding.is_none()
            || self.has_creation(id)
            || self
                .restoration_queue
                .iter()
                .any(|(engine, _)| *engine == id)
            || self
                .restoration_reads
                .values()
                .any(|(engine, _)| *engine == id)
        {
            return Ok(());
        }
        let selected = crate::runtime::surface_restorer::initial_terminal_selection(
            &session.core_state,
            self.restored_views.get(&id),
        );
        let next = self
            .restoration_ready
            .get(&id)
            .and_then(|items| {
                items.iter().find(|item| {
                    if item.input.kind == "terminal" {
                        selected.contains(&item.surface_id)
                    } else {
                        session
                            .runtime.surface_registry
                            .get_live(&item.input.kind)
                            .is_some()
                    }
                })
            })
            .map(|item| item.surface_id);
        if let Some(surface_id) = next {
            self.activate_restored_surface(session, surface_id)?;
        } else {
            self.restoration_boot_done.insert(id);
        }
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(crate) fn take_restored_presentation(
        &mut self,
        id: EngineId,
    ) -> Option<crate::model::RestoredPresentation> {
        self.restored_views.remove(&id)
    }

    /// Selection is supplied by the application/View. Loading a payload alone grants no activation.
    pub(crate) fn activate_restored_surface(
        &mut self,
        session: &EngineSession,
        surface_id: u32,
    ) -> Result<bool, String> {
        #[cfg(feature = "gui")]
        if self.retirements.pending.contains_key(&session.id) {
            return Ok(false);
        }
        if self.is_halted() || self.has_creation(session.id) {
            return Ok(false);
        }
        let Some(items) = self.restoration_ready.get_mut(&session.id) else {
            return Ok(false);
        };
        let Some(index) = items.iter().position(|item| item.surface_id == surface_id) else {
            return Ok(false);
        };
        let request = items.remove(index);
        let ticket = self.next_ticket;
        self.next_ticket = ticket
            .checked_add(1)
            .ok_or("journal ticket range exhausted")?;
        let creation = creation::Creation::restore(ticket, session, &self.worker, request)?;
        self.creations.insert((session.id,creation.ticket), creation);
        Ok(true)
    }

    pub(super) fn has_creation(&self,id:EngineId)->bool {self.creations.keys().any(|(engine,_)|*engine==id)}
    pub(crate) fn has_pending_engine_effects(&self,id:EngineId)->bool {
        self.has_pending_capture(id)||self.has_creation(id)||self.has_resource_cleanup(id)||self.commands.has_resource_request(id)
    }
    #[cfg(feature="gui")]
    pub(crate) fn has_pending_view_for(&self,stream:&str)->bool {
        self.queued_view_writes.contains_key(stream)||self.view_writes.values().any(|view|view.binding.stream==stream)
    }

    pub(crate) fn is_ready(&self, id: EngineId) -> bool {
        self.started
            && self.halted.is_none()
            && !self.opening.contains_key(&id)
            && !self.has_creation(id)
            && !self.has_resource_cleanup(id)
            && !self
                .restoration_queue
                .iter()
                .any(|(engine, _)| *engine == id)
            && !self
                .restoration_reads
                .values()
                .any(|(engine, _)| *engine == id)
            && (self.restoration_boot_done.contains(&id)
                || !self
                    .restoration_ready
                    .get(&id)
                    .is_some_and(|items| !items.is_empty()))
    }

    fn submit_restore_reads(&mut self) -> Result<(), String> {
        const MAX_READS: usize = 8;
        while self.restoration_reads.len() < MAX_READS {
            let Some((engine, restoration)) = self.restoration_queue.pop_front() else {
                break;
            };
            let ticket = self.next_ticket;
            let request = Request {
                ticket,
                work: Work::ReadPayload(
                    restoration
                        .reference
                        .expect("only referenced captures enter the read queue"),
                ),
            };
            match self.worker.submit(request) {
                Ok(()) => {
                    self.next_ticket = ticket
                        .checked_add(1)
                        .ok_or("journal ticket range exhausted")?;
                    self.restoration_reads.insert(ticket, (engine, restoration));
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => {
                    self.restoration_queue.push_front((engine, restoration));
                    break;
                }
                Err(error) => return Err(format!("restore capture read: {error:?}")),
            }
        }
        Ok(())
    }

    #[cfg(feature = "gui")]
    pub(crate) fn queue_view(
        &mut self,
        mut view: crate::runtime::journal_product::view_record::StoredView,
    ) {
        if !self.is_halted() {
            let Some(sequence) = self.latest_view_sequence.checked_add(1) else {
                self.halted = Some("View checkpoint sequence exhausted".into());
                return;
            };
            self.latest_view_sequence = sequence;
            view.sequence = sequence;
            self.failed_view_writes.remove(&view.binding.stream);
            self.queued_view_writes
                .insert(view.binding.stream.clone(), view);
            (self.wake)();
        }
    }

    #[cfg(feature = "gui")]
    pub(crate) fn latest_view_sequence(&self) -> u64 {
        self.latest_view_sequence
    }

    #[cfg(feature = "gui")]
    pub(crate) fn has_pending_view_writes(&self) -> bool {
        !self.is_halted() && (!self.capture_requests.is_empty() || !self.captures.is_empty() || !self.queued_view_writes.is_empty() || !self.view_writes.is_empty())
    }

    #[cfg(feature = "gui")]
    fn submit_view_writes(&mut self) -> Result<(), String> {
        let ready: Vec<_> = self
            .queued_view_writes
            .keys()
            .filter(|stream| {
                !self
                    .view_writes
                    .values()
                    .any(|current| current.binding.stream == **stream)
            })
            .cloned()
            .take(8)
            .collect();
        for stream in ready {
            let view = self
                .queued_view_writes
                .remove(&stream)
                .expect("listed View write");
            let ticket = self.next_ticket;
            match self.worker.submit(Request {
                ticket,
                work: Work::SaveView(view.clone()),
            }) {
                Ok(()) => {
                    self.next_ticket = ticket
                        .checked_add(1)
                        .ok_or("journal ticket range exhausted")?;
                    self.view_writes.insert(ticket, view);
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => {
                    self.queued_view_writes.insert(stream, view);
                    break;
                }
                Err(error) => return Err(format!("View snapshot submission: {error:?}")),
            }
        }
        Ok(())
    }

    fn submit_openings(&mut self) -> Result<(), String> {
        for opening in self
            .opening
            .values_mut()
            .filter(|opening| opening.ticket.is_none())
        {
            let ticket = self.next_ticket;
            self.next_ticket = ticket
                .checked_add(1)
                .ok_or("journal ticket range exhausted")?;
            self.worker
                .submit(Request {
                    ticket,
                    work: Work::OpenEngine {
                        selection: opening.selection.clone(),
                        normal_category_name: "normal".into(),
                        surface_floor: opening.surface_floor,
                    },
                })
                .map_err(|error| format!("engine bootstrap submission: {error:?}"))?;
            opening.ticket = Some(ticket);
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod bulk_restore_tests;
#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, unix, feature = "gui"))]
mod view_tests;
