//! Plugin 부트스트랩 + 메시지 루프.
//!
//! `run(plugin)`은 두 스레드 구조로 동작한다:
//!
//! - **메인 스레드 (receiver)**: 호스트로부터 NDJSON 한 줄씩 받는다. 받은 메시지가
//!   `ipc.result`면 매칭되는 `HostHandle::call` 대기자에게 결과를 전달. 그 외 모든
//!   `PluginRequest`는 worker queue로 enqueue.
//! - **worker 스레드 (dispatcher)**: queue에서 request를 pop해 plugin에 dispatch.
//!   dispatch 안에서 [`HostHandle::call`]을 통해 호스트를 동기 호출하면 메인이
//!   계속 recv 가능하므로 deadlock 없이 결과가 회신된다.
//!
//! shutdown 요청은 메인이 즉시 ack 보내고 worker는 queue가 닫히면 자연스럽게 종료.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::net::TcpStream;
use std::sync::{mpsc, Arc, Mutex};

use anyhow::Result;
use serde_json::Value;

use tasty_plugin_protocol::{
    EventDispatchParams, HandleChannelMessage, IpcCallResult, IpcInvokeParams, PopupClosedParams,
    PopupOpenParams, METHOD_COMMAND_INVOKE, METHOD_EVENT_DISPATCH, METHOD_IPC_INVOKE,
    METHOD_IPC_RESULT, METHOD_PING, METHOD_POPUP_CLOSED, METHOD_POPUP_EVENT, METHOD_POPUP_OPEN,
    METHOD_SHUTDOWN, METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT,
    METHOD_SURFACE_RESTORE, METHOD_SURFACE_SNAPSHOT, PluginEvent, PluginRequest, PluginResponse,
};

use crate::connection::Connection;
use crate::env::PluginEnv;
use crate::handle_channel::HandleClient;
use crate::host::{deliver_ipc_result, HostHandle, PendingCalls, SharedBufferFdPending};
use crate::plugin::{
    CommandInvokeCtx, IpcMethodCtx, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceRestoreCtx,
    SurfaceSnapshotCtx,
};

/// dispatch 내부에서만 쓰이는 에러. JSON-RPC 에러 코드를 보존한다.
pub(crate) struct DispatchError {
    pub message: String,
    pub code: Option<i32>,
}

impl DispatchError {
    fn from_anyhow(e: anyhow::Error) -> Self {
        Self {
            message: e.to_string(),
            code: None,
        }
    }

    fn with_code(message: String, code: i32) -> Self {
        Self {
            message,
            code: Some(code),
        }
    }
}

pub fn run<P: Plugin>(plugin: P) -> Result<()> {
    let env = PluginEnv::load()?;
    // connect + AuthMessage 송신 + AuthAck 5s 대기.
    // 호스트가 토큰을 거부하면 PluginError::HandshakeRejected가 즉시 올라온다.
    let conn = Connection::connect_and_authenticate(&env)?;
    let (writer_stream, mut reader) = conn.into_parts();
    let writer = Arc::new(Mutex::new(writer_stream));

    // 보조 핸들 채널이 활성화되어 있으면 connect한다. 실패는 fatal이 아니라 warn만 남긴다 —
    // 보조 채널을 안 쓰는 plugin이라면 그대로 동작해야 한다 (shared buffer 기능만 비활성).
    let handle_client: Option<HandleClient> = if env.handle_endpoint.is_some() {
        match HandleClient::connect(&env) {
            Ok(c) => {
                tracing::info!("plugin handle channel connected");
                Some(c)
            }
            Err(e) => {
                tracing::warn!("plugin handle channel connect failed: {e}");
                None
            }
        }
    } else {
        None
    };

    // hello event 송신.
    let hello = PluginEvent::Hello {
        plugin_id: plugin.id().to_string(),
        version: plugin.version().to_string(),
    };
    send_event(&writer, &hello)?;

    tracing::info!(
        "plugin '{}' v{} connected to host",
        plugin.id(),
        plugin.version()
    );

    let pending: PendingCalls = Arc::new(Mutex::new(HashMap::new()));
    let shared_buffer_fd_pending: SharedBufferFdPending =
        Arc::new(Mutex::new(HashMap::new()));
    let mut host = HostHandle::new(writer.clone(), pending.clone());

    // 보조 채널이 살아 있으면 reader thread 띄우고 HostHandle에 writer 연결.
    let _handle_reader_thread: Option<std::thread::JoinHandle<()>> = match handle_client {
        Some(client) => {
            #[cfg(unix)]
            {
                match client.reader() {
                    Ok(reader) => {
                        let handle_writer = Arc::new(Mutex::new(client));
                        host = host.with_handle_channel(
                            handle_writer.clone(),
                            shared_buffer_fd_pending.clone(),
                        );
                        let fd_pending_clone = shared_buffer_fd_pending.clone();
                        let writer_clone = handle_writer.clone();
                        let handle = std::thread::Builder::new()
                            .name("plugin-handle-reader".into())
                            .spawn(move || {
                                handle_reader_loop(reader, fd_pending_clone, writer_clone);
                            })?;
                        Some(handle)
                    }
                    Err(e) => {
                        tracing::warn!("plugin handle channel reader split failed: {e}");
                        None
                    }
                }
            }
            #[cfg(not(unix))]
            {
                let _ = client; // Windows에서는 보조 채널 미구현.
                None
            }
        }
        None => None,
    };

    let (req_tx, req_rx) = mpsc::channel::<PluginRequest>();
    let worker_writer = writer.clone();
    let worker_host = host.clone();
    let worker_handle = std::thread::Builder::new()
        .name("plugin-worker".into())
        .spawn(move || worker_loop(plugin, req_rx, worker_writer, worker_host))?;

    // 메인 recv loop.
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break, // host closed
            Ok(_) => {
                let trim = line.trim();
                if trim.is_empty() {
                    continue;
                }
                let req: PluginRequest = match serde_json::from_str(trim) {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("unparseable host line: {e}");
                        continue;
                    }
                };
                if req.method == METHOD_IPC_RESULT {
                    handle_ipc_result_request(&req, &pending, &writer);
                    continue;
                }
                if req.method == METHOD_SHUTDOWN {
                    tracing::info!("plugin received shutdown");
                    let resp = PluginResponse {
                        id: req.id,
                        result: Some(Value::Null),
                        error: None,
                        error_code: None,
                    };
                    let _ = send_response(&writer, &resp);
                    break;
                }
                if req_tx.send(req).is_err() {
                    break;
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue
            }
            Err(e) => {
                tracing::warn!("plugin recv error: {e}");
                break;
            }
        }
    }
    drop(req_tx);
    let _ = worker_handle.join();
    Ok(())
}

/// 보조 채널의 reader thread loop. host가 보낸 `HandleAttach`의 fd를 fd_pending에
/// 매칭해 `HostHandle::create_shared_buffer` 대기자에게 push하고, ping을 받으면 pong을
/// 회신한다. 연결이 닫히면 조용히 종료.
#[cfg(unix)]
fn handle_reader_loop(
    mut reader: crate::handle_channel::HandleClientReader,
    fd_pending: SharedBufferFdPending,
    writer: Arc<Mutex<HandleClient>>,
) {
    loop {
        match reader.recv_message() {
            Ok((msg, aux_fd)) => match msg {
                HandleChannelMessage::HandleAttach { request_id, .. } => match aux_fd {
                    Some(fd) => {
                        let sender = fd_pending
                            .lock()
                            .ok()
                            .and_then(|mut m| m.remove(&request_id));
                        match sender {
                            Some(tx) => {
                                if tx.send(fd).is_err() {
                                    tracing::warn!(
                                        "handle channel: orphan fd for request_id={request_id} (waiter dropped)"
                                    );
                                    // SAFETY: fd는 방금 SCM_RIGHTS로 받은 valid한 file descriptor.
                                    // 매칭되는 waiter가 사라졌으니 leak 방지 위해 close.
                                    unsafe { libc::close(fd) };
                                }
                            }
                            None => {
                                tracing::warn!(
                                    "handle channel: unsolicited HandleAttach (request_id={request_id})"
                                );
                                // SAFETY: 위와 동일 — 미수령 fd는 close해서 leak 방지.
                                unsafe { libc::close(fd) };
                            }
                        }
                    }
                    None => {
                        tracing::warn!(
                            "handle channel: HandleAttach without fd (request_id={request_id})"
                        );
                    }
                },
                HandleChannelMessage::Ping { seq } => {
                    let pong = HandleChannelMessage::Pong { seq };
                    if let Ok(mut w) = writer.lock() {
                        if let Err(e) = w.send_message(&pong) {
                            tracing::warn!("handle channel: pong send failed: {e}");
                        }
                    }
                }
                HandleChannelMessage::Pong { .. } => {
                    // plugin이 ping을 안 보내므로 도착할 일이 없지만 와도 무해.
                }
                HandleChannelMessage::Dirty { .. } => {
                    tracing::warn!("handle channel: plugin received Dirty (unexpected)");
                }
            },
            Err(e) => {
                tracing::debug!("handle channel reader exiting: {e}");
                break;
            }
        }
    }
}

fn worker_loop<P: Plugin>(
    mut plugin: P,
    req_rx: mpsc::Receiver<PluginRequest>,
    writer: Arc<Mutex<TcpStream>>,
    host: HostHandle,
) {
    // dispatch가 시작되기 전에 plugin에 1회 시작 알림. plugin이 여기서 자체
    // background thread를 spawn하면 host call이 안전하게 동작한다 (메인 recv
    // 루프가 이미 동작 중이므로 ipc.result delivery 가능).
    let bus = crate::bus::BusHandle::new(writer.clone(), plugin.id().to_string());
    plugin.on_start(host.clone(), bus);
    for req in req_rx.iter() {
        let result = dispatch(&mut plugin, &req.method, &req.params, &host);
        let resp = build_response(req.id, result);
        if let Err(e) = send_response(&writer, &resp) {
            tracing::warn!("plugin worker send_response failed: {e}");
            break;
        }
    }
}

fn handle_ipc_result_request(
    req: &PluginRequest,
    pending: &PendingCalls,
    writer: &Arc<Mutex<TcpStream>>,
) {
    match serde_json::from_value::<IpcCallResult>(req.params.clone()) {
        Ok(parsed) => {
            deliver_ipc_result(pending, parsed.call_id, parsed.result, parsed.error);
        }
        Err(e) => {
            tracing::warn!("ipc.result parse error: {e}");
        }
    }
    // 호스트는 응답을 기다리지 않지만, JSON-RPC 의미상 ack 응답을 보낸다.
    let ack = PluginResponse {
        id: req.id,
        result: Some(Value::Null),
        error: None,
        error_code: None,
    };
    let _ = send_response(writer, &ack);
}

pub(crate) fn send_event(writer: &Arc<Mutex<TcpStream>>, event: &PluginEvent) -> Result<()> {
    let payload = serde_json::json!({ "event": event });
    let line = serde_json::to_string(&payload)?;
    let mut w = writer.lock().expect("writer lock");
    writeln!(*w, "{line}")?;
    w.flush()?;
    Ok(())
}

pub(crate) fn send_response(
    writer: &Arc<Mutex<TcpStream>>,
    response: &PluginResponse,
) -> Result<()> {
    let line = serde_json::to_string(response)?;
    let mut w = writer.lock().expect("writer lock");
    writeln!(*w, "{line}")?;
    w.flush()?;
    Ok(())
}

pub(crate) fn build_response(id: u64, result: Result<Value, DispatchError>) -> PluginResponse {
    match result {
        Ok(v) => PluginResponse {
            id,
            result: Some(v),
            error: None,
            error_code: None,
        },
        Err(e) => PluginResponse {
            id,
            result: None,
            error: Some(e.message),
            error_code: e.code,
        },
    }
}

pub(crate) fn dispatch<P: Plugin>(
    plugin: &mut P,
    method: &str,
    params: &Value,
    host: &HostHandle,
) -> Result<Value, DispatchError> {
    match method {
        METHOD_PING => Ok(serde_json::json!({"pong": true})),
        METHOD_SURFACE_CREATE => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            let kind = params
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let result = plugin.create_surface(SurfaceCreateCtx {
                surface_id,
                kind,
                params: params.clone(),
            });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_SURFACE_EVENT => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            let ev_value = params
                .get("event")
                .ok_or_else(|| DispatchError {
                    message: "surface.event params missing 'event'".into(),
                    code: None,
                })?;
            let event = serde_json::from_value(ev_value.clone())
                .map_err(|e| DispatchError::from_anyhow(e.into()))?;
            let result = plugin.handle_event(SurfaceEventCtx { surface_id, event });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_SURFACE_RESTORE => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            let kind = params
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let data = params.get("data").cloned().unwrap_or(Value::Null);
            let result = plugin.restore_surface(SurfaceRestoreCtx {
                surface_id,
                kind,
                data,
            });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_SURFACE_SNAPSHOT => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            let data = plugin.snapshot_surface(SurfaceSnapshotCtx { surface_id });
            Ok(serde_json::json!({"data": data}))
        }
        METHOD_SURFACE_DESTROY => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            plugin.destroy_surface(surface_id);
            Ok(Value::Null)
        }
        METHOD_COMMAND_INVOKE => {
            let surface_id = require_surface_id(params).map_err(DispatchError::from_anyhow)?;
            let command_id = params
                .get("command_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| DispatchError {
                    message: "command.invoke params missing 'command_id'".into(),
                    code: None,
                })?
                .to_string();
            let result = plugin.handle_command(CommandInvokeCtx {
                surface_id,
                command_id,
            });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_IPC_INVOKE => {
            let parsed: IpcInvokeParams = serde_json::from_value(params.clone()).map_err(|e| {
                DispatchError::with_code(format!("invalid ipc.invoke params: {e}"), -32602)
            })?;
            match plugin.handle_ipc_method(IpcMethodCtx {
                method: parsed.method,
                params: parsed.params,
                caller_plugin_id: parsed.caller_plugin_id,
                host: host.clone(),
            }) {
                Ok(value) => Ok(value),
                Err(err) => Err(DispatchError::with_code(err.message, err.code)),
            }
        }
        tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK => {
            let parsed: tasty_plugin_protocol::ExtensionHookInvokeParams =
                serde_json::from_value(params.clone()).map_err(|e| {
                    DispatchError::with_code(
                        format!("invalid extension.invoke_hook params: {e}"),
                        -32602,
                    )
                })?;
            let outcome = plugin.handle_extension_hook(crate::plugin::ExtensionHookCtx {
                kind: parsed.kind,
                phase: parsed.phase,
                mode: parsed.mode,
                target: parsed.target,
                payload: parsed.payload,
                host: host.clone(),
            });
            serde_json::to_value(outcome.into_proto())
                .map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_EVENT_DISPATCH => {
            let parsed: EventDispatchParams = serde_json::from_value(params.clone())
                .map_err(|e| {
                    DispatchError::with_code(
                        format!("invalid event.dispatch params: {e}"),
                        -32602,
                    )
                })?;
            plugin.on_event(crate::plugin::EventDispatchCtx {
                sub_id: parsed.sub_id,
                envelope: parsed.envelope,
            });
            // 호스트는 응답을 무시한다. fire-and-forget이라 null 반환.
            Ok(Value::Null)
        }
        METHOD_POPUP_OPEN => {
            let parsed: PopupOpenParams = serde_json::from_value(params.clone()).map_err(|e| {
                DispatchError::with_code(format!("invalid popup.open params: {e}"), -32602)
            })?;
            let result = plugin.open_popup(crate::plugin::PopupOpenCtx {
                popup_id: parsed.popup_id,
                instance_id: parsed.instance_id,
                context: parsed.context,
            });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_POPUP_EVENT => {
            let instance_id = params
                .get("instance_id")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| DispatchError {
                    message: "popup.event params missing 'instance_id'".into(),
                    code: None,
                })?;
            let ev_value = params.get("event").ok_or_else(|| DispatchError {
                message: "popup.event params missing 'event'".into(),
                code: None,
            })?;
            let event = serde_json::from_value(ev_value.clone())
                .map_err(|e| DispatchError::from_anyhow(e.into()))?;
            let result = plugin.handle_popup_event(crate::plugin::PopupEventCtx {
                instance_id,
                event,
            });
            serde_json::to_value(result).map_err(|e| DispatchError::from_anyhow(e.into()))
        }
        METHOD_POPUP_CLOSED => {
            let parsed: PopupClosedParams =
                serde_json::from_value(params.clone()).map_err(|e| {
                    DispatchError::with_code(format!("invalid popup.closed params: {e}"), -32602)
                })?;
            plugin.on_popup_closed(crate::plugin::PopupClosedCtx {
                instance_id: parsed.instance_id,
                reason: parsed.reason,
            });
            Ok(Value::Null)
        }
        other => Err(DispatchError::with_code(
            format!("plugin does not handle method '{other}'"),
            -32601,
        )),
    }
}

fn require_surface_id(params: &Value) -> Result<u32> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| anyhow::anyhow!("missing 'surface_id' parameter"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{IpcMethodCtx, IpcMethodError, SurfaceResult};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct StubPlugin {
        last_ctx: Arc<Mutex<Option<IpcMethodCtx>>>,
        behavior: Behavior,
    }

    #[derive(Clone)]
    enum Behavior {
        Ok(Value),
        Err(IpcMethodError),
    }

    impl Plugin for StubPlugin {
        fn id(&self) -> &str {
            "test.plugin"
        }
        fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
            *self.last_ctx.lock().unwrap() = Some(ctx);
            match self.behavior.clone() {
                Behavior::Ok(v) => Ok(v),
                Behavior::Err(e) => Err(e),
            }
        }
    }

    struct DefaultPlugin;
    impl Plugin for DefaultPlugin {
        fn id(&self) -> &str {
            "test.default"
        }
        fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
    }

    /// 테스트 전용 dummy HostHandle — 실제 호출하지 않고 ctx에 끼우기만 한다.
    fn dummy_host() -> HostHandle {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind localhost");
        let port = listener.local_addr().unwrap().port();
        let accept = std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        accept.join().unwrap();
        let pending: PendingCalls = Arc::new(Mutex::new(HashMap::new()));
        HostHandle::new(Arc::new(Mutex::new(stream)), pending)
    }

    fn invoke_params(method: &str, params: Value, caller: Option<&str>) -> Value {
        let mut obj = serde_json::Map::new();
        obj.insert("method".into(), Value::String(method.into()));
        obj.insert("params".into(), params);
        if let Some(c) = caller {
            obj.insert("caller_plugin_id".into(), Value::String(c.into()));
        }
        Value::Object(obj)
    }

    #[test]
    fn ipc_invoke_ok_serializes_result() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = StubPlugin {
            last_ctx: last.clone(),
            behavior: Behavior::Ok(json!({"ok": true, "n": 42})),
        };
        let host = dummy_host();
        let params = invoke_params("codex.spawn", json!({"cwd": "/tmp"}), None);
        let resp = build_response(7, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
        assert_eq!(resp.id, 7);
        assert!(resp.error.is_none());
        assert_eq!(resp.result, Some(json!({"ok": true, "n": 42})));

        let ctx = last.lock().unwrap().clone().unwrap();
        assert_eq!(ctx.method, "codex.spawn");
        assert_eq!(ctx.params, json!({"cwd": "/tmp"}));
        assert_eq!(ctx.caller_plugin_id, None);
    }

    #[test]
    fn ipc_invoke_not_found_carries_error_code() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = StubPlugin {
            last_ctx: last.clone(),
            behavior: Behavior::Err(IpcMethodError::not_found("codex.bogus")),
        };
        let host = dummy_host();
        let params = invoke_params("codex.bogus", json!({}), None);
        let resp =
            build_response(11, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
        assert_eq!(resp.id, 11);
        assert!(resp.result.is_none());
        assert_eq!(resp.error_code, Some(-32601));
    }

    #[test]
    fn ipc_invoke_passes_caller_plugin_id() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = StubPlugin {
            last_ctx: last.clone(),
            behavior: Behavior::Ok(Value::Null),
        };
        let host = dummy_host();
        let params = invoke_params("codex.spawn", json!({}), Some("com.other.plugin"));
        let _ = build_response(1, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
        let ctx = last.lock().unwrap().clone().unwrap();
        assert_eq!(ctx.caller_plugin_id.as_deref(), Some("com.other.plugin"));

        let params2 = invoke_params("codex.spawn", json!({}), None);
        let _ = build_response(2, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params2, &host));
        let ctx2 = last.lock().unwrap().clone().unwrap();
        assert_eq!(ctx2.caller_plugin_id, None);
    }

    #[test]
    fn ipc_invoke_default_impl_returns_not_implemented() {
        let mut plugin = DefaultPlugin;
        let host = dummy_host();
        let params = invoke_params("codex.spawn", json!({}), None);
        let resp = build_response(3, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
        assert_eq!(resp.error_code, Some(-32601));
        assert!(resp.error.unwrap().contains("not implemented"));
    }

    #[test]
    fn ipc_invoke_invalid_params_returns_minus_32602() {
        let mut plugin = DefaultPlugin;
        let host = dummy_host();
        let params = json!({"params": {}});
        let resp = build_response(4, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
        assert_eq!(resp.error_code, Some(-32602));
    }

    #[test]
    fn unknown_method_returns_minus_32601() {
        let mut plugin = DefaultPlugin;
        let host = dummy_host();
        let resp = build_response(5, dispatch(&mut plugin, "nonsense", &Value::Null, &host));
        assert_eq!(resp.error_code, Some(-32601));
    }

    struct OnStartRecorder {
        called: Arc<Mutex<u32>>,
    }
    impl Plugin for OnStartRecorder {
        fn id(&self) -> &str {
            "test.on_start"
        }
        fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn on_start(&mut self, _host: HostHandle, _bus: crate::bus::BusHandle) {
            *self.called.lock().unwrap() += 1;
        }
    }

    struct ExtensionStubPlugin {
        last_ctx: Arc<Mutex<Option<(
            tasty_plugin_protocol::ExtensionHookKind,
            tasty_plugin_protocol::ExtensionHookPhase,
            tasty_plugin_protocol::ExtensionHookMode,
            String,
            Value,
        )>>>,
        outcome: crate::plugin::ExtensionHookOutcome,
    }
    impl Plugin for ExtensionStubPlugin {
        fn id(&self) -> &str {
            "test.extension"
        }
        fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_extension_hook(
            &mut self,
            ctx: crate::plugin::ExtensionHookCtx,
        ) -> crate::plugin::ExtensionHookOutcome {
            *self.last_ctx.lock().unwrap() =
                Some((ctx.kind, ctx.phase, ctx.mode, ctx.target, ctx.payload));
            self.outcome.clone()
        }
    }

    #[test]
    fn extension_invoke_hook_transform_returns_modified_payload() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = ExtensionStubPlugin {
            last_ctx: last.clone(),
            outcome: crate::plugin::ExtensionHookOutcome::transformed(json!({"x": 99})),
        };
        let host = dummy_host();
        let params = json!({
            "kind": "ipc",
            "phase": "pre",
            "mode": "transform",
            "target": "com.target/method.foo",
            "payload": {"x": 1},
        });
        let resp = build_response(
            21,
            dispatch(
                &mut plugin,
                tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
                &params,
                &host,
            ),
        );
        assert_eq!(resp.id, 21);
        assert!(resp.error.is_none());
        let result = resp.result.expect("result present");
        assert_eq!(
            result.get("modified_payload"),
            Some(&json!({"x": 99}))
        );
        assert!(result.get("pass").is_none() || result.get("pass") == Some(&Value::Null));

        let (kind, phase, mode, target, payload) = last.lock().unwrap().clone().unwrap();
        assert_eq!(kind, tasty_plugin_protocol::ExtensionHookKind::Ipc);
        assert_eq!(phase, tasty_plugin_protocol::ExtensionHookPhase::Pre);
        assert_eq!(mode, tasty_plugin_protocol::ExtensionHookMode::Transform);
        assert_eq!(target, "com.target/method.foo");
        assert_eq!(payload, json!({"x": 1}));
    }

    #[test]
    fn extension_invoke_hook_filter_block_returns_pass_false() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = ExtensionStubPlugin {
            last_ctx: last.clone(),
            outcome: crate::plugin::ExtensionHookOutcome::block(),
        };
        let host = dummy_host();
        let params = json!({
            "kind": "event",
            "phase": "pre",
            "mode": "filter",
            "target": "com.target/event.bar",
            "payload": {},
        });
        let resp = build_response(
            22,
            dispatch(
                &mut plugin,
                tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
                &params,
                &host,
            ),
        );
        let result = resp.result.expect("result present");
        assert_eq!(result.get("pass"), Some(&Value::Bool(false)));
    }

    #[test]
    fn extension_invoke_hook_default_impl_returns_pass() {
        let mut plugin = DefaultPlugin;
        let host = dummy_host();
        let params = json!({
            "kind": "ipc",
            "phase": "post",
            "mode": "observe",
            "target": "com.target/method.foo",
            "payload": {},
        });
        let resp = build_response(
            23,
            dispatch(
                &mut plugin,
                tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
                &params,
                &host,
            ),
        );
        assert!(resp.error.is_none());
        let result = resp.result.expect("result present");
        // pass() = default Outcome → both fields skipped from serialization.
        assert!(result.as_object().unwrap().is_empty());
    }

    #[test]
    fn extension_invoke_hook_invalid_params_returns_minus_32602() {
        let mut plugin = DefaultPlugin;
        let host = dummy_host();
        let params = json!({"kind": "ipc"}); // missing required fields
        let resp = build_response(
            24,
            dispatch(
                &mut plugin,
                tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
                &params,
                &host,
            ),
        );
        assert_eq!(resp.error_code, Some(-32602));
    }

    /// worker_loop이 dispatch 전에 on_start를 정확히 1회 호출해야 한다.
    /// req_rx를 닫아 worker가 즉시 종료하면 on_start만 실행되고 끝.
    #[test]
    fn worker_loop_invokes_on_start_once_before_dispatch() {
        let called = Arc::new(Mutex::new(0u32));
        let plugin = OnStartRecorder {
            called: called.clone(),
        };
        let host = dummy_host();
        // dummy writer: 어디로도 안 가는 TcpStream 페어
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let accept = std::thread::spawn(move || {
            let _ = listener.accept();
        });
        let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        accept.join().unwrap();
        let writer = Arc::new(Mutex::new(stream));

        let (tx, rx) = mpsc::channel::<PluginRequest>();
        drop(tx); // queue 즉시 닫기 — worker_loop은 on_start 후 iter()로 빠져나간다.
        let join = std::thread::spawn(move || {
            worker_loop(plugin, rx, writer, host);
        });
        join.join().unwrap();
        assert_eq!(*called.lock().unwrap(), 1);
    }

    /// popup.open / popup.event / popup.closed 라우팅과 콜백 호출 검증.
    struct PopupStubPlugin {
        opened: Arc<Mutex<Vec<(String, u64, Value)>>>,
        events: Arc<Mutex<Vec<(u64, tasty_plugin_protocol::UiEvent)>>>,
        closed: Arc<Mutex<Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>>>,
        next_open_tree: Option<tasty_plugin_protocol::UiNode>,
        request_close: bool,
    }

    impl Plugin for PopupStubPlugin {
        fn id(&self) -> &str {
            "test.popup"
        }
        fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
            SurfaceResult {
                tree: None,
                display_name: None,
            }
        }
        fn open_popup(
            &mut self,
            ctx: crate::plugin::PopupOpenCtx,
        ) -> tasty_plugin_protocol::PopupOpenResult {
            self.opened
                .lock()
                .unwrap()
                .push((ctx.popup_id, ctx.instance_id, ctx.context));
            tasty_plugin_protocol::PopupOpenResult {
                tree: self.next_open_tree.take(),
            }
        }
        fn handle_popup_event(
            &mut self,
            ctx: crate::plugin::PopupEventCtx,
        ) -> tasty_plugin_protocol::PopupEventResult {
            self.events.lock().unwrap().push((ctx.instance_id, ctx.event));
            tasty_plugin_protocol::PopupEventResult {
                tree: None,
                close: self.request_close,
            }
        }
        fn on_popup_closed(&mut self, ctx: crate::plugin::PopupClosedCtx) {
            self.closed.lock().unwrap().push((ctx.instance_id, ctx.reason));
        }
    }

    fn make_popup_plugin() -> PopupStubPlugin {
        PopupStubPlugin {
            opened: Arc::new(Mutex::new(Vec::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            closed: Arc::new(Mutex::new(Vec::new())),
            next_open_tree: None,
            request_close: false,
        }
    }

    #[test]
    fn popup_open_calls_plugin_with_ctx_and_returns_tree() {
        let mut plugin = make_popup_plugin();
        plugin.next_open_tree = Some(tasty_plugin_protocol::UiNode::Spacer { size: 4 });
        let opened = plugin.opened.clone();
        let host = dummy_host();
        let params = json!({
            "popup_id": "search",
            "instance_id": 7,
            "context": {"q": "abc"},
        });
        let resp = build_response(1, dispatch(&mut plugin, METHOD_POPUP_OPEN, &params, &host));
        assert!(resp.error.is_none(), "got error: {:?}", resp.error);
        let tree = resp.result.unwrap().get("tree").cloned().unwrap();
        assert!(!tree.is_null());

        let opened = opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].0, "search");
        assert_eq!(opened[0].1, 7);
        assert_eq!(opened[0].2["q"], "abc");
    }

    #[test]
    fn popup_event_request_close_propagates() {
        let mut plugin = make_popup_plugin();
        plugin.request_close = true;
        let host = dummy_host();
        let params = json!({
            "instance_id": 11,
            "event": {"kind": "click", "node_id": "btn"},
        });
        let resp = build_response(2, dispatch(&mut plugin, METHOD_POPUP_EVENT, &params, &host));
        assert!(resp.error.is_none(), "got error: {:?}", resp.error);
        let r = resp.result.unwrap();
        assert_eq!(r.get("close"), Some(&Value::Bool(true)));
    }

    #[test]
    fn popup_closed_dispatch_invokes_callback() {
        let mut plugin = make_popup_plugin();
        let closed = plugin.closed.clone();
        let host = dummy_host();
        let params = json!({"instance_id": 5, "reason": "outside_click"});
        let resp = build_response(3, dispatch(&mut plugin, METHOD_POPUP_CLOSED, &params, &host));
        assert!(resp.error.is_none(), "got error: {:?}", resp.error);
        assert_eq!(resp.result, Some(Value::Null));

        let c = closed.lock().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].0, 5);
        assert_eq!(c[0].1, tasty_plugin_protocol::PopupCloseReason::OutsideClick);
    }
}
