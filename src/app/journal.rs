//! Application side of the data-home worker. Engine bindings live on their EngineSession.
use crate::runtime::engine_session::{EngineId, EngineSession};
use crate::runtime::journal_product::{
    Completion, EngineSelection, JournalWorker, Request, ResultValue, Work,
};
use tasty_core::projection;
pub(crate) mod commands;
mod creation;
pub(crate) use creation::{ActivationOutcome, ActivationReceipt};
mod capture;
#[cfg(feature = "gui")]
pub(crate) mod forward;
mod process_release;
mod publication;
mod resource_cleanup;
#[cfg(feature = "gui")]
pub(crate) use capture::PresetCaptureNotice;
pub(crate) use capture::{PresetCaptureOutput, PresetCaptureReply};
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
    captures: std::collections::BTreeMap<u64, capture::PendingCapture>,
    preset_captures: capture::PresetCaptures,
    capture_requests: HashMap<EngineId, bool>,
    execution_id_requests: HashMap<u64, EngineId>,
    replacements: Vec<(EngineId, tasty_core::Replacement)>,
    changed_engines: std::collections::HashSet<EngineId>,
    completion_views: HashMap<EngineId, crate::runtime::journal_product::CompletionView>,
    wake: Arc<dyn Fn() + Send + Sync>,
    opening: HashMap<EngineId, Opening>,
    creations: HashMap<(EngineId, u64), creation::Creation>,
    #[cfg(feature = "gui")]
    forwards: std::collections::BTreeMap<u64, forward::Forward>,
    resource_cleanups: std::collections::BTreeMap<u64, resource_cleanup::Cleanup>,
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
    runtime_epoch: Option<u64>,
    next_ticket: u64,
    halted: Option<String>,
}

impl JournalApplication {
    pub(crate) fn update_completion_view(
        &mut self,
        id: EngineId,
        core: &crate::core::CoreState,
        presentation: &dyn crate::model::StructurePresentation,
    ) {
        let mut focused_panes = std::collections::BTreeMap::new();
        let mut selected_tabs = std::collections::BTreeMap::new();
        let mut selected_surfaces = std::collections::BTreeMap::new();
        for workspace in &core.workspaces() {
            if let Some(pane) = presentation.pane_id(workspace) {
                focused_panes.insert(workspace.id, pane);
            }
            for pane in workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pane) {
                    if let Some(tab) = pane.tabs.get(presentation.tab_index(pane)) {
                        selected_tabs.insert(pane.id, tab.id);
                    }
                    for tab in &pane.tabs {
                        if let Some(surface) = presentation.surface_id(tab) {
                            selected_surfaces.insert(tab.id, surface);
                        }
                    }
                }
            }
        }
        self.completion_views.insert(
            id,
            crate::runtime::journal_product::CompletionView {
                focused_panes,
                mirror_count: core.mirror_workspaces().len(),
                selected_tabs,
                selected_surfaces,
            },
        );
    }

    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<Self> {
        let home = tasty_utils::path::tasty_home()
            .ok_or_else(|| anyhow::anyhow!("data home unavailable for structure journal"))?;
        let worker = JournalWorker::spawn(home, wake.clone()).map_err(anyhow::Error::msg)?;
        Ok(Self {
            worker,
            commands: Default::default(),
            captures: Default::default(),
            preset_captures: Default::default(),
            capture_requests: Default::default(),
            execution_id_requests: Default::default(),
            changed_engines: Default::default(),
            replacements: Vec::new(),
            completion_views: Default::default(),
            wake,
            opening: HashMap::new(),
            creations: HashMap::new(),
            resource_cleanups: Default::default(),
            #[cfg(feature = "gui")]
            forwards: Default::default(),
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
            runtime_epoch: None,
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
        if let EngineSelection::ImportedSlot { source } = &selection {
            if session.persistence.slot != Some(source.destination_slot) {
                return Err("import destination must be claimed by the opening engine".into());
            }
        }
        let scopes = session
            .runtime
            .memory
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
        plugins: Option<&mut crate::plugin::PluginManager>,
    ) -> Result<(), String> {
        if let Some(reason) = &self.halted {
            return Err(reason.clone());
        }
        let result = self.poll_initial(sessions, plugins);
        #[cfg(feature = "gui")]
        for session in sessions {
            if session
                .journal_binding
                .as_ref()
                .is_some_and(|binding| self.failed_view_writes.contains_key(&binding.stream))
            {
                session.persistence.dirty.mark_dirty();
            }
        }
        if let Err(reason) = &result {
            self.halted = Some(reason.clone());
            self.fail_pending_commands(reason);
        }
        result
    }

    #[cfg(feature = "gui")]
    pub(crate) fn halt_reason(&self) -> Option<&str> {
        self.halted.as_deref()
    }

    #[cfg(feature = "gui")]
    pub(crate) fn input_generation(
        &self,
        engine: EngineId,
        surface: u32,
    ) -> Option<Option<tasty_terminal::ResourceGeneration>> {
        self.creations
            .iter()
            .filter(|((id, _), _)| *id == engine)
            .find_map(|(_, creation)| creation.input_generation(surface))
    }

    pub(crate) fn pauses_observation(&self) -> bool {
        self.commands.has_closing()
            || self.cleanup_pauses_observation()
            || self
                .creations
                .values()
                .any(creation::Creation::pauses_observation)
    }

    pub(crate) fn cleanup_poll_deadline(&self) -> Option<std::time::Instant> {
        self.resource_cleanup_deadline()
            .into_iter()
            .chain(
                self.creations
                    .values()
                    .filter_map(creation::Creation::cleanup_deadline),
            )
            .min()
    }

    #[cfg(not(feature = "gui"))]
    pub(crate) fn runtime_epoch(&self) -> Option<u64> {
        self.runtime_epoch
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
                            .runtime
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
    pub(crate) fn wake_application(&self) {
        (self.wake)();
    }
    pub(crate) fn request_restore_receipt(
        &mut self,
        session: &EngineSession,
        surface: u32,
        activation: Option<u64>,
    ) -> Result<Option<ActivationReceipt>, String> {
        if self.is_halted() {
            return Err("resource publication is halted".into());
        }
        if let Some(reason) =
            crate::runtime::surface_restorer::recovery_blocked_reason(session, surface)
        {
            return Err(reason.to_owned());
        }
        if let Some(receipt) = self
            .creations
            .iter()
            .filter(|((engine, _), _)| *engine == session.id)
            .find_map(|(_, creation)| creation.join_restore(surface, activation))
        {
            return Ok(Some(receipt));
        }
        if self.has_creation(session.id) {
            return Ok(None);
        }
        if session
            .core_state
            .find_surface_by_id(surface)
            .and_then(|value| value.activation_generation)
            != activation
        {
            return Err("activation target changed".into());
        }
        if self.activate_restored_surface(session, surface)? {
            return Ok(self
                .creations
                .iter()
                .filter(|((engine, _), _)| *engine == session.id)
                .find_map(|(_, creation)| creation.join_restore(surface, activation)));
        }
        let reading =
            self.restoration_queue
                .iter()
                .any(|(engine, request)| *engine == session.id && request.surface_id == surface)
                || self.restoration_reads.values().any(|(engine, request)| {
                    *engine == session.id && request.surface_id == surface
                });
        if reading {
            Ok(None)
        } else {
            Err("selected resource has no pending restore input".into())
        }
    }

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
        if items[index].input.kind != "terminal"
            && session
                .runtime
                .surface_registry
                .get_live(&items[index].input.kind)
                .is_none()
        {
            return Ok(false);
        }
        let request = items.remove(index);
        let ticket = self.next_ticket;
        self.next_ticket = ticket
            .checked_add(1)
            .ok_or("journal ticket range exhausted")?;
        let creation = creation::Creation::restore(ticket, session, &self.worker, request)?;
        self.creations
            .insert((session.id, creation.ticket), creation);
        (self.wake)();
        Ok(true)
    }

    pub(super) fn has_creation(&self, id: EngineId) -> bool {
        self.creations.keys().any(|(engine, _)| *engine == id)
    }
    #[cfg(feature = "gui")]
    pub(super) fn has_forward(&self, id: EngineId) -> bool {
        self.forwards.values().any(|forward| forward.engine == id)
    }
    #[cfg(feature = "gui")]
    pub(crate) fn has_pending_engine_effects(&self, id: EngineId) -> bool {
        self.commands.has_remote_request(id)
            || self.has_forward(id)
            || self.has_pending_capture(id)
            || self.has_pending_preset_capture(id)
            || self.has_creation(id)
            || self.has_resource_cleanup(id)
            || self.commands.has_resource_request(id)
    }
    #[cfg(feature = "gui")]
    pub(crate) fn has_pending_view_for(&self, stream: &str) -> bool {
        self.queued_view_writes.contains_key(stream)
            || self
                .view_writes
                .values()
                .any(|view| view.binding.stream == stream)
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
        !self.is_halted()
            && (self.has_pending_preset_captures()
                || !self.capture_requests.is_empty()
                || !self.captures.is_empty()
                || !self.queued_view_writes.is_empty()
                || !self.view_writes.is_empty())
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
