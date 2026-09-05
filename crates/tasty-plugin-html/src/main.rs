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
    BusHandle, HostHandle, IpcMethodCtx, IpcMethodError, Plugin, SurfaceCreateCtx,
    SurfaceRestoreCtx, SurfaceResult,
};

const PLUGIN_ID: &str = "com.tasty.html";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default)]
struct HtmlPlugin {
    /// `on_start`에서 받아 저장 — `create_surface`(host 가 없는 ctx)에서
    /// `webview.set_url` 을 호출하는 데 재사용한다.
    host: Option<HostHandle>,
}

impl Plugin for HtmlPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn on_start(&mut self, host: HostHandle, _bus: BusHandle) {
        self.host = Some(host);
    }

    fn create_surface(&mut self, ctx: SurfaceCreateCtx) -> SurfaceResult {
        // SDK 는 surface.create 의 전체 envelope 을 ctx.params 로 넘긴다 — 실제 생성
        // params(url 등)는 params.params 아래에 중첩돼 있다(자매 plugin
        // markdown 의 surface_param_file 과 동일 계약).
        let url = surface_param_url(&ctx.params);
        self.open_url_surface(ctx.surface_id, url)
    }

    // layout 재시작 복원 경로. preset apply 는 `surface.create` 를 타지만 layout
    // 재시작은 `surface.restore` 를 탄다 — SDK 기본 구현은 빈 `SurfaceResult` 라,
    // 구현하지 않으면 재시작 시 html 이 url 을 잃고 빈 채로 살아난다. create 가
    // 실어 둔 snapshot(`{"url": ...}`)을 그대로 받아 같은 페이지를 연다.
    fn restore_surface(&mut self, ctx: SurfaceRestoreCtx) -> SurfaceResult {
        let url = ctx
            .data
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        self.open_url_surface(ctx.surface_id, url)
    }

    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        match ctx.method.as_str() {
            "html.open" => html_open(&ctx),
            other => Err(IpcMethodError::not_found(other)),
        }
    }
}

impl HtmlPlugin {
    /// create/restore 공용 — url 을 webview 에 싣고 snapshot 으로 되돌려준다.
    /// 빈 문자열은 "url 없음" 으로 취급한다(빈 채로 열린 html surface 는 snapshot 에
    /// 실을 것이 없다 — markdown 의 file=None 과 같은 계약). host 가 채우는 경로는
    /// create/restore 응답의 `SurfaceResult.snapshot` 이다(host 는 surface.snapshot 을
    /// 따로 부르지 않는다).
    fn open_url_surface(&mut self, surface_id: u32, url: Option<String>) -> SurfaceResult {
        let url = url.filter(|u| !u.is_empty());
        if let Some(u) = url.as_deref() {
            let file_url = local_path_to_file_uri(u);
            match &self.host {
                Some(host) => {
                    if let Err(e) = host.call(
                        "webview.set_url",
                        json!({
                            "surface_id": surface_id,
                            "url": file_url,
                        }),
                    ) {
                        tracing::warn!(
                            "open_url_surface s{surface_id}: webview.set_url failed: {e}"
                        );
                    }
                }
                None => {
                    tracing::warn!(
                        "open_url_surface s{surface_id}: no host handle (on_start not called yet?) — cannot load {u}"
                    );
                }
            }
        }
        SurfaceResult {
            display_name: Some(url.clone().unwrap_or_else(|| "HTML".to_string())),
            snapshot: url.map(|u| json!({ "url": u })),
        }
    }
}

/// surface.create envelope 에서 `url` 을 꺼낸다. SDK 가 `ctx.params` 로 넘기는 것은
/// `{surface_id, kind, cwd, params:{url, ...}}` 전체이므로 `params.url` 을 본다(중첩).
/// flat 으로 온 경우(`url` top-level)도 fallback 으로 받는다.
fn surface_param_url(envelope: &Value) -> Option<String> {
    envelope
        .get("params")
        .and_then(|p| p.get("url"))
        .or_else(|| envelope.get("url"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// `http://`/`https://`/`file://` 스킴이 이미 있으면 그대로 통과. 없으면 로컬
/// 파일시스템 경로로 간주해 `file://` URI 로 변환한다 — host `dispatch.rs` 가
/// 스킴 없는 원시 경로를 그대로 `url` 파라미터에 담아 보내기 때문(다른
/// `OpenSurface` 소비자인 markdown 의 `file` 파라미터 계약을 지키기 위해 host
/// 레벨에서는 변환하지 않는다 — 변환은 이 plugin 이 자기 `url` 파라미터 의미를
/// 아는 여기서만 한다).
fn local_path_to_file_uri(raw: &str) -> String {
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("file://") {
        return raw.to_string();
    }
    let normalized = raw.replace('\\', "/");
    let uri = if let Some(unc) = normalized.strip_prefix("//") {
        // UNC 경로: \\server\share\path → file://server/share/path
        format!("file://{unc}")
    } else if let Some(stripped) = normalized.strip_prefix('/') {
        // POSIX 절대경로: /home/user/a.html → file:///home/user/a.html
        format!("file:///{stripped}")
    } else {
        // Windows 드라이브 경로: C:/Users/a.html → file:///C:/Users/a.html
        format!("file:///{normalized}")
    };
    percent_encode_uri(&uri)
}

/// URI 안전 문자(영숫자 + `-_.~/:`) 외 모든 바이트를 `%XX` 로 이스케이프한다.
/// 공백/`#`/`%`/비-ASCII 문자를 포함한 경로를 다룬다. 스킴/슬래시/콜론은
/// 안전 문자 집합에 포함돼 있어 그대로 보존된다.
fn percent_encode_uri(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
    tasty_plugin_sdk::run(HtmlPlugin::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_param_url_reads_nested_and_flat() {
        assert_eq!(
            surface_param_url(&json!({ "params": { "url": "/a/b.html" } })).as_deref(),
            Some("/a/b.html")
        );
        assert_eq!(
            surface_param_url(&json!({ "url": "/c/d.html" })).as_deref(),
            Some("/c/d.html")
        );
        assert_eq!(surface_param_url(&json!({ "params": {} })), None);
    }

    #[test]
    fn local_html_path_gets_file_scheme() {
        assert_eq!(
            local_path_to_file_uri("/home/user/project/index.html"),
            "file:///home/user/project/index.html"
        );
    }

    #[test]
    fn already_schemed_url_passthrough() {
        assert_eq!(
            local_path_to_file_uri("https://example.com"),
            "https://example.com"
        );
        assert_eq!(
            local_path_to_file_uri("file:///already/schemed.html"),
            "file:///already/schemed.html"
        );
    }

    #[test]
    fn windows_drive_path_gets_file_scheme() {
        assert_eq!(
            local_path_to_file_uri("C:\\Users\\a\\index.html"),
            "file:///C:/Users/a/index.html"
        );
    }

    #[test]
    fn unc_path_gets_file_scheme_with_host() {
        assert_eq!(
            local_path_to_file_uri("\\\\server\\share\\index.html"),
            "file://server/share/index.html"
        );
    }

    #[test]
    fn special_chars_are_percent_encoded() {
        assert_eq!(
            local_path_to_file_uri("/home/user/my file #1 100%.html"),
            "file:///home/user/my%20file%20%231%20100%25.html"
        );
    }

    #[test]
    fn create_surface_carries_url_in_snapshot() {
        let mut p = HtmlPlugin::default();
        let res = p.create_surface(SurfaceCreateCtx {
            surface_id: 1,
            kind: "html".into(),
            cwd: None,
            params: json!({ "surface_id": 1, "kind": "html", "params": { "url": "/a/b.html" } }),
        });
        assert_eq!(res.snapshot, Some(json!({ "url": "/a/b.html" })));
    }

    #[test]
    fn restore_surface_reopens_from_snapshot_and_re_carries_it() {
        // create 가 실은 snapshot 을 layout 재시작이 restore 로 되먹인다 — 같은 url 을
        // 다시 열고 다시 실어야 round-trip 이 반복해서 성립한다.
        let mut p = HtmlPlugin::default();
        let res = p.restore_surface(SurfaceRestoreCtx {
            surface_id: 2,
            kind: "html".into(),
            data: json!({ "url": "/a/b.html" }),
        });
        assert_eq!(res.snapshot, Some(json!({ "url": "/a/b.html" })));
    }

    #[test]
    fn create_without_url_yields_no_snapshot() {
        let mut p = HtmlPlugin::default();
        let res = p.create_surface(SurfaceCreateCtx {
            surface_id: 3,
            kind: "html".into(),
            cwd: None,
            params: json!({ "surface_id": 3, "kind": "html", "params": {} }),
        });
        assert_eq!(res.snapshot, None);
    }
}
