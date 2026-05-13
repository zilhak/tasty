//! Tasty Image plugin — 외부 plugin.
//!
//! `image` surface kind와 `image.*` IPC 네임스페이스를 점유한다. 실제 픽셀
//! 렌더링은 호스트가 담당하는 host-rendered kind(`rendering = "host"`)이며,
//! plugin은 manifest 등록과 `image.*` IPC trampoline만 제공한다.

use serde_json::Value;
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.image";
const PLUGIN_VERSION: &str = "0.1.0";

struct ImagePlugin;

impl Plugin for ImagePlugin {
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
            "image.open"
            | "image.save"
            | "image.export_png"
            | "image.next"
            | "image.prev"
            | "image.paste"
            | "image.list" => Err(IpcMethodError::not_implemented()),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(ImagePlugin)
}
