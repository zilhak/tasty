//! 요청의 **주인 창**을 찾는다 — 창 순회가 필요한 부분(gui 전용).
//!
//! 요청이 무엇을 지목했는지 자체를 푸는 순수 부분은
//! [`crate::core::request_target`] 에 있다. 헤드리스도 같은 판정을 써야 해서
//! 거기로 옮겼다 — 한쪽만 거절하면 같은 요청이 조합에 따라 다르게 끝난다.

use crate::app::App;
use crate::core::request_target::request_resource_id;
use winit::window::WindowId;

/// `terminal.kill`/`terminal.release`/`terminal.respawn`/`terminal.broadcast` 가
/// `--surface`(parent) 생략 시 기대는 host `single_parent()` 폴백은 **호출이 실제로
/// 라우팅된 그 window 안에서만** 유일성을 본다 — 다중 윈도우 세션에서는 애초에 어느
/// window 를 봐야 하는지가 정해지지 않는다. 이 4개 메서드가 리소스 id 없이(= 이
/// 함수 호출 시점에 `params_resource_id`/`workspace` 문자열 둘 다로 owner 를 못 찾은
/// 채) 호출됐는데 main window 가 2개 이상 열려 있으면, `find_request_owner` 가
/// focused window 로 조용히 새지 않고 명시적 `--surface` 를 요구한다(호출자가
/// 명시하지 않는 한 대상 window 를 추론할 근거가 없다). window 가 1개뿐이면 기존
/// 동작 그대로(하위 호환) — `single_parent()` 가 그 안에서 0/2+ parent 를 여전히 스스로 거부한다.
///
/// 순수 함수로 분리해 `App`/`winit` 없이 단위 테스트한다(`find_workspace_by_name`
/// 와 동일 패턴, `window_access.rs` 참고).
fn ambiguous_parent_fallback_requires_surface(method: &str, main_window_count: usize) -> bool {
    main_window_count > 1
        && matches!(
            method,
            "terminal.kill" | "terminal.release" | "terminal.respawn" | "terminal.broadcast"
        )
}

/// `split` 의 `target_surface` 중 **nickname 으로만 풀리는 값**.
///
/// 숫자와 숫자로 읽히는 문자열은 [`request_resource_id`] 가 이미 봤다 — 여기서
/// nickname 으로 다시 찾으면 핸들러와 우선순위가 어긋난다(`pane::resolve_surface_target`
/// 도 숫자를 먼저 본다). `App`/`winit` 없이 단위 테스트하려고 순수 함수로 뗀다
/// (`ambiguous_parent_fallback_requires_surface` 와 같은 패턴).
fn surface_nickname_target<'p>(method: &str, params: &'p serde_json::Value) -> Option<&'p str> {
    if method != "split" {
        return None;
    }
    let nick = params.get("target_surface")?.as_str()?;
    (!nick.is_empty() && nick.parse::<u32>().is_err()).then_some(nick)
}

impl App {
    /// `split` 의 `target_surface` 가 **nickname** 일 때 그 surface 를 가진 창.
    ///
    /// [`request_resource_id`] 는 `App` 없이 도는 순수 함수라 memory store 를 못 본다 —
    /// 그래서 숫자(또는 숫자로 읽히는 문자열)까지만 거기서 풀고, nickname 은 여기서 푼다.
    /// nickname→surface 매핑은 `Core` 의 memory 에 있어 **창에 안 매이므로** 창을 건너
    /// 정확히 풀린다.
    ///
    /// 안 풀면 요청이 포커스된 창으로 가고, 핸들러가 **같은 nickname 을 풀어** 얻은
    /// surface 가 그 창에 없어 "surface N not found" 로 끝난다 — 실측(2026-09-05):
    /// 비포커스 창의 surface 에 nickname 을 달고 그 이름으로 split 하면 실패했고,
    /// 포커스를 옮기면 같은 요청이 성공했다.
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

    /// request.params 에 resource id 가 있으면 그 리소스를 가진 MainView 의 id 반환.
    ///
    /// 숫자 키(`params_resource_id`)로 못 찾으면 `terminal.spawn` 의 `"workspace"`
    /// (문자열, id 또는 표시 이름)를 마지막으로 시도한다 — 이건 u64 전용 추출
    /// 루프로는 못 뽑는다(이름일 수 있어서). `"parent"`/`"surface"` 가 같은
    /// request 에 있으면 이미 그쪽에서 찾아졌을 것이므로, 이 폴백은 사실상
    /// 순수 `terminal.spawn` 직접 호출(parent 없이 workspace 만 지정)에만 닿는다.
    ///
    /// `Err`은 두 경우에 반환된다 — 호출자는 어느 쪽이든 focused window 로 조용히
    /// 폴백하지 말고 명확한 에러를 클라이언트에 돌려줘야 한다:
    /// - workspace 이름이 2개 이상 window 에 걸쳐 모호하게 일치할 때
    ///   (`find_main_with_workspace_target` 참고)
    /// - `method` 가 [`ambiguous_parent_fallback_requires_surface`] 에 해당하고
    ///   (`terminal.kill`/`terminal.release`/`terminal.respawn`/`terminal.broadcast`)
    ///   리소스 id 를 전혀 못 찾은 채 main window 가 2개 이상 열려 있을 때
    pub(crate) fn find_request_owner(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Option<WindowId>, String> {
        if let Some(rid) = request_resource_id(method, params) {
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
        if ambiguous_parent_fallback_requires_surface(method, self.main_window_count()) {
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

    /// main window 가 1개뿐이면(단일 윈도우 세션) `--surface` 생략을 그대로
    /// 허용한다 — host `single_parent()` 폴백이 여전히 그 안에서 유일성을
    /// 판단하므로 하위 호환을 깨지 않는다.
    /// nickname 갈래만 이 경로로 온다 — 숫자·숫자문자열은 순수 라우팅이 이미 풀었고,
    /// 여기서 또 풀면 핸들러와 우선순위가 갈린다.
    #[test]
    fn only_a_non_numeric_target_surface_needs_the_nickname_lookup() {
        let j = serde_json::json!({ "target_surface": "faraway" });
        assert_eq!(surface_nickname_target("split", &j), Some("faraway"));
        // 숫자 · 숫자문자열 · 빈 문자열 · 다른 메서드는 여기 안 온다.
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
                !ambiguous_parent_fallback_requires_surface(method, 1),
                "{method} 는 window 1개일 때 생략을 허용해야 함"
            );
        }
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.kill",
            0
        ));
    }

    /// main window 가 2개 이상이면 kill/release/respawn/broadcast 는 `--surface`
    /// 생략을 거부해야 한다 — focused window 로 조용히 새면 안 보이는 다른
    /// window 의 데이터를 조작할 수 있다.
    #[test]
    fn multiple_windows_reject_omitted_surface_for_ambiguous_methods() {
        for method in [
            "terminal.kill",
            "terminal.release",
            "terminal.respawn",
            "terminal.broadcast",
        ] {
            assert!(
                ambiguous_parent_fallback_requires_surface(method, 2),
                "{method} 는 window 2개일 때 --surface 를 요구해야 함"
            );
        }
    }

    /// 이 4개 메서드 밖의 다른 메서드는 이 판정에 걸리지 않는다 — 다중 윈도우여도
    /// 기존 focused-window 폴백을 그대로 유지한다(범위 밖 메서드의 동작 변경 금지).
    #[test]
    fn unrelated_methods_are_unaffected_even_with_multiple_windows() {
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.spawn",
            2
        ));
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.tell",
            2
        ));
        assert!(!ambiguous_parent_fallback_requires_surface(
            "terminal.children",
            2
        ));
    }
}
