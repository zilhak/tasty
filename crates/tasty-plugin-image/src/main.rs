//! Tasty Image plugin — 외부 plugin.
//!
//! `image` surface kind와 `image.*` IPC 네임스페이스를 점유한다. 실제 픽셀
//! 렌더링은 호스트가 담당하는 host-rendered kind(`rendering = "host"`)이며,
//! 모든 `image.*` IPC 메서드는 호스트의 동명 메서드로 trampoline한다.
//!
//! self-call(caller==owner)이 들어오면 plugin manager가 namespace forward를
//! 건너뛰고 호스트 dispatcher로 통과시키므로 무한 루프가 발생하지 않는다.

use serde_json::Value;
use tasty_plugin_sdk::{
    host::HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx,
    SurfaceResult,
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
        // host-rendered kind라 plugin이 tree를 만들지 않는다. 호스트가 직접 그린다.
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
            | "image.list" => trampoline(&ctx.host, &ctx.method, ctx.params),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

/// `image.*` 메서드를 호스트의 동명 IPC로 위임한다. plugin manager가 self-call을
/// 호스트 dispatcher로 우회시키므로 무한 forward 루프는 발생하지 않는다.
fn trampoline(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    Ok(host.call(method, params)?)
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
