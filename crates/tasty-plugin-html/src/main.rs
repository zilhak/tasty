#![forbid(unsafe_code)]

//! Tasty HTML plugin — host webview overlay 를 사용해 HTML/URL 을 표시하는 surface.
//!
//! host 는 webview 토대 (OS-level native overlay) 만 제공하고, html surface 의
//! 모든 도메인 로직은 본 plugin 안. surface kind="html", rendering="webview"
//! 매니페스트 선언으로 host 가 surface 생성 시 webview overlay 자동 생성.
//!
//! `html.open(url, surface)` IPC 가 host 의 `webview.set_url` 로 URL 전달.

use serde_json::{Value, json};
use tasty_plugin_sdk::{
    IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx, SurfaceEventCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.html";
const PLUGIN_VERSION: &str = "0.1.0";

struct HtmlPlugin;

impl Plugin for HtmlPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // surface create params 의 url 을 display_name 으로. host 의 webview
        // overlay 가 URL 을 직접 표시한다. 실제 URL 설정은 ctx.host 가 없는 환경
        // 이라 별도 IPC (html.open) 또는 surface meta 로 처리한다.
        let url = ctx.params.get("url").and_then(|v| v.as_str()).unwrap_or("");
        SurfaceResult {
            tree: None,
            display_name: Some(if url.is_empty() {
                "HTML".to_string()
            } else {
                url.to_string()
            }),
            snapshot: None,
        }
    }

    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "html.open" => html_open(&ctx),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

/// `html.open(url, surface)` — host 의 webview.set_url 로 URL 전달.
fn html_open(ctx: &IpcMethodCtx) -> Result<Value, IpcMethodError> {
    let url = ctx
        .params
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params("missing 'url'"))?
        .to_string();
    let surface_id = ctx
        .params
        .get("surface")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::invalid_params("missing 'surface' — specify --surface <id>")
        })?;

    ctx.host
        .call(
            "webview.set_url",
            json!({
                "surface_id": surface_id,
                "url": url,
            }),
        )
        .map_err(|e| IpcMethodError::invalid_params(&format!("webview.set_url failed: {e}")))?;

    Ok(json!({ "ok": true, "surface_id": surface_id }))
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    tasty_plugin_sdk::run(HtmlPlugin)
}
