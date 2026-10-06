//! Resolve close compatibility once, then commit its tombstone and exact cleanup obligation.
use super::*;
#[derive(Clone, Copy, Default)]
pub(super) enum Cause {
    #[default]
    Ordinary,
    ProcessExit(tasty_terminal::ResourceGeneration),
    RemoteHolder {
        client: u32,
        workspace: u32,
    },
}
pub(super) struct Request {
    #[cfg(feature = "gui")]
    pub engine: EngineId,
    pub binding: crate::runtime::journal_product::EngineBinding,
    pub target: tasty_core::CloseTarget,
    pub response: JsonRpcResponse,
    pub not_closed: JsonRpcResponse,
    pub is_user_close: bool,
    pub remote_user_close: bool,
    pub input_ref: Option<tasty_core::DataRef>,
    pub replacement: bool,
    pub undo: Option<tasty_core::UndoCapture>,
    pub expected: Vec<tasty_core::RetiredSurface>,
    pub capture_undo: bool,
}
impl Request {
    pub fn resolve(
        request: &JsonRpcRequest,
        session: &EngineSession,
        cause: Cause,
    ) -> Result<Self, JsonRpcResponse> {
        if request.method == "terminal.kill" {
            let id = serde_json::Value::Null;
            let parent =
                crate::ipc::handler::params::optional_u32(&request.params, "surface", &id)?
                    .or_else(|| session.runtime.child_terminals.single_parent())
                    .ok_or_else(|| {
                        JsonRpcResponse::invalid_params(
                            id.clone(),
                            "missing 'surface' parameter (0 or >1 parents — specify --surface)",
                        )
                    })?;
            let index = crate::ipc::handler::params::require_u32(&request.params, "child", &id)?;
            let child = session
                .runtime
                .child_terminals
                .find_child(parent, index)
                .ok_or_else(|| {
                    JsonRpcResponse::invalid_params(
                        id.clone(),
                        format!("child {index} not found for parent {parent}"),
                    )
                })?;
            let mut normalized = request.clone();
            normalized.method = "surface.close".into();
            normalized.params["surface_id"] = serde_json::json!(child.child_surface_id);
            let mut resolved = Self::resolve(&normalized, session, cause)?;
            resolved.response = JsonRpcResponse::success(
                id,
                serde_json::json!({"killed_surface_id":child.child_surface_id,"child_index":index}),
            );
            return Ok(resolved);
        }
        if request.method == "intent.close" {
            let target: tasty_core::CloseTarget =
                serde_json::from_value(request.params["target"].clone()).map_err(|error| {
                    JsonRpcResponse::invalid_params(serde_json::Value::Null, error.to_string())
                })?;
            let mut normalized = request.clone();
            let (method, field, value) = match target {
                tasty_core::CloseTarget::Workspace(id) => ("workspace.close", "id", id),
                tasty_core::CloseTarget::Pane(id) => ("pane.close", "pane_id", id),
                tasty_core::CloseTarget::Tab(id) => ("tab.close", "tab_id", id),
                tasty_core::CloseTarget::Surface(id) => ("surface.close_self", "surface_id", id),
            };
            normalized.method = method.into();
            normalized.params = serde_json::json!({});
            normalized.params[field] = serde_json::json!(value);
            let mut resolved = if matches!(target, tasty_core::CloseTarget::Workspace(_)) {
                Self::workspace(&normalized, session, true)?
            } else {
                Self::resolve(&normalized, session, cause)?
            };
            if let Some(expected) = request.params.get("expected_activation") {
                let expected: Option<u64> =
                    serde_json::from_value(expected.clone()).map_err(|error| {
                        JsonRpcResponse::invalid_params(serde_json::Value::Null, error.to_string())
                    })?;
                if resolved
                    .expected
                    .first()
                    .is_none_or(|target| target.activation_generation != expected)
                {
                    return Err(JsonRpcResponse::invalid_params(
                        serde_json::Value::Null,
                        "close belongs to a retired surface activation",
                    ));
                }
            }
            resolved.capture_undo = request.params["capture"].as_bool().unwrap_or(false);
            resolved.is_user_close = request.params["user_close"].as_bool().unwrap_or(false);
            return Ok(resolved);
        }
        if request.method == "workspace.close" {
            return Self::workspace(request, session, false);
        }
        use crate::ipc::handler::params;
        use tasty_core::CloseTarget as T;
        let id = serde_json::Value::Null;
        let bad = |reason: String| JsonRpcResponse::invalid_params(id.clone(), reason);
        let (field, target) = match request.method.as_str() {
            "tab.close" => ("tab_id", 0),
            "pane.close" => ("pane_id", 1),
            "surface.close" | "surface.close_self" => ("surface_id", 2),
            _ => return Err(bad("unsupported close request".into())),
        };
        let value = params::opt_int::<u32>(&request.params, field, &id)?
            .ok_or_else(|| bad(format!("Missing required '{field}' parameter")))?;
        let target = match target {
            0 => T::Tab(value),
            1 => T::Pane(value),
            _ => T::Surface(value),
        };
        let engine = session.as_ref();
        let core = engine.core;
        if let Cause::ProcessExit(generation) = cause
            && !matches!(target,T::Surface(surface) if engine.runtime.terminals.matches_generation(surface,generation))
        {
            return Err(bad("PTY exit belongs to a retired physical owner".into()));
        }
        if let Cause::RemoteHolder { client, workspace } = cause
            && engine.live.occupancy.workspace_holder(workspace) != Some(client)
        {
            return Err(bad("remote workspace holder changed".into()));
        }
        let (workspace, targets, closes_workspace) = match target {
            T::Tab(tab_id) => {
                let pane = core
                    .find_pane_for_tab(tab_id)
                    .and_then(|id| core.find_pane_by_id(id));
                let tab = pane.and_then(|pane| pane.tabs.iter().find(|tab| tab.id == tab_id));
                let workspace = pane
                    .and_then(|pane| core.find_workspace_index_for_pane(pane.id))
                    .and_then(|index| core.workspace_at(index));
                (
                    workspace,
                    tab.map(|tab| tab.all_surface_ids()).unwrap_or_default(),
                    false,
                )
            }
            T::Pane(pane_id) => {
                let pane = core
                    .find_pane_by_id(pane_id)
                    .ok_or_else(|| bad(format!("Pane {pane_id} not found")))?;
                let workspace = core
                    .find_workspace_index_for_pane(pane_id)
                    .and_then(|index| core.workspace_at(index));
                (
                    workspace,
                    pane.tabs
                        .iter()
                        .flat_map(|tab| tab.all_surface_ids())
                        .collect(),
                    false,
                )
            }
            T::Surface(surface) => {
                let workspace = core
                    .find_workspace_index_for_surface(surface)
                    .and_then(|(index, _)| core.workspace_at(index));
                let closes =
                    workspace.is_some_and(|workspace| workspace.all_surface_ids() == vec![surface]);
                (
                    workspace,
                    core.has_surface(surface)
                        .then_some(surface)
                        .into_iter()
                        .collect(),
                    closes,
                )
            }
            T::Workspace(_) => unreachable!("workspace resolver is separate"),
        };
        if let Cause::RemoteHolder {
            workspace: expected,
            ..
        } = cause
            && workspace.is_none_or(|workspace| workspace.id != expected)
        {
            return Err(bad(
                "remote close target is outside its held workspace".into()
            ));
        }
        if let Some(response) = caller_refusal(request, core) {
            return Err(response);
        }
        // Remote structural effects are admitted separately; a mirror is never a local journal fact.
        if workspace.is_some_and(|workspace| workspace.mirror) {
            return Err(bad(
                "mirror close requires the bound remote structural effect".into(),
            ));
        }
        if matches!(cause, Cause::Ordinary)
            && let Some(surface) = targets
                .iter()
                .find(|id| engine.live.occupancy.is_hard_occupied(**id))
        {
            return Err(bad(format!(
                "Surface {surface} is occupied by a remote attach session (hard-occupied) — someone is working in that terminal right now. Release it from the attaching instance first."
            )));
        }
        let mut success = serde_json::json!({"closed":true});
        success[field] = serde_json::json!(value);
        let mut no_op = success.clone();
        no_op["closed"] = serde_json::json!(false);
        no_op["reason"] = serde_json::json!(match target {
            T::Tab(_) => "tab not found or cannot close the last tab",
            T::Pane(_) => "cannot close the last pane",
            _ => "surface not found",
        });
        Ok(Self {
            #[cfg(feature = "gui")]
            engine: session.id,
            binding: session.journal_binding.clone().ok_or_else(|| {
                JsonRpcResponse::internal_error(id.clone(), "engine has no journal binding")
            })?,
            target,
            response: JsonRpcResponse::success(id.clone(), success),
            not_closed: JsonRpcResponse::success(id, no_op),
            is_user_close: false,
            remote_user_close: false,
            input_ref: None,
            replacement: closes_workspace && core.workspaces().len() == 1,
            undo: None,
            expected: targets
                .into_iter()
                .filter_map(|id| core_expected(session, id))
                .collect(),
            capture_undo: false,
        })
    }
    pub fn workspace(
        request: &JsonRpcRequest,
        session: &EngineSession,
        allow_last: bool,
    ) -> Result<Self, JsonRpcResponse> {
        use crate::ipc::handler::params;
        let engine = session.as_ref();
        let id = serde_json::Value::Null;
        let bad = |reason: String| JsonRpcResponse::invalid_params(id.clone(), reason);
        let explicit = params::optional_u32(&request.params, "id", &id)?;
        let index = if let Some(workspace) = explicit {
            engine
                .find_workspace_index_for_id(workspace)
                .ok_or_else(|| bad(format!("Workspace {workspace} not found")))?
        } else if let Some(index) = params::opt_int::<u64>(&request.params, "index", &id)? {
            usize::try_from(index).map_err(|_| bad("Workspace index out of range".into()))?
        } else {
            return Err(bad("Missing required 'id' or 'index' parameter".into()));
        };
        let workspace = engine.workspace_at(index).ok_or_else(|| {
            bad(format!(
                "Workspace index {index} out of range (0..{})",
                engine.workspaces().len()
            ))
        })?;
        if let Some(caller) = crate::ipc::handler::caller_surface_id(&request.params)
            && workspace.all_surface_ids().contains(&caller)
        {
            return Err(bad("Cannot close a workspace that contains your own surface. Move elsewhere first, or use 'tasty close self' to close just your surface.".into()));
        }
        if workspace.mirror {
            return Err(bad("Workspace is a mirror of a remote attach session — detach it from that session instead of closing it here".into()));
        }
        if let Some(surface) = workspace
            .all_surface_ids()
            .into_iter()
            .find(|id| engine.live.occupancy.is_hard_occupied(*id))
        {
            return Err(bad(format!(
                "Workspace holds surface {surface}, which is occupied by a remote attach session (hard-occupied) — someone is working in that terminal right now. Release it from the attaching instance first."
            )));
        }
        if !allow_last && engine.workspaces().len() == 1 {
            return Err(bad(crate::ipc::handler::workspace::last_workspace_refusal(
            )
            .into()));
        }
        Ok(Self {
            #[cfg(feature = "gui")]
            engine: session.id,
            binding: session.journal_binding.clone().ok_or_else(|| {
                JsonRpcResponse::internal_error(id.clone(), "engine has no journal binding")
            })?,
            target: tasty_core::CloseTarget::Workspace(workspace.id),
            response: JsonRpcResponse::success(
                id.clone(),
                serde_json::json!({"closed":true,"id":workspace.id}),
            ),
            not_closed: JsonRpcResponse::success(
                id,
                serde_json::json!({"closed":false,"id":workspace.id}),
            ),
            is_user_close: false,
            remote_user_close: false,
            input_ref: None,
            replacement: false,
            undo: None,
            expected: workspace
                .all_surface_ids()
                .into_iter()
                .filter_map(|id| core_expected(session, id))
                .collect(),
            capture_undo: false,
        })
    }
    pub fn input(
        &self,
        session: &EngineSession,
        view: Option<&crate::runtime::journal_product::CompletionView>,
    ) -> Result<Work, String> {
        if !self.capture_undo {
            return Ok(Work::PutPayload(
                serde_json::to_vec(&self.target).map_err(|error| error.to_string())?,
            ));
        }
        let core = &session.core_state;
        let selected: std::collections::HashSet<_> = match self.target {
            tasty_core::CloseTarget::Workspace(id) => core
                .find_workspace_index_for_id(id)
                .and_then(|index| core.workspace_at(index))
                .map(|workspace| workspace.all_surface_ids())
                .unwrap_or_default(),
            tasty_core::CloseTarget::Pane(id) => core
                .find_pane_by_id(id)
                .map(|pane| {
                    pane.tabs
                        .iter()
                        .flat_map(|tab| tab.all_surface_ids())
                        .collect()
                })
                .unwrap_or_default(),
            tasty_core::CloseTarget::Tab(id) => core
                .find_pane_for_tab(id)
                .and_then(|pane| core.find_pane_by_id(pane))
                .and_then(|pane| pane.tabs.iter().find(|tab| tab.id == id))
                .map(|tab| tab.all_surface_ids())
                .unwrap_or_default(),
            tasty_core::CloseTarget::Surface(id) => {
                core.find_workspace_index_for_surface(id)
                    .and_then(|(index, _)| core.workspace_at(index))
                    .map(|workspace| {
                        // Closing its last leaf cascades through the containing structures, but all removed
                        // leaves still consist of this one ID.
                        workspace
                            .all_surface_ids()
                            .into_iter()
                            .filter(|surface| *surface == id)
                            .collect()
                    })
                    .unwrap_or_default()
            }
        }
        .into_iter()
        .collect();
        let display_name = if let tasty_core::CloseTarget::Surface(id) = self.target {
            core.find_tab_for_surface(id)
                .and_then(|tab| {
                    core.find_pane_for_tab(tab)
                        .and_then(|pane| core.find_pane_by_id(pane))
                        .and_then(|pane| pane.tabs.iter().find(|candidate| candidate.id == tab))
                })
                .map(|tab| {
                    session.as_ref().tab_display_name(
                        tab,
                        view.and_then(|view| view.selected_surfaces.get(&tab.id).copied())
                            .or_else(|| tab.first_surface_id()),
                    )
                })
        } else {
            None
        };
        Ok(Work::CaptureClosed {
            view: view.cloned().unwrap_or_default(),
            binding: self.binding.clone(),
            target: self.target,
            display_name,
            surfaces: crate::runtime::surface_capture::capture_selected(session, Some(&selected))?,
        })
    }
    pub fn stored(&self, input: tasty_core::DataRef) -> Work {
        Work::Resolve {
            changes: vec![StreamCommand {
                stream: self.binding.stream.clone(),
                command: tasty_core::StructuralCommand::Close {
                    operation: tasty_core::OperationId(String::new()),
                    command_id: String::new(),
                    input,
                    target: self.target,
                    expected: self.expected.clone(),
                    undo: self.undo.clone(),
                    is_user_close: self.is_user_close,
                    remote_user_close: self.remote_user_close,
                },
            }],
            response: Some(ResponsePlan::Closed {
                success: self.response.clone(),
                not_closed: self.not_closed.clone(),
            }),
        }
    }
    pub fn weight(&self) -> usize {
        self.binding.stream.len()
            + serde_json::to_vec(&(&self.response, &self.not_closed, &self.expected, &self.undo))
                .map_or(usize::MAX, |bytes| bytes.len())
    }
}
impl JournalApplication {
    pub(super) fn resolve_close(&mut self, ticket: u64, session: &EngineSession) {
        let Some(pending) = self.commands.pending.get(&ticket) else {
            return;
        };
        match Request::resolve(&pending.request, session, pending.close_cause) {
            Ok(mut request) => {
                if let Reply::Remote(remote) = &pending.reply {
                    request.is_user_close = false;
                    request.remote_user_close = remote.user;
                    request.capture_undo = remote.user;
                } else if !matches!(&pending.reply, Reply::Intent { .. }) {
                    request.is_user_close = false;
                    request.capture_undo = false;
                } else if matches!(&pending.reply,Reply::Intent {origin,..} if origin.is_user()) {
                    request.is_user_close = true;
                }
                let work = match request.input(session, self.completion_views.get(&session.id)) {
                    Ok(work) => work,
                    Err(error) => {
                        self.reject_resolved_request(
                            ticket,
                            JsonRpcResponse::internal_error(serde_json::Value::Null, error),
                        );
                        return;
                    }
                };
                let pending = self
                    .commands
                    .pending
                    .get_mut(&ticket)
                    .expect("resolved request remains pending");
                if request.replacement {
                    let spec = super::create_spec::Spec {
                        destination: super::create_spec::Destination::Workspace {
                            name: Some("Workspace 1".into()),
                            subtitle: None,
                            description: None,
                            category: Some(0),
                        },
                        kind: "terminal".into(),
                        cwd: None,
                        params: serde_json::json!({}),
                    };
                    match create::Request::from_spec(spec, session) {
                        Ok(mut replacement) => {
                            replacement.activate = false;
                            pending.resource = Some(replacement);
                        }
                        Err(response) => {
                            self.reject_resolved_request(ticket, response);
                            return;
                        }
                    }
                }
                pending.request.params = serde_json::Value::Null;
                pending.queued = Some(work);
                pending.closing = Some(request);
                self.refresh_command_weight(ticket);
            }
            Err(response) => self.reject_resolved_request(ticket, response),
        }
        (self.wake)();
    }
}

pub(super) fn is_close_method(method: &str) -> bool {
    matches!(
        method,
        "terminal.kill"
            | "workspace.close"
            | "tab.close"
            | "pane.close"
            | "surface.close"
            | "surface.close_self"
            | "intent.close"
    )
}

impl JournalApplication {
    /// Surfaces an admitted close may still retire. A close is admitted before its targets are
    /// resolved, so unresolved closes are resolved here against the current structure; resolved
    /// ones keep their captured targets until the command finishes.
    pub(crate) fn closing_surfaces(
        &self,
        sessions: &[&EngineSession],
    ) -> std::collections::BTreeSet<u32> {
        let mut closing = std::collections::BTreeSet::new();
        for pending in self.commands.pending.values() {
            if let Some(request) = &pending.closing {
                closing.extend(request.expected.iter().map(|surface| surface.id));
            } else if is_close_method(&pending.request.method) {
                for session in sessions {
                    if let Ok(request) =
                        Request::resolve(&pending.request, session, pending.close_cause)
                    {
                        closing.extend(request.expected.iter().map(|surface| surface.id));
                    }
                }
            }
        }
        closing
    }
}

fn core_expected(session: &EngineSession, id: u32) -> Option<tasty_core::RetiredSurface> {
    let surface = session.core_state.find_surface_by_id(id)?;
    Some(tasty_core::RetiredSurface {
        id,
        kind: surface.kind.clone(),
        activation_generation: surface.activation_generation,
    })
}

/// Caller protection applies before choosing local execution or remote submission.
/// Public parameters never grant the holder/system cleanup exceptions.
pub(super) fn caller_refusal(
    request: &JsonRpcRequest,
    core: &crate::core::CoreState,
) -> Option<JsonRpcResponse> {
    let caller = crate::ipc::handler::caller_surface_id(&request.params)?;
    let id = |field| {
        request
            .params
            .get(field)
            .and_then(|value| value.as_u64())
            .and_then(|id| u32::try_from(id).ok())
    };
    let reason = match request.method.as_str() {
        "surface.close" if id("surface_id") == Some(caller) => {
            "Cannot close your own surface with 'close surface'. Use 'tasty close self' instead."
        }
        "tab.close"
            if core.find_tab_for_surface(caller) == id("tab_id") && id("tab_id").is_some() =>
        {
            "Cannot close a tab that contains your own surface. Use 'tasty close self' instead."
        }
        "pane.close"
            if core.find_pane_for_surface(caller) == id("pane_id") && id("pane_id").is_some() =>
        {
            "Cannot close a pane that contains your own surface. Close all other surfaces in the pane first, then use 'tasty close self'."
        }
        _ => return None,
    };
    Some(JsonRpcResponse::invalid_params(
        serde_json::Value::Null,
        reason,
    ))
}
