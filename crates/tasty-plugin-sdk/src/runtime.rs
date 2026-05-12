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
    IpcCallResult, IpcInvokeParams, METHOD_COMMAND_INVOKE, METHOD_IPC_INVOKE, METHOD_IPC_RESULT,
    METHOD_PING, METHOD_SHUTDOWN, METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY,
    METHOD_SURFACE_EVENT, METHOD_SURFACE_RESTORE, METHOD_SURFACE_SNAPSHOT, PluginEvent,
    PluginRequest, PluginResponse,
};

use crate::connection::Connection;
use crate::env::PluginEnv;
use crate::host::{deliver_ipc_result, HostHandle, PendingCalls};
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
    let host = HostHandle::new(writer.clone(), pending.clone());

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

fn worker_loop<P: Plugin>(
    mut plugin: P,
    req_rx: mpsc::Receiver<PluginRequest>,
    writer: Arc<Mutex<TcpStream>>,
    host: HostHandle,
) {
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
}
