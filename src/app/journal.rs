//! Application side of the data-home worker. Engine bindings live on their EngineSession.
mod capture;
pub(crate) mod commands;
mod creation;
#[cfg(feature = "gui")]
pub(crate) mod forward;
mod id_reservations;
mod process_release;
mod publication;
mod resource_cleanup;
mod restorations;
#[cfg(feature = "gui")]
mod retirement;
#[cfg(feature = "gui")]
mod view_writes;

use crate::runtime::engine_session::{EngineId, EngineSession};
use crate::runtime::journal_product::{
    Completion, EngineSelection, JournalWorker, Request, ResultValue, Work,
};
#[cfg(feature = "gui")]
pub(crate) use capture::PresetCaptureNotice;
pub(crate) use capture::{PresetCaptureOutput, PresetCaptureReply};
pub(crate) use creation::{ActivationOutcome, ActivationReceipt};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tasty_core::projection;

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
    /// 엔진을 보여 주는 창의 터미널 영역. split 의 탐색기 하한 판정에만 쓰며 journal 에 저장하지 않는다.
    split_geometries: HashMap<EngineId, commands::split_floor::SplitGeometry>,
    wake: Arc<dyn Fn() + Send + Sync>,
    opening: HashMap<EngineId, Opening>,
    creations: HashMap<(EngineId, u64), creation::Creation>,
    #[cfg(feature = "gui")]
    forwards: std::collections::BTreeMap<u64, forward::Forward>,
    resource_cleanups: std::collections::BTreeMap<u64, resource_cleanup::Cleanup>,
    restorations: restorations::Restorations,
    restored_views: HashMap<EngineId, crate::model::RestoredPresentation>,
    #[cfg(feature = "gui")]
    view_writes: view_writes::ViewWrites,
    #[cfg(feature = "gui")]
    retirements: retirement::Retirements,
    known_slots: std::collections::BTreeMap<u32, bool>,
    started: bool,
    /// 저널을 열지 못한 이유. 부팅 오류 화면이 문구를 고를 때 읽는다.
    #[cfg(feature = "gui")]
    startup_failure: Option<crate::runtime::journal_product::StartupFailure>,
    runtime_epoch: Option<u64>,
    next_ticket: u64,
    halted: Option<String>,
}

impl JournalApplication {
    /// 엔진을 보여 주는 창의 터미널 영역을 기록한다. 기록이 없는 엔진의 split 에는 하한이 없다.
    #[cfg(feature = "gui")]
    pub(crate) fn update_split_geometry(
        &mut self,
        id: EngineId,
        geometry: commands::split_floor::SplitGeometry,
    ) {
        self.split_geometries.insert(id, geometry);
    }

    /// 창이 닫힌 엔진의 기록을 지운다.
    #[cfg(feature = "gui")]
    pub(crate) fn retain_split_geometries(&mut self, shown: impl Fn(EngineId) -> bool) {
        self.split_geometries.retain(|id, _| shown(*id));
    }

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

    #[cfg(any(test, not(feature = "gui")))]
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> anyhow::Result<Self> {
        Self::new_with_writer_lock(wake, None)
    }

    /// `writer_lock`은 부팅 첫머리에서 이 홈의 저널에 대해 선점한 잠금이다(GUI 부팅). 없으면
    /// worker가 저장소를 연 뒤 잠근다(헤드리스·테스트).
    pub(crate) fn new_with_writer_lock(
        wake: Arc<dyn Fn() + Send + Sync>,
        writer_lock: Option<tasty_event_store::WriterLock>,
    ) -> anyhow::Result<Self> {
        let home = tasty_utils::path::tasty_home()
            .ok_or_else(|| anyhow::anyhow!("data home unavailable for structure journal"))?;
        let worker = JournalWorker::spawn_with(home, wake.clone(), writer_lock)
            .map_err(anyhow::Error::msg)?;
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
            split_geometries: Default::default(),
            wake,
            opening: HashMap::new(),
            creations: HashMap::new(),
            resource_cleanups: Default::default(),
            #[cfg(feature = "gui")]
            forwards: Default::default(),
            restorations: Default::default(),
            restored_views: Default::default(),
            #[cfg(feature = "gui")]
            view_writes: Default::default(),
            #[cfg(feature = "gui")]
            retirements: Default::default(),
            known_slots: Default::default(),
            started: false,
            #[cfg(feature = "gui")]
            startup_failure: None,
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
        if let EngineSelection::ImportedSlot { source } = &selection
            && session.persistence.slot != Some(source.destination_slot)
        {
            return Err("import destination must be claimed by the opening engine".into());
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
        self.restorations.register(session.id);
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
                .is_some_and(|binding| self.view_writes.failed_for(&binding.stream))
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
        self.commands
            .has_closing(|operation| self.cleanup_awaits_receipts(operation))
            || self.cleanup_pauses_observation()
            || self
                .creations
                .values()
                .any(creation::Creation::pauses_observation)
    }

    /// Shutdown waits for publication and also for retirements that only await their receipts,
    /// so a close replies once and records its outcome. The receipt deadline bounds the wait.
    pub(crate) fn shutdown_waits_for_publication(&self) -> bool {
        self.pauses_observation() || self.cleanup_awaits_any_receipts()
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
            || self.restorations.has_pending(id)
        {
            return Ok(());
        }
        let selected = crate::runtime::surface_restorer::initial_terminal_selection(
            &session.core_state,
            self.restored_views.get(&id),
        );
        let next = self
            .restorations
            .ready(id)
            .iter()
            .find(|item| {
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
            .map(|item| item.surface_id);
        if let Some(surface_id) = next {
            self.activate_restored_surface(session, surface_id)?;
        } else {
            self.restorations.finish_bootstrap(id);
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
        let reading = self.restorations.is_reading(session.id, surface);
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
        let items = self.restorations.ready(session.id);
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
        let request = self.restorations.take_ready(session.id, index);
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
        self.view_writes.pending_for(stream)
    }

    /// 저널 worker가 다른 프로세스가 같은 홈을 쓰고 있어 시작하지 못했다.
    #[cfg(feature = "gui")]
    pub(crate) fn home_in_use(&self) -> bool {
        self.startup_failure
            .as_ref()
            .is_some_and(crate::runtime::journal_product::StartupFailure::is_home_in_use)
    }

    pub(crate) fn is_ready(&self, id: EngineId) -> bool {
        self.started
            && self.halted.is_none()
            && !self.opening.contains_key(&id)
            && !self.has_creation(id)
            && !self.has_resource_cleanup(id)
            && self.restorations.is_ready(id)
    }

    fn submit_restore_reads(&mut self) -> Result<(), String> {
        self.restorations
            .submit(&self.worker, &mut self.next_ticket)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn queue_view(
        &mut self,
        view: crate::runtime::journal_product::view_record::StoredView,
    ) {
        if !self.is_halted() {
            if let Err(error) = self.view_writes.queue(view) {
                self.halted = Some(error);
                return;
            }
            (self.wake)();
        }
    }
    #[cfg(feature = "gui")]
    pub(crate) fn latest_view_sequence(&self) -> u64 {
        self.view_writes.sequence()
    }
    #[cfg(feature = "gui")]
    pub(crate) fn has_pending_view_writes(&self) -> bool {
        !self.is_halted()
            && (self.has_pending_preset_captures()
                || !self.capture_requests.is_empty()
                || !self.captures.is_empty()
                || self.view_writes.pending())
    }
    #[cfg(feature = "gui")]
    fn submit_view_writes(&mut self) -> Result<(), String> {
        self.view_writes.submit(&self.worker, &mut self.next_ticket)
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
#[cfg(test)]
mod stall_budget;
#[cfg(test)]
mod test_shell;
#[cfg(all(test, unix))]
mod tests;

#[cfg(all(test, unix, feature = "gui"))]
mod view_tests;
