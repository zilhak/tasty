//! Keep precommit work ordered while unrelated engines can pass a committed effect wait.
use super::*;

impl Reply {
    pub(super) fn engine_scope(&self) -> Option<EngineId> {
        match self {
            Self::Resume(resume) => Some(resume.engine),
            Self::Remote(remote) => Some(remote.engine),
            Self::Intent { engine, .. } => Some(*engine),
            #[cfg(feature = "gui")]
            Self::Divider { engine, .. } => Some(*engine),
            _ => None,
        }
    }
}
impl Commands {
    fn can_advance(&self, ticket: u64, engine: Option<EngineId>) -> bool {
        let metadata = self
            .pending
            .get(&ticket)
            .is_some_and(|pending| pending.ordered_metadata);
        self.pending.range(..ticket).all(|(_, previous)| {
            if metadata
                && previous.ordered_metadata
                && engine.is_some()
                && engine == previous.engine_scope
                && previous.admitted
                && !previous.needs_resolution
                && previous.waiting_command.is_none()
                && (previous.in_flight || matches!(previous.queued, Some(Work::Resolve { .. })))
            {
                return true;
            }
            // An unresolved scope is a global barrier. A blocked, already bound proposal does
            // not block a third engine, but an active reserve/prepare/commit sequence still does.
            engine.is_some()
                && previous.engine_scope.is_some()
                && engine != previous.engine_scope
                && (previous.waiting_command.is_some() || previous.needs_resolution)
        })
    }
    pub(super) fn submittable(&self, ticket: u64, pending: &Pending) -> bool {
        matches!(
            &pending.queued,
            Some(Work::ReadCommand(_) | Work::CancelAdmission)
        ) || matches!(&pending.queued, Some(Work::Resolve { changes, .. }) if changes.is_empty())
            || self.can_advance(ticket, pending.engine_scope)
    }
}
impl JournalApplication {
    /// Classify an explicit local patch before the first scheduling check, including GUI/headless dispatch.
    pub(super) fn bind_ipc_command_engine(
        &mut self,
        ticket: u64,
        engine: EngineId,
        core: &crate::core::CoreState,
    ) -> bool {
        let metadata = self
            .commands
            .pending
            .get(&ticket)
            .is_some_and(|pending| fixed_metadata(&pending.request, core));
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.ordered_metadata = metadata;
        }
        self.bind_command_engine(ticket, engine)
    }

    /// Fix the engine before resolving targets or starting any preparation. Repeated proposals
    /// use this original scope even if GUI selection changes during the wait.
    pub(crate) fn bind_command_engine(&mut self, ticket: u64, engine: EngineId) -> bool {
        let Some(pending) = self.commands.pending.get_mut(&ticket) else {
            return false;
        };
        if pending
            .engine_scope
            .is_some_and(|original| original != engine)
        {
            self.reject_resolved_request(
                ticket,
                JsonRpcResponse::invalid_params(
                    serde_json::Value::Null,
                    "structural request engine changed",
                ),
            );
            return false;
        }
        pending.engine_scope = Some(engine);
        let allowed = self.commands.can_advance(ticket, Some(engine));
        self.commands
            .pending
            .get_mut(&ticket)
            .expect("bound command")
            .needs_resolution = !allowed;
        allowed
    }
}

pub(super) fn fixed_metadata(request: &JsonRpcRequest, core: &crate::core::CoreState) -> bool {
    if request.method != "workspace.update" {
        return false;
    }
    let Some(params) = request.params.as_object() else {
        return false;
    };
    if params
        .keys()
        .any(|key| !matches!(key.as_str(), "id" | "name" | "subtitle" | "description"))
    {
        return false;
    }
    let Some(id) = params
        .get("id")
        .and_then(|id| id.as_u64())
        .and_then(|id| u32::try_from(id).ok())
    else {
        return false;
    };
    core.workspaces()
        .iter()
        .any(|workspace| workspace.id == id && !workspace.mirror)
}
