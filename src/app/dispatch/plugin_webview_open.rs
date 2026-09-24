//! 플러그인의 외부 URL 열기를 호스트로 모아 소유자와 URL을 검사한다.
//! terminal_link::open_uri를 사용해 debug OS 열기 기록 설정도 적용한다.

use serde_json::json;
use tasty_host_plugin::manager::PendingPluginCall;

use crate::app::App;

impl App {
    /// 권한은 진입부 게이트에서 검사한다. OS 열기 실패는 opened=false로 반환한다.
    pub(super) fn handle_ipc_webview_open_external(&mut self, call: &PendingPluginCall) {
        let (result, error, code) = match self.webview_open_request(call) {
            Ok(url) => (
                Some(json!({ "opened": crate::terminal_link::open_uri(&url) })),
                None,
                None,
            ),
            Err(message) => (None, Some(message), Some(-32602)),
        };
        if let Some(mgr) = self.plugin_manager.as_mut() {
            mgr.send_ipc_result(&call.plugin_id, call.call_id, result, error, code);
        }
    }

    fn webview_open_request(&self, call: &PendingPluginCall) -> Result<String, String> {
        let surface_id =
            crate::adapters::ipc::handler::params::read_u32(&call.params, "surface_id")
                .map_err(|m| format!("webview.open_external: {m}"))?
                .ok_or("webview.open_external: missing 'surface_id'")?;
        let url = call
            .params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("webview.open_external: missing 'url'")?;
        let owner = self.plugin_surface_owner(surface_id);
        authorize(owner.as_deref(), &call.plugin_id, surface_id, url)?;
        Ok(url.trim().to_string())
    }

    /// MainView의 RemoteSurface 소유자를 찾는다. parked 상태는 조회하지 않는다.
    fn plugin_surface_owner(&self, surface_id: u32) -> Option<String> {
        self.view.views.values().find_map(|w| {
            let surface = w.as_main()?.core_state.find_surface_by_id(surface_id)?;
            surface
                .as_any()
                .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
                .map(|rs| rs.plugin_id.clone())
        })
    }
}

/// 호출자가 surface 소유자여야 하며 스킴 없는 경로와 javascript URL은 거절한다.
/// 실제 클릭이 있었는지나 surface가 WebView로 렌더되는지는 이 함수가 확인하지 않는다.
pub(crate) fn authorize(
    owner: Option<&str>,
    caller: &str,
    surface_id: u32,
    url: &str,
) -> Result<(), String> {
    match owner {
        None => {
            return Err(format!(
                "webview.open_external: surface {surface_id} is not a live plugin surface"
            ));
        }
        Some(owner) if owner != caller => {
            return Err(format!(
                "webview.open_external: plugin '{caller}' does not own surface {surface_id} (owner '{owner}')"
            ));
        }
        Some(_) => {}
    }
    let scheme = url_scheme(url.trim())
        .ok_or_else(|| format!("webview.open_external: '{url}' has no URL scheme"))?;
    if scheme.eq_ignore_ascii_case("javascript") {
        return Err("webview.open_external: javascript: URLs are not opened".to_string());
    }
    Ok(())
}

/// RFC 3986 스킴(`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) 뒤에 `:` 가 오면 그 스킴.
/// 한 글자 스킴은 Windows 드라이브 문자(`C:\…`)와 겹쳐 스킴으로 치지 않는다.
fn url_scheme(url: &str) -> Option<&str> {
    let (scheme, _) = url.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && scheme.len() > 1
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

#[cfg(test)]
mod tests {
    use super::authorize;

    const P: &str = "com.tasty.markdown";

    #[test]
    fn the_owner_opens_a_url_with_a_scheme() {
        for url in [
            "https://example.com/a?b=1",
            "http://x",
            "mailto:a@b.com",
            "  https://trim.me  ",
        ] {
            assert_eq!(authorize(Some(P), P, 7, url), Ok(()), "{url}");
        }
    }

    #[test]
    fn another_plugins_surface_or_a_missing_one_is_refused() {
        assert!(authorize(Some("com.example.other"), P, 7, "https://x").is_err());
        assert!(authorize(None, P, 7, "https://x").is_err());
    }

    #[test]
    fn a_path_or_a_script_is_not_opened() {
        for url in [
            "",
            "/tmp/a.md",
            "a.md",
            "C:\\docs\\a.md",
            "javascript:alert(1)",
            "JavaScript:alert(1)",
        ] {
            assert!(authorize(Some(P), P, 7, url).is_err(), "{url}");
        }
    }
}
