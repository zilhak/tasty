//! Application side of the data-home worker. Engine bindings live on their EngineSession.
use crate::runtime::engine_session::{EngineId, EngineSession};
use crate::runtime::journal_product::{
    Completion, EngineSelection, JournalWorker, Request, ResultValue, Work,
};
use crate::runtime::live_projection;
mod creation;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

struct Opening {
    selection: EngineSelection,
    ticket: Option<u64>,
    projected: bool,
    surface_floor: u32,
}

pub(crate) struct JournalApplication {
    worker: JournalWorker,
    wake: Arc<dyn Fn() + Send + Sync>,
    opening: HashMap<EngineId, Opening>,
    creations: HashMap<EngineId, creation::Creation>,
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
    started: bool,
    next_ticket: u64,
    halted: Option<String>,
}

impl JournalApplication {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<Self> {
        let home = tasty_utils::path::tasty_home()
            .ok_or_else(|| anyhow::anyhow!("data home unavailable for structure journal"))?;
        let worker = JournalWorker::spawn(home, wake.clone()).map_err(anyhow::Error::msg)?;
        Ok(Self {
            worker,
            wake,
            opening: HashMap::new(),
            creations: HashMap::new(),
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
            .core_state
            .memory
            .lock()
            .map_err(|error| error.to_string())?
            .scopes()
            .map_err(|error| format!("cannot establish existing surface ID floor: {error}"))?;
        let surface_floor = scopes
            .iter()
            .filter_map(|scope| match tasty_memory::Scope::parse(scope) {
                Ok(tasty_memory::Scope::Surface(id))
                    if crate::core::terminal_store::is_surface_id_space(id) =>
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
            },
        );
        Ok(())
    }

    /// The caller holds newly opening engines behind its startup read/render barrier.
    pub(crate) fn poll_bootstrap(
        &mut self,
        sessions: &mut [&mut EngineSession],
    ) -> Result<(), String> {
        if let Some(reason) = &self.halted {
            return Err(reason.clone());
        }
        let result = self.poll_initial(sessions);
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
        }
        result
    }

    fn poll_initial(&mut self, sessions: &mut [&mut EngineSession]) -> Result<(), String> {
        const MAX_COMPLETIONS: usize = 16;
        for _ in 0..MAX_COMPLETIONS {
            if self.started {
                self.submit_openings()?;
                self.submit_restore_reads()?;
                #[cfg(feature = "gui")]
                self.submit_view_writes()?;
                for creation in self.creations.values_mut() {
                    creation.poll_cleanup(&self.worker)?;
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
                    for session in sessions.iter_mut() {
                        let Some(opening) = self.opening.get_mut(&session.id) else {
                            continue;
                        };
                        let model = match opening.selection {
                            EngineSelection::Slot { slot, resume: true } => bootstrap
                                .streams
                                .remove(&format!("structure:slot-{slot}"))
                                .unwrap_or_default(),
                            _ => tasty_domain::JournalModel::default(),
                        };
                        live_projection::bootstrap::initialize(&mut session.core_state, &model)?;
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
                Completion::StartupFailed(error) => return Err(error),
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
                            opening.projected = true;
                        } else {
                            let mut prepared = live_projection::PreparedLeaves::new();
                            let mut installation = None;
                            if let Some(creation) = self.creations.get_mut(&session.id) {
                                installation = creation.authorize_installation(session, events)?;
                                if let Some(leaf) =
                                    creation.leaf_for_publication(session, events)?
                                {
                                    prepared.insert(
                                        leaf.surface.surface_id().expect("prepared surface ID"),
                                        leaf,
                                    );
                                }
                            }
                            live_projection::apply(
                                &mut session.core_state,
                                predecessor,
                                &domain,
                                &mut prepared,
                                &mut Vec::new(),
                            )?;
                            if let Some(installation) = installation {
                                let retiring = session
                                    .core_state
                                    .find_surface_by_id(installation.surface_id())
                                    .map(crate::runtime::effect_runner::RetiringKind::capture);
                                let installed = installation
                                    .install(&mut session.borrow_mut(), None, retiring)
                                    .map_err(|error| error.to_string())?;
                                self.creations
                                    .get_mut(&session.id)
                                    .expect("materialization request")
                                    .installed(installed);
                            }
                        }
                        if let Some(binding) = session.journal_binding.as_mut() {
                            binding.published_cut = Some(batch.batch_id);
                            if let Some(last) = events.last() {
                                binding.revision = Some(last.revision);
                            }
                        }
                    }
                    self.worker
                        .acknowledge(batch.batch_id, Ok(()))
                        .map_err(|error| format!("bootstrap publication ACK: {error:?}"))?;
                }
                Completion::Finished { ticket, result } => {
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
                        let ResultValue::Payload { reference, bytes } = result? else {
                            return Err("restore payload request returned another value".into());
                        };
                        let session = sessions
                            .iter()
                            .find(|session| session.id == engine)
                            .ok_or("restoring engine disappeared")?;
                        let current = session
                            .core_state
                            .find_surface_by_id(restoration.surface_id)
                            .and_then(|surface| {
                                surface
                                    .as_any()
                                    .downcast_ref::<live_projection::bootstrap::JournalPlaceholder>(
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
                    if let Some(id) = self
                        .creations
                        .iter()
                        .find_map(|(id, creation)| (creation.ticket == ticket).then_some(*id))
                    {
                        let session = sessions
                            .iter_mut()
                            .find(|session| session.id == id)
                            .ok_or("creating engine disappeared")?;
                        if self
                            .creations
                            .get_mut(&id)
                            .expect("creation exists")
                            .answered(&self.worker, session, result?)?
                        {
                            self.creations.remove(&id);
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
                    }
                    self.restored_views.insert(
                        session.id,
                        live_projection::bootstrap::presentation(
                            &session.core_state,
                            bound.imported_view,
                        ),
                    );
                    session.core_state.pending_layout_restore = None;
                    session.journal_binding = Some(bound.binding);
                    if session.core_state.local_workspaces.is_empty() {
                        let ticket = self.next_ticket;
                        self.next_ticket = ticket
                            .checked_add(1)
                            .ok_or("journal ticket range exhausted")?;
                        let creation =
                            creation::Creation::default_workspace(ticket, session, &self.worker)?;
                        self.creations.insert(session.id, creation);
                    } else {
                        for restoration in
                            crate::runtime::surface_restorer::describe(&session.core_state)
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
        self.creations.remove(&id);
        self.restored_views.remove(&id);
        self.restoration_boot_done.remove(&id);
        self.restoration_ready.remove(&id);
        self.restoration_queue.retain(|(engine, _)| *engine != id);
        self.restoration_reads
            .retain(|_, (engine, _)| *engine != id);
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
        if self.opening.contains_key(&id)
            || session.journal_binding.is_none()
            || self.creations.contains_key(&id)
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
                            .core_state
                            .surface_registry
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
        if self.is_halted() || self.creations.contains_key(&session.id) {
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
        self.creations.insert(session.id, creation);
        Ok(true)
    }

    pub(crate) fn is_ready(&self, id: EngineId) -> bool {
        self.started
            && self.halted.is_none()
            && !self.opening.contains_key(&id)
            && !self.creations.contains_key(&id)
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
        !self.is_halted() && (!self.queued_view_writes.is_empty() || !self.view_writes.is_empty())
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
