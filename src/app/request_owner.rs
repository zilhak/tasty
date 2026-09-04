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

impl App {
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
