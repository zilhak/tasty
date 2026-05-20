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
use std::sync::{Arc, Mutex, mpsc};

use anyhow::Result;
use serde_json::Value;

use tasty_plugin_protocol::{
    EventDispatchParams, HandleChannelMessage, IpcCallResult, IpcInvokeParams,
    METHOD_COMMAND_INVOKE, METHOD_EVENT_DISPATCH, METHOD_IPC_INVOKE, METHOD_IPC_RESULT,
    METHOD_PING, METHOD_POPUP_CLOSED, METHOD_POPUP_EVENT, METHOD_POPUP_OPEN, METHOD_SHUTDOWN,
    METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT, METHOD_SURFACE_RESTORE,
    METHOD_SURFACE_SNAPSHOT, PluginEvent, PluginRequest, PluginResponse, PopupClosedParams,
    PopupOpenParams,
};

use crate::connection::Connection;
use crate::env::PluginEnv;
use crate::handle_channel::HandleClient;
use crate::host::{HostHandle, PendingCalls, SharedBufferFdPending, deliver_ipc_result};
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
    let shared_buffer_fd_pending: SharedBufferFdPending = Arc::new(Mutex::new(HashMap::new()));
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
                // Windows에서는 보조 채널 미구현 — client는 drop된다.
                let _client = client;
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
                    if let Err(e) = send_response(&writer, &resp) {
                        tracing::trace!("shutdown ack send failed (host closing): {e}");
                    }
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
                continue;
            }
            Err(e) => {
                tracing::warn!("plugin recv error: {e}");
                break;
            }
        }
    }
    drop(req_tx);
    if let Err(e) = worker_handle.join() {
        tracing::warn!("plugin worker thread panicked: {e:?}");
    }
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
    if let Err(e) = send_response(writer, &ack) {
        tracing::trace!("ipc.result ack send failed: {e}");
    }
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
            let ev_value = params.get("event").ok_or_else(|| DispatchError {
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
            let parsed: EventDispatchParams =
                serde_json::from_value(params.clone()).map_err(|e| {
                    DispatchError::with_code(format!("invalid event.dispatch params: {e}"), -32602)
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
            let result =
                plugin.handle_popup_event(crate::plugin::PopupEventCtx { instance_id, event });
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
#[path = "runtime_tests.rs"]
mod tests;
