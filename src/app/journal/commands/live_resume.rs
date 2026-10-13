//! Bounded, process-local input continuation after committed lazy activation.
//! The journal only sees the activation request. Original input is never a durable replay payload.
use super::*;

pub(super) struct Resume {
    pub engine: EngineId,
    pub surface: u32,
    checked: crate::ipc::handler::OwnedCheckedRequest,
    reply: Return,
    pub generation: Option<tasty_terminal::ResourceGeneration>,
    fixed: Option<crate::app::terminal_input::PreparedTerminalInput>,
    bindings: crate::app::terminal_input::Bindings,
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
    fn into_reply_to(self) -> crate::ipc::handler::ReplyTo {
        match self {
            Self::Ipc(sender) => crate::ipc::handler::ReplyTo::Ipc(sender),
            Self::Plugin {
                plugin,
                call,
                binding,
            } => crate::ipc::handler::ReplyTo::Plugin {
                plugin_id: plugin,
                call_id: call,
                binding,
            },
        }
    }
    fn into_reply(self) -> Reply {
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
        } else if let Some(fixed) = self.fixed.as_ref().filter(|fixed| fixed.binds_children()) {
            let result = fixed.execute(services, engine, &self.bindings);
            crate::ipc::handler::terminal::input_response(&engine.as_ref(), id, result)
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
        } else if let Some(fixed) = &self.fixed {
            let result = fixed.execute(services, engine, &self.bindings);
            crate::ipc::handler::terminal::input_response(&engine.as_ref(), id, result)
        } else {
            match crate::ipc::handler::handle_checked_request(
                services,
                state,
                engine,
                &self.checked.borrow(),
            ) {
                crate::ipc::handler::Handled::Now(response) => response,
                crate::ipc::handler::Handled::Later(deferred) => {
                    services.defer_reply(deferred, self.reply.into_reply_to());
                    return;
                }
            }
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
        (self.reply.into_reply(), response)
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
/// 재개를 보류한 요청이 나중에 다시 실행될 때 필요한 값 — 해석해 둔 입력,
/// 대상별 리소스 세대, 남은 대상 큐.
type ResumeCarryOver = (
    Option<crate::app::terminal_input::PreparedTerminalInput>,
    crate::app::terminal_input::Bindings,
    std::collections::VecDeque<(u32, Option<u64>)>,
);

fn plan(
    request: &JsonRpcRequest,
    engine: &crate::runtime::engine_access::EngineRef<'_>,
) -> Option<ResumeCarryOver> {
    if !supported(&request.method) {
        return None;
    }
    let fixed = crate::ipc::handler::terminal::prepare_input(engine, request).ok()?;
    let targets = if let Some(fixed) = &fixed {
        fixed.targets()
    } else {
        let (surface, _) = crate::ipc::handler::surface::decode_input_header(
            &request.method,
            &request.params,
            &serde_json::Value::Null,
        )
        .ok()?;
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
    #[cfg(not(feature = "gui"))]
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
            caller_agent_id: None,
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
            caller_agent_id: None,
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
            caller_agent_id: None,
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
    #[cfg(not(feature = "gui"))]
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
            let Some(mut context) =
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

#[cfg(test)]
mod tests {
    //! 활성화가 끝난 뒤 보류 입력을 실행하는 단계의 현재 동작을 고정한다. 활성화 자체는 다루지 않는다.
    use super::*;
    use crate::runtime::child_terminal::{ChildEntry, ParentKey};
    use serde_json::json;
    use std::sync::mpsc;

    const PARENT: u32 = 1;
    const FIRST: u32 = 2;
    const SECOND: u32 = 3;

    fn session() -> (
        crate::state::RequestContext,
        crate::runtime::engine_session::EngineSession,
    ) {
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        let mut events = vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "workspace".into(),
                category: 0,
                index: 0,
                pane: 1,
            },
        ];
        for (index, id) in [PARENT, FIRST, SECOND].into_iter().enumerate() {
            events.push(E::TabCreated {
                id,
                pane: 1,
                index,
                name: "terminal".into(),
                surface: SurfaceSpec {
                    id,
                    kind: "terminal".into(),
                    data: None,
                },
            });
        }
        crate::state::tests::test_state_from_model(crate::state::tests::test_model(events))
    }

    fn child(surface: u32, index: u32) -> ChildEntry {
        ChildEntry {
            child_surface_id: surface,
            index,
            cwd: None,
            role: None,
            nickname: None,
        }
    }

    /// 두 child를 등록하고 각 터미널의 입력을 sink로 관측한다.
    fn children(
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    ) -> [mpsc::Receiver<Vec<u8>>; 2] {
        [FIRST, SECOND].map(|surface| {
            let index = engine
                .runtime
                .child_terminals
                .reserve_index(&ParentKey::Surface(PARENT))
                .expect("index");
            engine
                .runtime
                .child_terminals
                .register_child(PARENT, child(surface, index));
            let (tx, rx) = mpsc::channel();
            engine
                .runtime
                .terminals
                .get_mut(surface)
                .expect("terminal")
                .set_input_sink(tx);
            rx
        })
    }

    fn received(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<u8> {
        rx.try_iter().flatten().collect()
    }

    fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".into(),
            id: Some(json!(7)),
            method: method.into(),
            params,
            session_token: None,
        }
    }

    /// 대기 중이던 요청을 실제 지연 경로와 같은 값으로 감싼다. 대상 고정은 요청 시점에 한 번 한다.
    fn waiting(
        core: &mut crate::app::services::AppServices,
        state: &mut crate::state::RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        request: &JsonRpcRequest,
        surface: u32,
    ) -> (Resume, mpsc::Receiver<JsonRpcResponse>) {
        waiting_in(core, state, engine, request, request, surface)
    }

    /// `envelope`는 인증·응답 봉투로만 보관한다. 실행 값은 `request`에서 확정한다.
    fn waiting_in(
        core: &mut crate::app::services::AppServices,
        state: &mut crate::state::RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        request: &JsonRpcRequest,
        envelope: &JsonRpcRequest,
        surface: u32,
    ) -> (Resume, mpsc::Receiver<JsonRpcResponse>) {
        let caller = crate::ipc::caller::CallerContext::local();
        let checked = crate::ipc::handler::check_request(core, state, engine, envelope, &caller)
            .unwrap_or_else(|_| panic!("request passes the entry check"));
        let fixed = crate::ipc::handler::terminal::prepare_input(&engine.as_ref(), request)
            .unwrap_or_else(|_| panic!("targets resolve"));
        let targets = fixed
            .as_ref()
            .map_or_else(|| vec![surface], |fixed| fixed.targets());
        let bindings = targets
            .into_iter()
            .map(|surface| (surface, engine.runtime.terminals.generation(surface)))
            .collect();
        let (tx, rx) = mpsc::sync_channel(1);
        let resume = Resume {
            engine: EngineId::for_test(),
            surface,
            checked: checked.to_owned_without_key(),
            reply: Return::Ipc(tx),
            generation: engine.runtime.terminals.generation(surface),
            fixed,
            bindings,
            remaining: Default::default(),
        };
        (resume, rx)
    }

    fn activated() -> JsonRpcResponse {
        JsonRpcResponse::success(serde_json::Value::Null, json!({}))
    }

    fn run(
        core: &mut crate::app::services::AppServices,
        state: &mut crate::state::RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
        resume: Resume,
        reply: &mpsc::Receiver<JsonRpcResponse>,
    ) -> JsonRpcResponse {
        resume.execute(core, state, engine, activated(), None);
        reply.try_recv().expect("the waiting caller is answered")
    }

    #[test]
    fn a_resumed_broadcast_skips_a_released_child_and_counts_the_fixed_targets() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, second] = children(&mut engine);
        let request = request(
            "terminal.broadcast",
            json!({ "surface": PARENT, "text": "go\r" }),
        );
        let (resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
        assert!(
            engine
                .runtime
                .child_terminals
                .remove_child(&ParentKey::Surface(PARENT), 1)
                .is_some()
        );
        let response = run(&mut core, &mut state, &mut engine, resume, &reply);
        assert_eq!(response.id, json!(7));
        assert_eq!(
            response.result,
            Some(json!({ "sent_count": 2, "children": [FIRST, SECOND] }))
        );
        assert_eq!(received(&first), b"go\r");
        assert!(received(&second).is_empty());
    }

    #[test]
    fn a_resumed_respawn_refuses_a_replaced_child() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, second] = children(&mut engine);
        let request = request(
            "terminal.respawn",
            json!({ "surface": PARENT, "child": 0, "command": "make" }),
        );
        let (resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
        let parent = ParentKey::Surface(PARENT);
        assert!(
            engine
                .runtime
                .child_terminals
                .remove_child(&parent, 0)
                .is_some()
        );
        assert!(
            engine
                .runtime
                .child_terminals
                .remove_child(&parent, 1)
                .is_some()
        );
        engine
            .runtime
            .child_terminals
            .register_child(PARENT, child(SECOND, 0));
        let error = run(&mut core, &mut state, &mut engine, resume, &reply)
            .error
            .expect("refused");
        assert_eq!(error.code, -32602);
        assert_eq!(
            error.message,
            "child terminal target changed while awaiting activation"
        );
        assert!(received(&first).is_empty());
        assert!(received(&second).is_empty());
    }

    #[test]
    fn a_resumed_respawn_interrupts_then_runs_the_command_on_the_fixed_child() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, second] = children(&mut engine);
        let request = request(
            "terminal.respawn",
            json!({ "surface": PARENT, "child": 1, "command": "make", "role": "builder" }),
        );
        let (resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, SECOND);
        let response = run(&mut core, &mut state, &mut engine, resume, &reply);
        assert_eq!(
            response.result,
            Some(json!({ "child_surface_id": SECOND, "child_index": 1 }))
        );
        assert_eq!(received(&second), b"\x03make");
        assert!(received(&first).is_empty());
        let entry = engine
            .runtime
            .child_terminals
            .find_child(&ParentKey::Surface(PARENT), 1)
            .expect("child");
        assert_eq!(entry.role.as_deref(), Some("builder"));
    }

    #[test]
    fn a_resumed_tell_writes_the_body_and_queues_the_submit_once() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, _second] = children(&mut engine);
        engine.runtime.child_terminals.set_idle(FIRST, true);
        let request = request("terminal.tell", json!({ "surface": FIRST, "text": "hi" }));
        let (resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
        let response = run(&mut core, &mut state, &mut engine, resume, &reply);
        assert_eq!(
            response.result,
            Some(json!({ "sent": true, "surface_id": FIRST }))
        );
        assert_eq!(received(&first), b"hi");
        assert_eq!(engine.runtime.pending_submits.len(), 1);
        assert_eq!(engine.runtime.child_terminals.state_of(FIRST), "active");
    }

    #[test]
    fn a_resumed_tell_refuses_a_hard_occupied_or_replaced_terminal() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, _second] = children(&mut engine);
        let request = request("terminal.tell", json!({ "surface": FIRST, "text": "hi" }));

        let (resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
        engine
            .live
            .occupancy
            .acquire(FIRST, 1)
            .expect("hard occupy");
        let error = run(&mut core, &mut state, &mut engine, resume, &reply)
            .error
            .expect("refused");
        assert_eq!(
            error.message,
            "surface became hard-occupied before pending input could be applied"
        );
        engine.live.occupancy.release(FIRST, 1).expect("release");

        let (mut resume, reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
        resume.generation = None;
        let error = run(&mut core, &mut state, &mut engine, resume, &reply)
            .error
            .expect("refused");
        assert_eq!(
            error.message,
            "surface resource changed before pending input could be applied"
        );
        assert!(received(&first).is_empty());
        assert!(engine.runtime.pending_submits.is_empty());
    }

    /// 실행은 확정해 둔 본문·옵션만 쓴다. 보관한 원 요청의 params를 다시 읽지 않는다.
    #[test]
    fn a_resumed_input_runs_the_values_fixed_at_request_time() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let [first, second] = children(&mut engine);
        let cases = [
            (
                request(
                    "terminal.tell",
                    json!({ "surface": FIRST, "text": "fixed" }),
                ),
                request(
                    "terminal.tell",
                    json!({ "surface": FIRST, "text": "other" }),
                ),
                FIRST,
            ),
            (
                request(
                    "terminal.broadcast",
                    json!({ "surface": PARENT, "text": "fixed\r" }),
                ),
                request(
                    "terminal.broadcast",
                    json!({ "surface": PARENT, "text": "other\r" }),
                ),
                FIRST,
            ),
            (
                request(
                    "terminal.respawn",
                    json!({ "surface": PARENT, "child": 1, "command": "fixed", "role": "kept" }),
                ),
                request(
                    "terminal.respawn",
                    json!({ "surface": PARENT, "child": 1, "command": "other", "role": "lost" }),
                ),
                SECOND,
            ),
        ];
        for (fixed, envelope, surface) in &cases {
            let (resume, reply) = waiting_in(
                &mut core,
                &mut state,
                &mut engine,
                fixed,
                envelope,
                *surface,
            );
            let response = run(&mut core, &mut state, &mut engine, resume, &reply);
            assert!(response.error.is_none(), "{response:?}");
        }
        assert_eq!(received(&first), b"fixedfixed\r");
        assert_eq!(received(&second), b"fixed\r\x03fixed");
        let entry = engine
            .runtime
            .child_terminals
            .find_child(&ParentKey::Surface(PARENT), 1)
            .expect("child");
        assert_eq!(entry.role.as_deref(), Some("kept"));
    }

    /// 확정한 본문 사본은 원 요청과 따로 대기 예산에 들어간다.
    #[test]
    fn the_wait_weight_counts_the_fixed_body_copy() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (mut state, mut session) = session();
        let mut engine = session.borrow_mut();
        let _children = children(&mut engine);
        let body = "x".repeat(4096);
        for request in [
            request("terminal.tell", json!({ "surface": FIRST, "text": body })),
            request(
                "terminal.broadcast",
                json!({ "surface": PARENT, "text": body }),
            ),
            request(
                "terminal.respawn",
                json!({ "surface": PARENT, "child": 0, "command": body }),
            ),
        ] {
            let (resume, _reply) = waiting(&mut core, &mut state, &mut engine, &request, FIRST);
            let fixed = resume.fixed.as_ref().expect("fixed input").weight();
            assert!(fixed >= body.len(), "{}: {fixed}", request.method);
            assert_eq!(
                resume.weight(),
                resume.checked.weight()
                    + std::mem::size_of::<Resume>()
                    + fixed
                    + resume.bindings.len() * 48
                    + resume.remaining.capacity() * 24
            );
            assert!(resume.weight() >= 2 * body.len());
        }
    }
}
