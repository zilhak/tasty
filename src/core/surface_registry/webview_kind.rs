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
    let inserted = {
        let mut guard = WEBVIEW_KINDS.write().unwrap_or_else(|p| p.into_inner());
        let set = guard.get_or_insert_with(HashSet::new);
        set.insert(kind.to_string())
    };
    if inserted {
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
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_some_and(|s| s.contains(kind))
}

/// test only — 등록된 kind 모두 제거.
#[cfg(test)]
pub fn reset_for_test() {
    *WEBVIEW_KINDS.write().unwrap_or_else(|p| p.into_inner()) = None;
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
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

    #[test]
    fn register_and_is_webview_kind_survive_poison() {
        reset_for_test();

        // 의도적으로 poison 유발
        let _ = std::thread::spawn(|| {
            let _guard = WEBVIEW_KINDS.write().unwrap();
            panic!("simulate poison");
        })
        .join();

        // poison 이후에도 패닉하지 않고 정상 등록/조회되어야 한다.
        register_webview_kind("plugin-a", "html");
        assert!(is_webview_kind("html"));
        assert!(!is_webview_kind("markdown"));

        reset_for_test();
    }
}
