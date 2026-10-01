//! Bounded, process-local input continuation after committed lazy activation.
//! The journal only sees the activation request. Original input is never a durable replay payload.
use super::*;

pub(super) struct Resume {
    pub engine: EngineId,
    pub surface: u32,
    checked: crate::ipc::handler::OwnedCheckedRequest,
    reply: Return,
    pub generation: Option<tasty_terminal::ResourceGeneration>,
    fixed: Option<crate::ipc::handler::terminal::FixedInput>,
    bindings: std::collections::BTreeMap<u32, Option<tasty_terminal::ResourceGeneration>>,
    remaining: std::collections::VecDeque<(u32, Option<u64>)>,
}
enum Return {
    Ipc(SyncSender<JsonRpcResponse>),
    Plugin {
        plugin: String,
        call: u64,
        binding: std::sync::Weak<()>,
    },
}
impl Return {
    fn current(&self, manager: Option<&crate::plugin::PluginManager>) -> bool {
        match self {
            Self::Ipc(_) => true,
            Self::Plugin {
                plugin, binding, ..
            } => manager
                .and_then(|manager| manager.processes.get(plugin))
                .is_some_and(|process| process.reply_binding().ptr_eq(binding)),
        }
    }
    fn answer(self, response: JsonRpcResponse, manager: Option<&mut crate::plugin::PluginManager>) {
        match self {
            Self::Ipc(sender) => crate::ipc::server::send_response(&sender, response),
            Self::Plugin {
                plugin,
                call,
                binding,
            } => {
                if let Some(manager) = manager {
                    manager.send_bound_ipc_result(&plugin, &binding, call, response);
                }
            }
        }
    }
    fn as_reply(self) -> Reply {
        match self {
            Self::Ipc(sender) => Reply::Ipc(sender),
            Self::Plugin {
                plugin,
                call,
                binding,
            } => Reply::Plugin {
                plugin_id: plugin,
                call_id: call,
                binding,
            },
        }
    }
}
impl Resume {
    pub fn weight(&self) -> usize {
        self.checked.weight()
            + std::mem::size_of::<Self>()
            + self.fixed.as_ref().map_or(0, |fixed| fixed.weight())
            + self.bindings.len() * 48
            + self.remaining.capacity() * 24
    }
    pub fn execute(
        self,
        services: &mut crate::app::services::AppServices,
        state: &mut crate::state::RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        activation: JsonRpcResponse,
        manager: Option<&mut crate::plugin::PluginManager>,
    ) {
        let id = self
            .checked
            .borrow()
            .request()
            .id
            .clone()
            .unwrap_or_default();
        let response = if !self.reply.current(manager.as_deref()) {
            JsonRpcResponse::invalid_params(id, "input caller process was replaced while waiting")
        } else if let Some(error) = activation.error {
            JsonRpcResponse::error(id, error.code, error.message)
        } else if let Some(fixed) = &self.fixed {
            fixed.execute(
                services,
                engine,
                self.checked.borrow().request(),
                &self.bindings,
            )
        } else if engine.live.occupancy.is_hard_occupied(self.surface) {
            JsonRpcResponse::invalid_params(
                id,
                "surface became hard-occupied before pending input could be applied",
            )
        } else if self.generation.is_none_or(|generation| {
            !engine
                .runtime
                .terminals
                .matches_generation(self.surface, generation)
        }) {
            JsonRpcResponse::invalid_params(
                id,
                "surface resource changed before pending input could be applied",
            )
        } else {
            crate::ipc::handler::handle_checked_request(
                services,
                state,
                engine,
                &self.checked.borrow(),
            )
        };
        self.reply.answer(response, manager);
    }
    pub(super) fn advance(&mut self, response: &mut JsonRpcResponse) -> Option<(u32, Option<u64>)> {
        self.bindings.insert(
            self.surface,
            if response.error.is_none() {
                self.generation
            } else {
                None
            },
        );
        if response.error.is_some()
            && !self
                .fixed
                .as_ref()
                .is_some_and(|fixed| fixed.permits_partial())
        {
            return None;
        }
        let next = self.remaining.pop_front();
        if let Some((surface, _)) = next {
            self.surface = surface;
            self.generation = None;
        }
        if self
            .fixed
            .as_ref()
            .is_some_and(|fixed| fixed.permits_partial())
        {
            *response = JsonRpcResponse::success(
                serde_json::Value::Null,
                serde_json::json!({"prepared":true}),
            );
        }
        next
    }
    pub fn error_reply(self, mut response: JsonRpcResponse) -> (Reply, JsonRpcResponse) {
        response.id = self
            .checked
            .borrow()
            .request()
            .id
            .clone()
            .unwrap_or_default();
        (self.reply.as_reply(), response)
    }
    pub fn reject(self, reason: &str, manager: Option<&mut crate::plugin::PluginManager>) {
        let id = self
            .checked
            .borrow()
            .request()
            .id
            .clone()
            .unwrap_or_default();
        self.reply
            .answer(JsonRpcResponse::internal_error(id, reason), manager);
    }
}

fn supported(method: &str) -> bool {
    matches!(
        method,
        "surface.send"
            | "surface.send_key"
            | "surface.send_combo"
            | "surface.send_wait_idle"
            | "surface.send_to"
            | "terminal.tell"
            | "terminal.broadcast"
            | "terminal.respawn"
    )
}
fn deferred(surface: u32, engine: &crate::runtime::engine_access::EngineRef<'_>) -> bool {
    !engine.live.occupancy.is_hard_occupied(surface)
        && !engine.runtime.terminals.contains(surface)
        && engine
            .runtime
            .surfaces
            .get(&surface)
            .and_then(|surface| {
                surface
                    .as_any()
                    .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
            })
            .is_some_and(|placeholder| placeholder.kind == "terminal")
}
fn plan(
    request: &JsonRpcRequest,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
) -> Option<(
    Option<crate::ipc::handler::terminal::FixedInput>,
    std::collections::BTreeMap<u32, Option<tasty_terminal::ResourceGeneration>>,
    std::collections::VecDeque<(u32, Option<u64>)>,
)> {
    if !supported(&request.method) {
        return None;
    }
    let fixed = crate::ipc::handler::terminal::decode_fixed_input(engine, request).ok()?;
    let targets = if let Some(fixed) = &fixed {
        fixed.targets()
    } else {
        let (surface, _) = if request.method == "terminal.tell" {
            crate::ipc::handler::terminal::decode_tell(&request.params, &serde_json::Value::Null)
                .ok()?
        } else {
            crate::ipc::handler::surface::decode_input_header(
                &request.method,
                &request.params,
                &serde_json::Value::Null,
            )
            .ok()?
        };
        if request.method == "surface.send_wait_idle" && engine.is_typing(surface) {
            return None;
        }
        vec![surface]
    };
    let remaining = targets
        .iter()
        .copied()
        .filter(|surface| deferred(*surface, engine))
        .map(|surface| {
            (
                surface,
                engine
                    .core
                    .find_surface_by_id(surface)
                    .and_then(|surface| surface.activation_generation),
            )
        })
        .collect::<std::collections::VecDeque<_>>();
    if remaining.is_empty() {
        return None;
    }
    let bindings = targets
        .into_iter()
        .map(|surface| (surface, engine.runtime.terminals.generation(surface)))
        .collect();
    Some((fixed, bindings, remaining))
}
impl JournalApplication {
    /// Original live-input key arbitration precedes resolving an implicit resource or starting it.
    pub(crate) fn defer_live_ipc(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
        id: EngineId,
        engine: &crate::runtime::engine_access::EngineRef<'_>,
    ) -> bool {
        if !supported(&command.request.method) {
            return false;
        }
        if let Some(handled) = crate::ipc::handler::idempotency::run_app_layer(
            checked.caller(),
            command,
            true,
            |handled| *handled,
            |relayed| self.defer_live_unkeyed(relayed, checked, id, engine),
        ) {
            return handled;
        }
        self.defer_live_unkeyed(command, checked, id, engine)
    }
    pub(crate) fn defer_live_unkeyed(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
        id: EngineId,
        engine: &crate::runtime::engine_access::EngineRef<'_>,
    ) -> bool {
        let Some((fixed, bindings, mut remaining)) = plan(&command.request, engine) else {
            return false;
        };
        let Some((surface, activation)) = remaining.pop_front() else {
            return false;
        };
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.wake".into(),
            params: serde_json::json!({"surface_id":surface,"activation":activation}),
            id: None,
            idempotency_key: None,
            session_token: None,
            response_timeout_ms: None,
        };
        let resume = Resume {
            engine: id,
            surface,
            checked: checked.to_owned_without_key(),
            reply: Return::Ipc(command.response_tx.clone()),
            generation: None,
            fixed,
            bindings,
            remaining,
        };
        let receipt = self
            .creations
            .iter()
            .filter(|((engine, _), _)| *engine == id)
            .find_map(|(_, creation)| creation.join_restore(surface, activation));
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Resume(resume),
            crate::ipc::handler::idempotency::caller_scope(checked.caller()),
            "live-input-activation",
        );
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.activation_wait = receipt;
        }
        true
    }
    pub(crate) fn defer_plugin_input(
        &mut self,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
        id: EngineId,
        engine: &crate::runtime::engine_access::EngineRef<'_>,
        call: &tasty_host_plugin::manager::PendingPluginCall,
        manager: Option<&crate::plugin::PluginManager>,
    ) -> bool {
        let Some((fixed, bindings, mut remaining)) = plan(checked.request(), engine) else {
            return false;
        };
        let Some((surface, activation)) = remaining.pop_front() else {
            return false;
        };
        if !manager.is_some_and(|manager| manager.plugin_call_is_current(call)) {
            return true;
        }
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.wake".into(),
            params: serde_json::json!({"surface_id":surface,"activation":activation}),
            id: None,
            idempotency_key: None,
            session_token: None,
            response_timeout_ms: None,
        };
        let resume = Resume {
            engine: id,
            surface,
            checked: checked.to_owned_without_key(),
            reply: Return::Plugin {
                plugin: call.plugin_id.clone(),
                call: call.call_id,
                binding: call.binding.clone(),
            },
            generation: None,
            fixed,
            bindings,
            remaining,
        };
        let receipt = self
            .creations
            .iter()
            .filter(|((engine, _), _)| *engine == id)
            .find_map(|(_, creation)| creation.join_restore(surface, activation));
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Resume(resume),
            crate::ipc::handler::idempotency::caller_scope(checked.caller()),
            "plugin-input-activation",
        );
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.activation_wait = receipt;
        }
        true
    }
    pub(super) fn continue_live_resume(
        &mut self,
        resume: Resume,
        surface: u32,
        activation: Option<u64>,
    ) {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "intent.wake".into(),
            params: serde_json::json!({"surface_id":surface,"activation":activation}),
            id: None,
            idempotency_key: None,
            session_token: None,
            response_timeout_ms: None,
        };
        let scope =
            crate::ipc::handler::idempotency::caller_scope(resume.checked.borrow().caller());
        let receipt = self
            .creations
            .iter()
            .filter(|((engine, _), _)| *engine == resume.engine)
            .find_map(|(_, creation)| creation.join_restore(surface, activation));
        let ticket = self.next_ticket;
        self.admit_request(
            &request,
            Reply::Resume(resume),
            scope,
            "live-input-activation",
        );
        if let Some(pending) = self.commands.pending.get_mut(&ticket) {
            pending.activation_wait = receipt;
        }
    }
    pub(crate) fn finish_headless_live_inputs(
        &mut self,
        id: EngineId,
        services: &mut crate::app::services::AppServices,
        state: &mut crate::state::RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        mut manager: Option<&mut crate::plugin::PluginManager>,
    ) {
        for (resume, response) in std::mem::take(&mut self.commands.completed_live) {
            if resume.engine == id {
                resume.execute(services, state, engine, response, manager.as_deref_mut());
            } else {
                resume.reject("pending input engine disappeared", manager.as_deref_mut());
            }
        }
    }
}
#[cfg(feature = "gui")]
impl crate::app::App {
    pub(crate) fn defer_live_input(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
    ) -> bool {
        if !supported(&command.request.method) {
            return false;
        }
        if let Some(handled) = crate::ipc::handler::idempotency::run_app_layer(
            checked.caller(),
            command,
            true,
            |handled| *handled,
            |relayed| self.defer_live_input_miss(relayed, checked),
        ) {
            return handled;
        }
        self.defer_live_input_miss(command, checked)
    }
    fn defer_live_input_miss(
        &mut self,
        command: &crate::ipc::server::IpcCommand,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
    ) -> bool {
        let resource = crate::core::request_target::request_resource_id(
            &command.request.method,
            &command.request.params,
        );
        let window = self
            .find_request_owner(&command.request.method, &command.request.params)
            .ok()
            .flatten()
            .or_else(|| {
                resource
                    .is_none()
                    .then_some(self.view.focused_view_id)
                    .flatten()
            });
        let owner = window
            .and_then(|window| self.engines.of_window(window))
            .or_else(|| {
                self.engines()
                    .parked_with_ids()
                    .find(|(_, engine)| {
                        resource.is_none_or(|resource| {
                            crate::core::request_target::engine_has_resource(engine, resource)
                        })
                    })
                    .map(|(id, _)| id)
            });
        let Some(session) = self.engines.all_sessions().find(|session| {
            owner.map_or_else(
                || {
                    resource.is_some_and(|resource| {
                        crate::core::request_target::engine_has_resource(
                            &session.as_ref(),
                            resource,
                        )
                    })
                },
                |id| session.id == id,
            )
        }) else {
            return false;
        };
        self.journal
            .defer_live_unkeyed(command, checked, session.id, &session.as_ref())
    }
    pub(crate) fn defer_live_plugin(
        &mut self,
        checked: &crate::ipc::handler::CheckedRequest<'_>,
        call: &tasty_host_plugin::manager::PendingPluginCall,
    ) -> bool {
        let request = checked.request();
        let resource =
            crate::core::request_target::request_resource_id(&request.method, &request.params);
        let window = self
            .find_request_owner(&request.method, &request.params)
            .ok()
            .flatten()
            .or_else(|| {
                resource
                    .is_none()
                    .then_some(self.view.focused_view_id)
                    .flatten()
            });
        let owner = window
            .and_then(|window| self.engines.of_window(window))
            .or_else(|| {
                self.engines()
                    .parked_with_ids()
                    .find(|(_, engine)| {
                        resource.is_none_or(|resource| {
                            crate::core::request_target::engine_has_resource(engine, resource)
                        })
                    })
                    .map(|(id, _)| id)
            });
        let Some(session) = self.engines.all_sessions().find(|session| {
            owner.map_or_else(
                || {
                    resource.is_some_and(|resource| {
                        crate::core::request_target::engine_has_resource(
                            &session.as_ref(),
                            resource,
                        )
                    })
                },
                |id| session.id == id,
            )
        }) else {
            return false;
        };
        self.journal.defer_plugin_input(
            checked,
            session.id,
            &session.as_ref(),
            call,
            self.plugin_manager.as_ref(),
        )
    }
    pub(crate) fn finish_live_inputs(&mut self) {
        for (resume, response) in std::mem::take(&mut self.journal.commands.completed_live) {
            let services = &mut self.services;
            let manager = self.plugin_manager.as_mut();
            let Some(context) =
                crate::app::window_access::engines_mut!(self).resolve(resume.engine)
            else {
                resume.reject("pending input engine disappeared", manager);
                continue;
            };
            resume.execute(
                services,
                context.state,
                &mut context.engine,
                response,
                manager,
            );
        }
    }
}
