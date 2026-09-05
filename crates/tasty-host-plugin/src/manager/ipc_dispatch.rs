//! IPC dispatch: 외부 client → plugin namespace forward, plugin → plugin namespace
//! forward, extension hook 진입 routing, hook backoff/실패 카운터, 최종 응답
//! 송신 helper.

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::json;

use crate::protocol::{self, IpcCallResult, PluginRequest};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_ipc::server::send_response;
use tasty_plugin_manifest::{HookMode, IpcHookDecl, Permission};

use super::{
    FinalCaller, HOOK_FAIL_BACKOFF, HOOK_FAIL_LIMIT, PendingPluginCall, PendingRequestKind,
    PluginManager,
};

impl PluginManager {
    pub fn take_pending_plugin_calls(&mut self) -> Vec<PendingPluginCall> {
        std::mem::take(&mut self.pending_plugin_calls)
    }

    /// 라우터가 처리한 결과를 plugin에 송신.
    ///
    /// `error_code` 는 호스트가 준 JSON-RPC 코드다. **버리면 plugin 을 거쳐 나온 응답이
    /// 전부 `-32000`(server error)이 된다** — 호스트가 "인자를 고쳐라"(`-32602`)로
    /// 거절한 것까지 그렇게 되어, 호출자가 재시도 정책을 반대로 고른다.
    /// 코드가 없는 실패(문자열만 있는 내부 경로)는 `None` 을 준다.
    pub fn send_ipc_result(
        &mut self,
        plugin_id: &str,
        call_id: u64,
        result: Option<serde_json::Value>,
        error: Option<String>,
        error_code: Option<i32>,
    ) {
        let req = PluginRequest {
            method: protocol::METHOD_IPC_RESULT.to_string(),
            params: serde_json::to_value(IpcCallResult {
                call_id,
                result,
                error,
                error_code,
            })
            .unwrap_or(serde_json::Value::Null),
            id: self.next_request_id.fetch_add(1, Ordering::Relaxed),
        };
        if let Some(proc) = self.processes.get(plugin_id)
            && let Err(e) = proc.req_tx.send(req)
        {
            tracing::warn!("plugin {plugin_id}: failed to send ipc.result: {e}");
        }
    }

    /// 호스트 본문이 새 envelope를 발화. 호스트는 모든 namespace에 publish 가능.
    /// 매칭되는 모든 plugin 구독자에게 `event.dispatch` 송신.
    pub fn forward_namespace_call(
        &mut self,
        method: &str,
        params: serde_json::Value,
        caller_plugin_id: Option<&str>,
        original_id: serde_json::Value,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
    ) {
        let plugin_id = match self.validate_namespace_call(method, caller_plugin_id) {
            Ok(id) => id,
            Err((code, msg)) => {
                send_response(
                    &response_tx,
                    JsonRpcResponse::error(original_id, code, &msg),
                );
                return;
            }
        };
        self.dispatch_namespace_or_hook(
            plugin_id,
            method.to_string(),
            params,
            caller_plugin_id.map(str::to_string),
            FinalCaller::Local {
                response_tx,
                original_id,
            },
        );
    }

    /// plugin이 보낸 IpcCall이 namespace 메서드인 경우의 forward.
    /// 응답은 caller plugin에 `ipc.result`로 회신한다 (`process_plugin_ipc_calls`와
    /// 같은 방식).
    pub fn forward_namespace_call_from_plugin(
        &mut self,
        method: &str,
        params: serde_json::Value,
        caller_plugin_id: &str,
        call_id: u64,
    ) {
        let plugin_id = match self.validate_namespace_call(method, Some(caller_plugin_id)) {
            Ok(id) => id,
            Err((_code, msg)) => {
                self.send_ipc_result(caller_plugin_id, call_id, None, Some(msg), None);
                return;
            }
        };
        self.dispatch_namespace_or_hook(
            plugin_id,
            method.to_string(),
            params,
            Some(caller_plugin_id.to_string()),
            FinalCaller::Plugin {
                caller_plugin_id: caller_plugin_id.to_string(),
                call_id,
            },
        );
    }

    /// validate를 통과한 namespace 호출을 hook-aware하게 분기.
    ///
    /// - 활성 extension이 있고 pre-hook이 매칭되면 → extension.invoke_hook 먼저 송신.
    /// - 그 외에는 기존처럼 target에 바로 ipc.invoke 송신. 매칭 post-hook이 있으면
    ///   응답 수신 시 post-hook으로 chain.
    /// - caller가 extension 자신이면 self-loop 방지를 위해 hook을 건너뛴다.
    pub(super) fn dispatch_namespace_or_hook(
        &mut self,
        target_plugin_id: String,
        method: String,
        params: serde_json::Value,
        caller_plugin_id: Option<String>,
        final_caller: FinalCaller,
    ) {
        let extension_self = match (
            caller_plugin_id.as_deref(),
            self.extensions
                .active_extension_for_target(&target_plugin_id),
        ) {
            (Some(c), Some(e)) => c == e,
            _ => false,
        };

        let active_ext_with_hooks = if extension_self {
            None
        } else {
            self.find_active_ipc_hooks(&target_plugin_id, &method)
        };

        match active_ext_with_hooks {
            Some((ext_id, pre_opt, post_opt)) => {
                // backoff 중인 hook은 우회.
                let pre = pre_opt.filter(|p| !self.is_hook_in_backoff(&ext_id, &p.method));
                let post = post_opt.filter(|p| !self.is_hook_in_backoff(&ext_id, &p.method));

                if let Some(pre) = pre {
                    let payload = serde_json::json!({
                        "method": method,
                        "params": params,
                        "caller_plugin_id": caller_plugin_id,
                    });
                    let deadline = Instant::now() + Duration::from_millis(pre.timeout_ms as u64);
                    match self.send_extension_invoke_hook(
                        &ext_id,
                        tasty_plugin_protocol::ExtensionHookKind::Ipc,
                        tasty_plugin_protocol::ExtensionHookPhase::Pre,
                        pre.mode,
                        &method,
                        payload,
                    ) {
                        Ok(req_id) => {
                            self.pending_requests.insert(
                                req_id,
                                PendingRequestKind::ExtensionPreIpcHook {
                                    target_plugin_id,
                                    extension_plugin_id: ext_id,
                                    method,
                                    params,
                                    pre_hook_mode: pre.mode,
                                    final_caller,
                                    post_hook: post,
                                    deadline,
                                },
                            );
                        }
                        Err(msg) => {
                            tracing::warn!("pre-hook dispatch failed: {msg}; bypassing hook");
                            self.dispatch_target_invoke(
                                target_plugin_id,
                                method,
                                params,
                                caller_plugin_id.as_deref(),
                                final_caller,
                                post.map(|p| (ext_id.clone(), p)),
                            );
                        }
                    }
                } else if let Some(post) = post {
                    self.dispatch_target_invoke(
                        target_plugin_id,
                        method,
                        params,
                        caller_plugin_id.as_deref(),
                        final_caller,
                        Some((ext_id, post)),
                    );
                } else {
                    self.dispatch_target_invoke(
                        target_plugin_id,
                        method,
                        params,
                        caller_plugin_id.as_deref(),
                        final_caller,
                        None,
                    );
                }
            }
            None => {
                self.dispatch_target_invoke(
                    target_plugin_id,
                    method,
                    params,
                    caller_plugin_id.as_deref(),
                    final_caller,
                    None,
                );
            }
        }
    }

    /// (ext_id, method) 페어가 backoff 중인지 검사. 만료되면 즉시 클리어해서 그 후
    /// 호출은 정상 hook 경로를 탄다.
    pub(super) fn is_hook_in_backoff(&mut self, ext_id: &str, method: &str) -> bool {
        let key = (ext_id.to_string(), method.to_string());
        let now = Instant::now();
        if let Some(state) = self.hook_failures.get(&key)
            && let Some(until) = state.backoff_until
            && now < until
        {
            return true;
        }
        // 만료 시 상태 정리.
        if let Some(state) = self.hook_failures.get_mut(&key)
            && let Some(until) = state.backoff_until
            && now >= until
        {
            state.backoff_until = None;
            state.consecutive_failures = 0;
        }
        false
    }

    /// hook 응답에서 에러/타임아웃이 발생했을 때 호출. 연속 실패 카운터를 증가시키고
    /// 임계를 넘으면 backoff 시작.
    pub(super) fn record_hook_failure(&mut self, ext_id: &str, method: &str) {
        let key = (ext_id.to_string(), method.to_string());
        let state = self.hook_failures.entry(key).or_default();
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= HOOK_FAIL_LIMIT && state.backoff_until.is_none() {
            state.backoff_until = Some(Instant::now() + HOOK_FAIL_BACKOFF);
            tracing::warn!(
                "extension '{ext_id}' hook on '{method}' entered {}s backoff after {} consecutive failures",
                HOOK_FAIL_BACKOFF.as_secs(),
                state.consecutive_failures
            );
        }
    }

    /// hook이 정상 응답하면 호출. 카운터를 0으로 리셋.
    pub(super) fn record_hook_success(&mut self, ext_id: &str, method: &str) {
        let key = (ext_id.to_string(), method.to_string());
        if let Some(state) = self.hook_failures.get_mut(&key) {
            state.consecutive_failures = 0;
            state.backoff_until = None;
        }
    }

    /// target plugin에 실제 ipc.invoke 송신. post-hook이 주어지면 응답 후 chain.
    pub(super) fn dispatch_target_invoke(
        &mut self,
        target_plugin_id: String,
        method: String,
        params: serde_json::Value,
        caller_plugin_id: Option<&str>,
        final_caller: FinalCaller,
        post_hook: Option<(String, IpcHookDecl)>,
    ) {
        let req_id =
            match self.send_namespace_invoke(&target_plugin_id, &method, &params, caller_plugin_id)
            {
                Ok(id) => id,
                Err(msg) => {
                    self.send_final_error(final_caller, -32003, msg);
                    return;
                }
            };
        let kind = match (final_caller, post_hook) {
            (
                FinalCaller::Local {
                    response_tx,
                    original_id,
                },
                None,
            ) => PendingRequestKind::NamespaceInvoke {
                plugin_id: target_plugin_id,
                response_tx,
                original_id,
            },
            (
                FinalCaller::Plugin {
                    caller_plugin_id,
                    call_id,
                },
                None,
            ) => PendingRequestKind::PluginToPluginNamespace {
                plugin_id: target_plugin_id,
                caller_plugin_id,
                call_id,
            },
            (fc, Some((ext_id, decl))) => PendingRequestKind::NamespaceInvokeWithPostHook {
                target_plugin_id,
                method,
                extension_plugin_id: ext_id,
                post_hook_decl: decl,
                final_caller: fc,
            },
        };
        self.pending_requests.insert(req_id, kind);
    }

    /// 활성 extension이 있고 method에 매칭되는 pre/post IPC hook을 검색.
    /// 둘 다 없으면 `None` 반환.
    pub(super) fn find_active_ipc_hooks(
        &self,
        target_plugin_id: &str,
        method: &str,
    ) -> Option<(String, Option<IpcHookDecl>, Option<IpcHookDecl>)> {
        let ext_id = self
            .extensions
            .active_extension_for_target(target_plugin_id)?
            .to_string();
        let pkg = self.packages.iter().find(|p| p.manifest.id == ext_id)?;
        let extends = pkg.manifest.extends.as_ref()?;
        let pre = extends.pre_ipc.iter().find(|h| h.method == method).cloned();
        let post = extends
            .post_ipc
            .iter()
            .find(|h| h.method == method)
            .cloned();
        if pre.is_none() && post.is_none() {
            None
        } else {
            Some((ext_id, pre, post))
        }
    }

    /// extension에 `extension.invoke_hook` 송신. 성공 시 req_id 반환.
    pub(super) fn send_extension_invoke_hook(
        &self,
        extension_plugin_id: &str,
        kind: tasty_plugin_protocol::ExtensionHookKind,
        phase: tasty_plugin_protocol::ExtensionHookPhase,
        mode: HookMode,
        target: &str,
        payload: serde_json::Value,
    ) -> Result<u64, String> {
        let proc = self
            .processes
            .get(extension_plugin_id)
            .ok_or_else(|| format!("extension plugin '{extension_plugin_id}' is not running"))?;
        let mode_str = match mode {
            HookMode::Transform => "transform",
            HookMode::Filter => "filter",
            HookMode::Observe => "observe",
        };
        let kind_str = match kind {
            tasty_plugin_protocol::ExtensionHookKind::Event => "event",
            tasty_plugin_protocol::ExtensionHookKind::Ipc => "ipc",
        };
        let phase_str = match phase {
            tasty_plugin_protocol::ExtensionHookPhase::Pre => "pre",
            tasty_plugin_protocol::ExtensionHookPhase::Post => "post",
        };
        let req_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let req = PluginRequest {
            method: tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK.to_string(),
            params: serde_json::json!({
                "kind": kind_str,
                "phase": phase_str,
                "mode": mode_str,
                "target": target,
                "payload": payload,
            }),
            id: req_id,
        };
        proc.req_tx
            .send(req)
            .map_err(|e| format!("extension '{extension_plugin_id}' send failed: {e}"))?;
        Ok(req_id)
    }

    /// final_caller로 에러 응답 송신.
    pub(super) fn send_final_error(
        &mut self,
        final_caller: FinalCaller,
        code: i32,
        message: String,
    ) {
        match final_caller {
            FinalCaller::Local {
                response_tx,
                original_id,
            } => {
                send_response(
                    &response_tx,
                    JsonRpcResponse::error(original_id, code, &message),
                );
            }
            FinalCaller::Plugin {
                caller_plugin_id,
                call_id,
            } => {
                self.send_ipc_result(&caller_plugin_id, call_id, None, Some(message), None);
            }
        }
    }

    /// final_caller로 성공 응답 송신.
    pub(super) fn send_final_success(
        &mut self,
        final_caller: FinalCaller,
        result: serde_json::Value,
    ) {
        match final_caller {
            FinalCaller::Local {
                response_tx,
                original_id,
            } => {
                send_response(&response_tx, JsonRpcResponse::success(original_id, result));
            }
            FinalCaller::Plugin {
                caller_plugin_id,
                call_id,
            } => {
                self.send_ipc_result(&caller_plugin_id, call_id, Some(result), None, None);
            }
        }
    }

    /// namespace 메서드 호출의 유효성 검사. 성공 시 target plugin id를 반환.
    /// 실패 시 (JSON-RPC code, message) 페어를 반환.
    pub(super) fn validate_namespace_call(
        &self,
        method: &str,
        caller_plugin_id: Option<&str>,
    ) -> Result<String, (i32, String)> {
        let plugin_id = self
            .ipc_namespaces
            .resolve(method)
            .map(str::to_string)
            .ok_or_else(|| (-32601, format!("method '{method}' not found")))?;
        if let Some(caller) = caller_plugin_id {
            if caller == plugin_id {
                return Err((
                    -32001,
                    format!("plugin '{caller}' cannot invoke its own namespace method '{method}'"),
                ));
            }
            let prefix = method.split('.').next().unwrap_or("");
            let required = Permission::IpcInvoke(prefix.to_string());
            let allowed = self
                .plugin_permissions
                .get(caller)
                .map(|set| set.contains(&required))
                .unwrap_or(false);
            if !allowed {
                return Err((
                    -32001,
                    format!(
                        "permission_denied: plugin '{caller}' lacks 'ipc.invoke:{prefix}' \
                         permission for namespace method '{method}'"
                    ),
                ));
            }
        }
        if !self.processes.contains_key(&plugin_id) {
            return Err((-32002, format!("plugin '{plugin_id}' is not running")));
        }
        Ok(plugin_id)
    }

    /// target plugin에 `ipc.invoke` 요청을 송신한다. 성공 시 발급된 host→plugin
    /// request id를 반환. caller는 pending_requests에 추적 kind를 직접 삽입한다.
    pub(super) fn send_namespace_invoke(
        &self,
        plugin_id: &str,
        method: &str,
        params: &serde_json::Value,
        caller_plugin_id: Option<&str>,
    ) -> Result<u64, String> {
        let proc = self
            .processes
            .get(plugin_id)
            .ok_or_else(|| format!("plugin '{plugin_id}' is not running"))?;
        let req_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let req = PluginRequest {
            method: tasty_plugin_protocol::ipc_method::METHOD_IPC_INVOKE.to_string(),
            params: json!({
                "method": method,
                "params": params,
                "caller_plugin_id": caller_plugin_id,
            }),
            id: req_id,
        };
        proc.req_tx
            .send(req)
            .map_err(|e| format!("plugin '{plugin_id}' send failed: {e}"))?;
        Ok(req_id)
    }

    /// 죽거나 비활성화된 plugin이 가진 모든 namespace pending에 에러 응답을 보내고
    /// pending에서 제거한다. CLI/Local caller에게는 response_tx로, plugin caller에게는
    /// `ipc.result`로 회신한다.
    pub(super) fn cancel_pending_namespace_calls(&mut self, plugin_id: &str, reason: &str) {
        let to_cancel: Vec<u64> = self
            .pending_requests
            .iter()
            .filter_map(|(id, kind)| match kind {
                PendingRequestKind::NamespaceInvoke { plugin_id: pid, .. }
                | PendingRequestKind::PluginToPluginNamespace { plugin_id: pid, .. }
                | PendingRequestKind::NamespaceInvokeWithPostHook {
                    target_plugin_id: pid,
                    ..
                } if pid == plugin_id => Some(*id),
                PendingRequestKind::ExtensionPreIpcHook {
                    target_plugin_id: pid,
                    extension_plugin_id: epid,
                    ..
                } if pid == plugin_id || epid == plugin_id => Some(*id),
                PendingRequestKind::ExtensionPostIpcHook {
                    extension_plugin_id: epid,
                    ..
                } if epid == plugin_id => Some(*id),
                PendingRequestKind::ExtensionPreEventHook {
                    publisher_plugin_id: pid,
                    extension_plugin_id: epid,
                    ..
                } if pid == plugin_id || epid == plugin_id => Some(*id),
                PendingRequestKind::ExtensionPostEventHook {
                    extension_plugin_id: epid,
                    ..
                } if epid == plugin_id => Some(*id),
                _ => None,
            })
            .collect();
        for id in to_cancel {
            let msg = format!("plugin '{plugin_id}' unavailable: {reason}");
            match self.pending_requests.remove(&id) {
                Some(PendingRequestKind::NamespaceInvoke {
                    response_tx,
                    original_id,
                    ..
                }) => {
                    send_response(
                        &response_tx,
                        JsonRpcResponse::error(original_id, -32004, &msg),
                    );
                }
                Some(PendingRequestKind::PluginToPluginNamespace {
                    caller_plugin_id,
                    call_id,
                    ..
                }) => {
                    self.send_ipc_result(&caller_plugin_id, call_id, None, Some(msg), None);
                }
                Some(PendingRequestKind::ExtensionPreIpcHook { final_caller, .. })
                | Some(PendingRequestKind::ExtensionPostIpcHook { final_caller, .. })
                | Some(PendingRequestKind::NamespaceInvokeWithPostHook { final_caller, .. }) => {
                    self.send_final_error(final_caller, -32004, msg);
                }
                Some(PendingRequestKind::ExtensionPreEventHook { .. })
                | Some(PendingRequestKind::ExtensionPostEventHook { .. }) => {
                    // event는 fire-and-forget이라 caller에 회신할 필요 없음.
                }
                _ => {}
            }
        }
    }
}
