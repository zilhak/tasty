//! Tasty markdown plugin (template/demo).
//!
//! host 가 현재 `markdown` SurfaceKindDef + detector + handler 를 모두 보유한다.
//! 본 crate 는 *향후 host 내장을 분리해낼 때 참고할 수 있는 reference template* 으로
//! workspace 에 존재한다. `crates/tasty-host-plugin/src/builtin.rs` 의 `BUILTINS`
//! 배열에는 등록되지 않으므로 런타임에 로드되지 않는다.

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
