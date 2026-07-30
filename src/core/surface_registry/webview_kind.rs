//! Webview-enabled surface kind 등록.
//!
//! plugin 매니페스트의 `[[surface_kinds]]` 에 `rendering = "webview"` 를 선언하면
//! 호스트가 그 kind 의 surface 가 생성될 때 자동으로 OS-level native WebView overlay
//! 를 생성·동기화한다. plugin 은 `webview.set_url(surface_id, url)` 등 IPC 로
//! URL/navigation 만 제어한다. host 는 어떤 컨텐츠 (html / svg / ...) 인지 모름.
//!
//! egui-mesh 채널과 달리 화이트리스트가 없다 — 어떤 plugin 이든 webview overlay
//! 를 사용할 수 있다 (`webview.*` IPC 권한으로 제어). host 는 자기 builtin kind
//! 정의를 갖지 않으며, plugin 이 정의한 kind 의 SurfaceKindDef 는 일반 remote
//! 메커니즘 (`remote_kind`) 으로 등록된다.
//!
//! 본 모듈은 surface_registry 에 "webview-enabled" 플래그만 기록한다 —
//! `sync_webviews` 가 매 프레임 이 flag 를 확인해서 해당 surface 에 webview
//! overlay 를 붙인다.

use std::collections::HashSet;
use std::sync::RwLock;

/// 전역 webview-enabled kind 집합 (plugin 이 hello 시 등록).
static WEBVIEW_KINDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// plugin manager 가 hello 직후 매니페스트에 `rendering = "webview"` 선언이 있을 때 호출.
pub fn register_webview_kind(plugin_id: &str, kind: &str) {
    let mut guard = WEBVIEW_KINDS.write().unwrap();
    let set = guard.get_or_insert_with(HashSet::new);
    if set.insert(kind.to_string()) {
        tracing::info!(
            "registered webview-enabled surface kind '{}' for plugin '{}'",
            kind,
            plugin_id
        );
    }
}

/// 주어진 surface kind 가 webview overlay 를 사용하는지 query.
/// `sync_webviews` 가 매 프레임 호출.
pub fn is_webview_kind(kind: &str) -> bool {
    WEBVIEW_KINDS
        .read()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.contains(kind)))
        .unwrap_or(false)
}

/// test only — 등록된 kind 모두 제거.
#[cfg(test)]
pub fn reset_for_test() {
    *WEBVIEW_KINDS.write().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_query() {
        reset_for_test();
        assert!(!is_webview_kind("foo"));
        register_webview_kind("com.example", "foo");
        assert!(is_webview_kind("foo"));
        assert!(!is_webview_kind("bar"));
    }
}
