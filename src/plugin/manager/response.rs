//! Plugin → host 응답 처리. `pump` 에서 매 tick 호출되는 `drain_plugin_responses`
//! 와 그 dispatch 로 호출되는 pre/post hook 응답 처리, `sweep_expired_hooks` 까지 포함.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::ipc::server::send_response;
use crate::plugin::manifest::{EventHookDecl, HookMode, IpcHookDecl};
use crate::plugin::protocol::{self, IpcCallResult, PluginResponse, SurfaceResult};

use super::{
    FinalCaller, HookOutcome, PendingRequestKind, PluginManager, TargetOutcome, parse_hook_result,
};

impl PluginManager {
    pub(super) fn drain_plugin_responses(&mut self) {
        let plugin_ids: Vec<String> = self.processes.keys().cloned().collect();
        for plugin_id in plugin_ids {
            // Drain all responses without holding a borrow on `self.processes`.
            let mut responses: Vec<PluginResponse> = Vec::new();
            if let Some(proc) = self.processes.get(&plugin_id) {
                while let Ok(resp) = proc.resp_rx.try_recv() {
                    responses.push(resp);
                }
            }
            for resp in responses {
                self.handle_plugin_response(&plugin_id, resp);
            }
        }
    }

    pub(super) fn handle_plugin_response(&mut self, plugin_id: &str, resp: PluginResponse) {
        let kind = self.pending_requests.remove(&resp.id);
        if let Some(err) = &resp.error {
            tracing::warn!(
                "plugin '{plugin_id}' response error (id={}): {err}",
                resp.id
            );
        }
        let kind = match kind {
            Some(k) => k,
            None => return,
        };
        match kind {
            PendingRequestKind::SurfaceCreate { surface_id }
            | PendingRequestKind::SurfaceEvent { surface_id }
            | PendingRequestKind::SurfaceRestore { surface_id }
            | PendingRequestKind::CommandInvoke { surface_id } => {
                let result_value = match resp.result {
                    Some(v) => v,
                    None => return,
                };
                let parsed: SurfaceResult = match serde_json::from_value(result_value) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("plugin '{plugin_id}' surface response decode error: {e}");
                        return;
                    }
                };
                if let Some(entry) = self.surfaces.get(&surface_id) {
                    if let Some(tree) = parsed.tree {
                        if let Ok(mut slot) = entry.handles.tree.lock() {
                            *slot = Some(tree);
                        }
                    }
                    if let Some(name) = parsed.display_name {
                        if let Ok(mut slot) = entry.handles.display_name.lock() {
                            *slot = name;
                        }
                    }
                }
            }
            PendingRequestKind::Other => {}
            PendingRequestKind::PopupOpen { instance_id } => {
                let result_value = match resp.result {
                    Some(v) => v,
                    None => return,
                };
                let parsed: protocol::PopupOpenResult = match serde_json::from_value(result_value) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            "plugin '{plugin_id}' popup.open response decode error: {e}"
                        );
                        return;
                    }
                };
                if let Some(inst) = self.popup_instances.get_mut(&instance_id) {
                    inst.tree = parsed.tree;
                }
            }
            PendingRequestKind::PopupEvent { instance_id } => {
                let result_value = match resp.result {
                    Some(v) => v,
                    None => return,
                };
                let parsed: protocol::PopupEventResult = match serde_json::from_value(result_value)
                {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(
                            "plugin '{plugin_id}' popup.event response decode error: {e}"
                        );
                        return;
                    }
                };
                if let Some(inst) = self.popup_instances.get_mut(&instance_id) {
                    if parsed.tree.is_some() {
                        inst.tree = parsed.tree;
                    }
                }
                if parsed.close {
                    self.close_popup_instance(
                        instance_id,
                        tasty_plugin_protocol::PopupCloseReason::PluginRequest,
                    );
                }
            }
            PendingRequestKind::NamespaceInvoke {
                plugin_id: _,
                response_tx,
                original_id,
            } => {
                let response = if let Some(err) = resp.error {
                    let code = resp.error_code.unwrap_or(-32000);
                    JsonRpcResponse::error(original_id, code, &err)
                } else {
                    JsonRpcResponse::success(
                        original_id,
                        resp.result.unwrap_or(serde_json::Value::Null),
                    )
                };
                send_response(&response_tx, response);
            }
            PendingRequestKind::PluginToPluginNamespace {
                plugin_id: _,
                caller_plugin_id,
                call_id,
            } => {
                // plugin caller에는 ipc.result로 회신. error_code는 plugin 측이
                // 받은 표준 form 그대로(메시지에 합쳐서 전달).
                self.send_ipc_result(&caller_plugin_id, call_id, resp.result, resp.error);
            }
            PendingRequestKind::ExtensionPreIpcHook {
                target_plugin_id,
                extension_plugin_id,
                method,
                params,
                pre_hook_mode,
                final_caller,
                post_hook,
                deadline: _,
            } => {
                self.handle_pre_ipc_hook_response(
                    extension_plugin_id,
                    target_plugin_id,
                    method,
                    params,
                    pre_hook_mode,
                    final_caller,
                    post_hook,
                    resp,
                );
            }
            PendingRequestKind::NamespaceInvokeWithPostHook {
                target_plugin_id: _,
                method,
                extension_plugin_id,
                post_hook_decl,
                final_caller,
            } => {
                self.handle_target_response_with_post_hook(
                    extension_plugin_id,
                    method,
                    post_hook_decl,
                    final_caller,
                    resp,
                );
            }
            PendingRequestKind::ExtensionPostIpcHook {
                extension_plugin_id,
                method,
                post_hook_mode,
                target_outcome,
                final_caller,
                deadline: _,
            } => {
                if resp.error.is_some() {
                    self.record_hook_failure(&extension_plugin_id, &method);
                } else {
                    self.record_hook_success(&extension_plugin_id, &method);
                }
                self.handle_post_ipc_hook_response(
                    post_hook_mode,
                    target_outcome,
                    final_caller,
                    resp,
                );
            }
            PendingRequestKind::ExtensionPreEventHook {
                publisher_plugin_id,
                extension_plugin_id,
                envelope,
                pre_hook_mode,
                post_hook,
                deadline: _,
            } => {
                self.handle_pre_event_hook_response(
                    publisher_plugin_id,
                    extension_plugin_id,
                    envelope,
                    pre_hook_mode,
                    post_hook,
                    resp,
                );
            }
            PendingRequestKind::ExtensionPostEventHook {
                extension_plugin_id,
                event_key,
                deadline: _,
            } => {
                if resp.error.is_some() {
                    self.record_hook_failure(&extension_plugin_id, &event_key);
                } else {
                    self.record_hook_success(&extension_plugin_id, &event_key);
                }
                // post-event는 결과를 무시 (이미 fan-out 완료).
            }
            #[cfg(debug_assertions)]
            PendingRequestKind::DebugExtensionInvokeHook {
                response_tx,
                original_id,
            } => {
                let response = if let Some(err) = resp.error {
                    let code = resp.error_code.unwrap_or(-32000);
                    JsonRpcResponse::error(original_id, code, &err)
                } else {
                    JsonRpcResponse::success(
                        original_id,
                        resp.result.unwrap_or(serde_json::Value::Null),
                    )
                };
                send_response(&response_tx, response);
            }
        }
    }

    /// debug 빌드 한정 — extension에 직접 hook을 송신하고 응답을 caller에 회신.
    /// 테스트 도구. 정상 트래픽(IPC/event)이 아니라 디버그 path.
    #[cfg(debug_assertions)]
    pub fn debug_invoke_extension_hook(
        &mut self,
        extension_id: &str,
        kind: tasty_plugin_protocol::ExtensionHookKind,
        phase: tasty_plugin_protocol::ExtensionHookPhase,
        mode: HookMode,
        target: &str,
        payload: serde_json::Value,
        original_id: serde_json::Value,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
    ) {
        if !self.processes.contains_key(extension_id) {
            send_response(
                &response_tx,
                JsonRpcResponse::error(
                    original_id,
                    -32002,
                    &format!("extension '{extension_id}' is not running"),
                ),
            );
            return;
        }
        match self.send_extension_invoke_hook(extension_id, kind, phase, mode, target, payload) {
            Ok(req_id) => {
                self.pending_requests.insert(
                    req_id,
                    PendingRequestKind::DebugExtensionInvokeHook {
                        response_tx,
                        original_id,
                    },
                );
            }
            Err(msg) => {
                send_response(
                    &response_tx,
                    JsonRpcResponse::error(original_id, -32003, &msg),
                );
            }
        }
    }

    /// pre-event-hook 응답을 처리. mode에 따라 transform(payload 교체) / filter(차단) / observe.
    pub(super) fn handle_pre_event_hook_response(
        &mut self,
        publisher_plugin_id: String,
        extension_plugin_id: String,
        mut envelope: tasty_plugin_protocol::EventEnvelope,
        mode: HookMode,
        post_hook: Option<EventHookDecl>,
        resp: PluginResponse,
    ) {
        if resp.error.is_some() {
            self.record_hook_failure(&extension_plugin_id, &envelope.key);
        } else {
            self.record_hook_success(&extension_plugin_id, &envelope.key);
        }
        let outcome = parse_hook_result(&resp);

        if matches!(mode, HookMode::Filter) && matches!(outcome, HookOutcome::Block) {
            tracing::info!(
                "extension '{extension_plugin_id}' filtered event '{}' from '{publisher_plugin_id}'",
                envelope.key
            );
            return;
        }
        if let (HookMode::Transform, HookOutcome::Modified(new_payload)) = (mode, outcome) {
            envelope.payload = new_payload;
        }
        self.fan_out_then_post(
            &publisher_plugin_id,
            envelope,
            extension_plugin_id,
            post_hook,
        );
    }

    /// pre-hook 응답을 처리. mode에 따라 transform/filter/observe 적용 후 target에 forward.
    pub(super) fn handle_pre_ipc_hook_response(
        &mut self,
        extension_plugin_id: String,
        target_plugin_id: String,
        method: String,
        original_params: serde_json::Value,
        mode: HookMode,
        final_caller: FinalCaller,
        post_hook: Option<IpcHookDecl>,
        resp: PluginResponse,
    ) {
        if resp.error.is_some() {
            self.record_hook_failure(&extension_plugin_id, &method);
        } else {
            self.record_hook_success(&extension_plugin_id, &method);
        }
        // hook 응답에서 outcome 추출. 에러/누락은 fail-open(original payload로 진행).
        let outcome = parse_hook_result(&resp);
        let post_pair = post_hook.map(|p| (extension_plugin_id.clone(), p));

        // payload는 hook 호출 시 {method, params, caller}로 래핑했으므로,
        // transform이면 wrapper의 params 필드를 다시 꺼내 사용한다.
        let final_params = match (mode, &outcome) {
            (HookMode::Transform, HookOutcome::Modified(v)) => {
                v.get("params").cloned().unwrap_or(original_params)
            }
            _ => original_params,
        };

        // filter mode에서 block이면 호출 자체 차단.
        if matches!(mode, HookMode::Filter) && matches!(outcome, HookOutcome::Block) {
            let msg = format!("extension '{extension_plugin_id}' filtered out method '{method}'");
            // post-hook이 있어도 filter block이면 target 호출이 일어나지 않으므로 post도 skip.
            self.send_final_error(final_caller, -32001, msg);
            return;
        }

        // observe/transform/filter-pass → target에 정상 invoke.
        self.dispatch_target_invoke(
            target_plugin_id,
            method,
            final_params,
            None, // caller_plugin_id는 검증을 이미 통과했으므로 target 측 caller 표시는 필요시만.
            final_caller,
            post_pair,
        );
    }

    /// target plugin 응답을 받았을 때 post-hook으로 chain.
    pub(super) fn handle_target_response_with_post_hook(
        &mut self,
        extension_plugin_id: String,
        method: String,
        post_hook_decl: IpcHookDecl,
        final_caller: FinalCaller,
        resp: PluginResponse,
    ) {
        let target_outcome = if let Some(err) = resp.error.clone() {
            TargetOutcome::Err {
                message: err,
                code: resp.error_code.unwrap_or(-32000),
            }
        } else {
            TargetOutcome::Ok(resp.result.clone().unwrap_or(serde_json::Value::Null))
        };

        // post-hook payload: target의 result (에러면 null 전달).
        let payload = match &target_outcome {
            TargetOutcome::Ok(v) => v.clone(),
            TargetOutcome::Err { .. } => serde_json::Value::Null,
        };

        if self.is_hook_in_backoff(&extension_plugin_id, &method) {
            self.finalize_target_outcome(final_caller, target_outcome);
            return;
        }
        let deadline = Instant::now() + Duration::from_millis(post_hook_decl.timeout_ms as u64);
        match self.send_extension_invoke_hook(
            &extension_plugin_id,
            tasty_plugin_protocol::ExtensionHookKind::Ipc,
            tasty_plugin_protocol::ExtensionHookPhase::Post,
            post_hook_decl.mode,
            &method,
            payload,
        ) {
            Ok(req_id) => {
                self.pending_requests.insert(
                    req_id,
                    PendingRequestKind::ExtensionPostIpcHook {
                        extension_plugin_id,
                        method,
                        post_hook_mode: post_hook_decl.mode,
                        target_outcome,
                        final_caller,
                        deadline,
                    },
                );
            }
            Err(msg) => {
                tracing::warn!("post-hook dispatch failed: {msg}; bypassing");
                self.finalize_target_outcome(final_caller, target_outcome);
            }
        }
    }

    /// 타임아웃된 pre/post hook pending을 sweep해서 fail-open 처리.
    pub(super) fn sweep_expired_hooks(&mut self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .pending_requests
            .iter()
            .filter_map(|(id, kind)| match kind {
                PendingRequestKind::ExtensionPreIpcHook { deadline, .. }
                | PendingRequestKind::ExtensionPostIpcHook { deadline, .. }
                | PendingRequestKind::ExtensionPreEventHook { deadline, .. }
                | PendingRequestKind::ExtensionPostEventHook { deadline, .. } => {
                    if now >= *deadline { Some(*id) } else { None }
                }
                _ => None,
            })
            .collect();
        for id in expired {
            let kind = match self.pending_requests.remove(&id) {
                Some(k) => k,
                None => continue,
            };
            match kind {
                PendingRequestKind::ExtensionPreIpcHook {
                    target_plugin_id,
                    extension_plugin_id,
                    method,
                    params,
                    pre_hook_mode: _,
                    final_caller,
                    post_hook,
                    deadline: _,
                } => {
                    tracing::warn!(
                        "pre-hook timeout: ext='{extension_plugin_id}' method='{method}' — fail-open"
                    );
                    self.record_hook_failure(&extension_plugin_id, &method);
                    let post_pair = post_hook.map(|p| (extension_plugin_id.clone(), p));
                    self.dispatch_target_invoke(
                        target_plugin_id,
                        method,
                        params,
                        None,
                        final_caller,
                        post_pair,
                    );
                }
                PendingRequestKind::ExtensionPostIpcHook {
                    extension_plugin_id,
                    method,
                    post_hook_mode: _,
                    target_outcome,
                    final_caller,
                    deadline: _,
                } => {
                    tracing::warn!(
                        "post-hook timeout: ext='{extension_plugin_id}' method='{method}' — fail-open"
                    );
                    self.record_hook_failure(&extension_plugin_id, &method);
                    self.finalize_target_outcome(final_caller, target_outcome);
                }
                PendingRequestKind::ExtensionPreEventHook {
                    publisher_plugin_id,
                    extension_plugin_id,
                    envelope,
                    pre_hook_mode: _,
                    post_hook,
                    deadline: _,
                } => {
                    tracing::warn!(
                        "pre-event-hook timeout: ext='{extension_plugin_id}' event='{}' — fail-open",
                        envelope.key
                    );
                    self.record_hook_failure(&extension_plugin_id, &envelope.key);
                    self.fan_out_then_post(
                        &publisher_plugin_id,
                        envelope,
                        extension_plugin_id,
                        post_hook,
                    );
                }
                PendingRequestKind::ExtensionPostEventHook {
                    extension_plugin_id,
                    event_key,
                    deadline: _,
                } => {
                    tracing::warn!(
                        "post-event-hook timeout: ext='{extension_plugin_id}' event='{event_key}'"
                    );
                    self.record_hook_failure(&extension_plugin_id, &event_key);
                }
                _ => {}
            }
        }
    }

    /// post-hook 응답을 처리. transform이면 result 교체, 그 외는 원 target 응답 사용.
    pub(super) fn handle_post_ipc_hook_response(
        &mut self,
        mode: HookMode,
        target_outcome: TargetOutcome,
        final_caller: FinalCaller,
        resp: PluginResponse,
    ) {
        let outcome = parse_hook_result(&resp);
        let final_outcome = match (mode, outcome, target_outcome) {
            (HookMode::Transform, HookOutcome::Modified(v), _) => TargetOutcome::Ok(v),
            (_, _, original) => original,
        };
        self.finalize_target_outcome(final_caller, final_outcome);
    }

    pub(super) fn finalize_target_outcome(
        &mut self,
        final_caller: FinalCaller,
        outcome: TargetOutcome,
    ) {
        match outcome {
            TargetOutcome::Ok(v) => self.send_final_success(final_caller, v),
            TargetOutcome::Err { message, code } => {
                self.send_final_error(final_caller, code, message);
            }
        }
    }
}
