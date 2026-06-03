//! Tasty markdown plugin (template/demo).
//!
//! host 가 현재 `markdown` SurfaceKindDef + detector + handler 를 모두 보유한다.
//! 본 crate 는 *향후 host 내장을 분리해낼 때 참고할 수 있는 reference template* 으로
//! workspace 에 존재한다. `crates/tasty-host-plugin/src/builtin.rs` 의 `BUILTINS`
//! 배열에는 등록되지 않으므로 런타임에 로드되지 않는다.

use serde_json::Value;
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
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(MarkdownPlugin)
}
