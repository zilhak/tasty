//! Plugin 부트스트랩 + 메시지 루프.
//!
//! `run(plugin)`은:
//! 1. 환경변수 읽고 호스트에 connect + auth.
//! 2. `Hello` 이벤트 송신.
//! 3. 호스트의 PluginRequest를 한 줄씩 받아 plugin 메서드 dispatch + 응답 송신.
//! 4. 호스트가 connection을 닫거나 `shutdown` 메서드를 보내면 종료.

use anyhow::Result;
use serde_json::Value;

use tasty_plugin_protocol::{
    IpcInvokeParams, METHOD_COMMAND_INVOKE, METHOD_IPC_INVOKE, METHOD_PING, METHOD_SHUTDOWN,
    METHOD_SURFACE_CREATE, METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT, METHOD_SURFACE_RESTORE,
    METHOD_SURFACE_SNAPSHOT, PluginEvent, PluginResponse,
};

use crate::connection::{Connection, HostMessage};
use crate::env::PluginEnv;
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

pub fn run<P: Plugin>(mut plugin: P) -> Result<()> {
    let env = PluginEnv::load()?;
    let mut conn = Connection::connect_and_authenticate(&env)?;

    // hello 송신.
    let hello = PluginEvent::Hello {
        plugin_id: plugin.id().to_string(),
        version: plugin.version().to_string(),
    };
    conn.send_event(&hello)?;

    tracing::info!(
        "plugin '{}' v{} connected to host",
        plugin.id(),
        plugin.version()
    );

    loop {
        match conn.try_recv() {
            Ok(None) => {
                continue;
            }
            Ok(Some(HostMessage::Request(req))) => {
                if req.method == METHOD_SHUTDOWN {
                    tracing::info!("plugin '{}' received shutdown", plugin.id());
                    let resp = PluginResponse {
                        id: req.id,
                        result: Some(Value::Null),
                        error: None,
                        error_code: None,
                    };
                    let _ = conn.send_response(&resp);
                    break;
                }
                let response = build_response(req.id, dispatch(&mut plugin, &req.method, &req.params));
                conn.send_response(&response)?;
            }
            Err(e) => {
                tracing::warn!("plugin '{}' recv error: {e}", plugin.id());
                break;
            }
        }
    }
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
        fn handle_ipc_method(
            &mut self,
            ctx: IpcMethodCtx,
        ) -> Result<Value, IpcMethodError> {
            *self.last_ctx.lock().unwrap() = Some(ctx);
            match self.behavior.clone() {
                Behavior::Ok(v) => Ok(v),
                Behavior::Err(e) => Err(e),
            }
        }
    }

    /// 기본 구현(`not_implemented`)을 확인하기 위해 `handle_ipc_method`를
    /// override하지 않는 plugin.
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
        let params = invoke_params("codex.spawn", json!({"cwd": "/tmp"}), None);
        let resp = build_response(7, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params));
        assert_eq!(resp.id, 7);
        assert!(resp.error.is_none());
        assert!(resp.error_code.is_none());
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
        let params = invoke_params("codex.bogus", json!({}), None);
        let resp = build_response(11, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params));
        assert_eq!(resp.id, 11);
        assert!(resp.result.is_none());
        assert_eq!(resp.error_code, Some(-32601));
        let msg = resp.error.unwrap();
        assert!(msg.contains("codex.bogus"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn ipc_invoke_passes_caller_plugin_id() {
        let last = Arc::new(Mutex::new(None));
        let mut plugin = StubPlugin {
            last_ctx: last.clone(),
            behavior: Behavior::Ok(Value::Null),
        };
        let params = invoke_params("codex.spawn", json!({}), Some("com.other.plugin"));
        let _ = build_response(1, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params));
        let ctx = last.lock().unwrap().clone().unwrap();
        assert_eq!(ctx.caller_plugin_id.as_deref(), Some("com.other.plugin"));

        // None caller도 동일하게 전달되는지.
        let params2 = invoke_params("codex.spawn", json!({}), None);
        let _ = build_response(2, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params2));
        let ctx2 = last.lock().unwrap().clone().unwrap();
        assert_eq!(ctx2.caller_plugin_id, None);
    }

    #[test]
    fn ipc_invoke_default_impl_returns_not_implemented() {
        let mut plugin = DefaultPlugin;
        let params = invoke_params("codex.spawn", json!({}), None);
        let resp = build_response(3, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params));
        assert_eq!(resp.error_code, Some(-32601));
        assert!(resp.error.unwrap().contains("not implemented"));
    }

    #[test]
    fn ipc_invoke_invalid_params_returns_minus_32602() {
        let mut plugin = DefaultPlugin;
        // method 필드 누락 → IpcInvokeParams deserialize 실패.
        let params = json!({"params": {}});
        let resp = build_response(4, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params));
        assert_eq!(resp.error_code, Some(-32602));
    }

    #[test]
    fn unknown_method_returns_minus_32601() {
        let mut plugin = DefaultPlugin;
        let resp = build_response(5, dispatch(&mut plugin, "nonsense", &Value::Null));
        assert_eq!(resp.error_code, Some(-32601));
    }
}
