//! GUI 요청의 대상 자원을 가진 창을 찾는다. 자원 ID 해석은 core::request_target을 공유한다.

use crate::app::App;
use crate::core::request_target::request_resource_id;
use winit::window::WindowId;

/// 아래 메서드의 single_parent 폴백은 한 창 안에서만 유일성을 확인한다.
/// 대상 ID 없이 여러 창 중 하나를 골라야 하면 --surface를 요구한다.
/// 이미 ID가 지정됐다면 이 오류로 가로채지 않고 라우터가 대상 부재를 알리게 한다.
fn ambiguous_parent_fallback_requires_surface(
    method: &str,
    main_window_count: usize,
    target_named: bool,
) -> bool {
    !target_named
        && main_window_count > 1
        && matches!(
            method,
            "terminal.kill" | "terminal.release" | "terminal.respawn" | "terminal.broadcast"
        )
}

/// 숫자·숫자 문자열은 request_resource_id가 먼저 해석하므로 여기서는 nickname만 다룬다.
fn surface_nickname_target<'p>(method: &str, params: &'p serde_json::Value) -> Option<&'p str> {
    if method != "split" {
        return None;
    }
    let nick = params.get("target_surface")?.as_str()?;
    (!nick.is_empty() && nick.parse::<u32>().is_err()).then_some(nick)
}

impl App {
    /// nickname은 창별 상태가 아닌 공용 메모리에서 찾아 실제 소유 창으로 보낸다.
    fn find_main_by_surface_nickname(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Option<WindowId> {
        let nick = surface_nickname_target(method, params)?;
        let sid = self.core.with_memory(|m| {
            crate::surface_meta::SurfaceMetaStore::find_by_value(m, "nickname", nick)
        })?;
        self.find_main_with_resource(crate::core::request_target::ResourceId {
            kind: crate::core::request_target::Kind::Surface,
            id: u64::from(sid),
        })
    }

    /// 자원 ID, surface nickname, 문자열 workspace 순서로 소유 창을 찾는다.
    /// 모호한 workspace 이름이나 대상 없는 다중 창 요청의 오류를 포커스 폴백으로 덮지 않는다.
    pub(crate) fn find_request_owner(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<WindowId>, String> {
        let named = request_resource_id(method, params);
        if let Some(rid) = named {
            let found = self.find_main_with_resource(rid);
            if found.is_some() {
                return Ok(found);
            }
        }
        if let Some(found) = self.find_main_by_surface_nickname(method, params) {
            return Ok(Some(found));
        }
        if let Some(target) = params.get("workspace").and_then(|v| v.as_str()) {
            return self.find_main_with_workspace_target(target);
        }
        if ambiguous_parent_fallback_requires_surface(
            method,
            self.main_window_count(),
            named.is_some(),
        ) {
            return Err(format!(
                "multiple windows open; --surface is required for '{method}' \
                 (cannot infer the target window from focus)"
            ));
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_non_numeric_target_surface_needs_the_nickname_lookup() {
        let j = serde_json::json!({ "target_surface": "faraway" });
        assert_eq!(surface_nickname_target("split", &j), Some("faraway"));
        for p in [
            serde_json::json!({ "target_surface": 7 }),
            serde_json::json!({ "target_surface": "7" }),
            serde_json::json!({ "target_surface": "" }),
            serde_json::json!({}),
        ] {
            assert_eq!(surface_nickname_target("split", &p), None, "{p}");
        }
        assert_eq!(surface_nickname_target("surface.close", &j), None);
    }

    #[test]
    fn single_window_allows_omitted_surface_for_ambiguous_methods() {
        for method in [
            "terminal.kill",
            "terminal.release",
            "terminal.respawn",
            "terminal.broadcast",
        ] {
            assert!(
                !ambiguous_parent_fallback_requires_surface(method, 1, false),
                "{method}: 창이 하나이면 --surface 생략을 허용해야 한다"
            );
        }
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.kill",
            0,
            false
        ));
    }

    #[test]
    fn multiple_windows_reject_omitted_surface_for_ambiguous_methods() {
        for method in [
            "terminal.kill",
            "terminal.release",
            "terminal.respawn",
            "terminal.broadcast",
        ] {
            assert!(
                ambiguous_parent_fallback_requires_surface(method, 2, false),
                "{method}: 창이 둘이면 --surface를 요구해야 한다"
            );
        }
    }

    /// 명시한 ID가 없다는 오류를 --surface 생략 오류로 덮지 않아야 한다.
    #[test]
    fn a_named_target_is_not_answered_with_the_ambiguity_error() {
        for method in [
            "terminal.kill",
            "terminal.release",
            "terminal.respawn",
            "terminal.broadcast",
        ] {
            assert!(
                !ambiguous_parent_fallback_requires_surface(method, 2, true),
                "{method}: 이미 지정한 ID의 부재를 --surface 생략 오류로 덮으면 안 된다"
            );
            assert!(
                ambiguous_parent_fallback_requires_surface(method, 2, false),
                "{method}: 대상이 없으면 다중 창 폴백을 거절해야 한다"
            );
        }
    }

    #[test]
    fn unrelated_methods_are_unaffected_even_with_multiple_windows() {
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.spawn",
            2,
            false
        ));
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.tell",
            2,
            false
        ));
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.children",
            2,
            false
        ));
    }
}
