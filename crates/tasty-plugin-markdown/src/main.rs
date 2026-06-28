#![forbid(unsafe_code)]

//! Tasty markdown plugin — host-rendered markdown viewer surface.
//!
//! `markdown` SurfaceKindDef 본체는 host 의 `host_rendered` whitelist 경유로
//! 등록되며 (image plugin 과 동일 패턴), detector + handler + cli 는 본
//! manifest 가 contribute. `BUILTINS` 배열에 등록되어 첫 부팅 시 자동 install.

use serde_json::{Value, json};
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.markdown";
const PLUGIN_VERSION: &str = "0.1.0";

struct MarkdownPlugin;

impl Plugin for MarkdownPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "markdown.reload" => markdown_reload(&ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

fn markdown_reload(params: &Value) -> Result<Value, IpcMethodError> {
    let surface_id = params
        .get("surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'surface'"))?;
    Ok(json!({ "ok": true, "surface_id": surface_id }))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(MarkdownPlugin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reload_with_surface_id_returns_ok() {
        let resp = markdown_reload(&json!({ "surface": 42 })).expect("reload should succeed");
        assert_eq!(resp["ok"], json!(true));
        assert_eq!(resp["surface_id"], json!(42));
    }

    #[test]
    fn reload_without_surface_id_is_invalid_params() {
        let err = markdown_reload(&json!({})).unwrap_err();
        assert_eq!(err.code, -32602);
        assert!(err.message.contains("surface"));
    }
}
