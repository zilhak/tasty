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
    METHOD_COMMAND_INVOKE, METHOD_PING, METHOD_SHUTDOWN, METHOD_SURFACE_CREATE,
    METHOD_SURFACE_DESTROY, METHOD_SURFACE_EVENT, METHOD_SURFACE_RESTORE, METHOD_SURFACE_SNAPSHOT,
    PluginEvent, PluginResponse,
};

use crate::connection::{Connection, HostMessage};
use crate::env::PluginEnv;
use crate::plugin::{
    CommandInvokeCtx, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceRestoreCtx,
    SurfaceSnapshotCtx,
};

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
                    };
                    let _ = conn.send_response(&resp);
                    break;
                }
                let result = dispatch(&mut plugin, &req.method, &req.params);
                let response = match result {
                    Ok(v) => PluginResponse {
                        id: req.id,
                        result: Some(v),
                        error: None,
                    },
                    Err(e) => PluginResponse {
                        id: req.id,
                        result: None,
                        error: Some(e.to_string()),
                    },
                };
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

fn dispatch<P: Plugin>(plugin: &mut P, method: &str, params: &Value) -> Result<Value> {
    match method {
        METHOD_PING => Ok(serde_json::json!({"pong": true})),
        METHOD_SURFACE_CREATE => {
            let surface_id = require_surface_id(params)?;
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
            Ok(serde_json::to_value(result)?)
        }
        METHOD_SURFACE_EVENT => {
            let surface_id = require_surface_id(params)?;
            let ev_value = params
                .get("event")
                .ok_or_else(|| anyhow::anyhow!("surface.event params missing 'event'"))?;
            let event = serde_json::from_value(ev_value.clone())?;
            let result = plugin.handle_event(SurfaceEventCtx { surface_id, event });
            Ok(serde_json::to_value(result)?)
        }
        METHOD_SURFACE_RESTORE => {
            let surface_id = require_surface_id(params)?;
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
            Ok(serde_json::to_value(result)?)
        }
        METHOD_SURFACE_SNAPSHOT => {
            let surface_id = require_surface_id(params)?;
            let data = plugin.snapshot_surface(SurfaceSnapshotCtx { surface_id });
            Ok(serde_json::json!({"data": data}))
        }
        METHOD_SURFACE_DESTROY => {
            let surface_id = require_surface_id(params)?;
            plugin.destroy_surface(surface_id);
            Ok(Value::Null)
        }
        METHOD_COMMAND_INVOKE => {
            let surface_id = require_surface_id(params)?;
            let command_id = params
                .get("command_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("command.invoke params missing 'command_id'"))?
                .to_string();
            let result = plugin.handle_command(CommandInvokeCtx {
                surface_id,
                command_id,
            });
            Ok(serde_json::to_value(result)?)
        }
        other => Err(anyhow::anyhow!("plugin does not handle method '{other}'")),
    }
}

fn require_surface_id(params: &Value) -> Result<u32> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| anyhow::anyhow!("missing 'surface_id' parameter"))
}
