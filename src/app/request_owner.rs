//! IPC request 에서 대상 리소스 id 를 추출해 owner MainView 를 찾는다.
//!
//! CLAUDE.md "포커스 독립" 원칙: 모든 명령은 대상 리소스를 ID 로 직접 지정한다.
//! request.params 의 `surface_id` / `workspace_id` / `pane_id` 가 명시되면 그
//! 리소스를 가진 main 으로 routing — focus 와 무관.
//!
//! `terminal.*` 핸들러(및 이를 감싸는 `codex.spawn`/`codex.tell`/`claude.spawn`/
//! `claude.tell`)는 위 표준 키 대신 의미론적 단수 키(`surface`/`parent`/`target`/
//! `pane`)를 쓴다 — 이 키들이 없으면 이 모듈이 항상 `None` 을 반환해 요청이 항상
//! **현재 포커스된 윈도우**로 새는 라우팅 갭이 있었다(다중 메인 윈도우 세션에서
//! 대상이 실재해도 "not found" 발생). `params_resource_id` 가 그 키들도 인식하도록
//! 확장해 흡수한다 — `terminal.*` 파라미터 계약 자체는 바꾸지 않는다(이미 다수
//! 호출부가 그 키를 쓰고 있어 계약을 바꾸면 파급범위가 훨씬 크다).

use winit::window::WindowId;

use crate::app::App;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    Surface,
    Workspace,
    Pane,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResourceId {
    pub kind: Kind,
    pub id: u32,
}

/// params 에서 resource id 추출. 앞쪽 키일수록 우선(같은 request 에 여러 ID 가
/// 있을 경우 더 구체적인 대상 — surface 류가 workspace 보다 항상 먼저 온다).
///
/// `"surface"`/`"parent"`/`"target"`/`"pane"` 는 `terminal.*` 핸들러 전용 키다(위
/// 모듈 doc 참고) — 이 4개가 host IPC 전체에서 u64 값으로 다른 의미로 쓰이는
/// 곳이 없음을 확인했다(`attach.into_gui`의 `"workspace"` 는 **remote** id 라
/// 로컬 라우팅에 안 쓴다 — 여기 포함 안 시킴, 문자열 `"workspace"` 는
/// [`App::find_request_owner`] 가 별도 처리).
pub(crate) fn params_resource_id(params: &serde_json::Value) -> Option<(&str, ResourceId)> {
    for key in [
        "surface_id",
        "surface",
        "parent",
        "target",
        "pane_id",
        "pane",
        "workspace_id",
    ] {
        if let Some(v) = params.get(key).and_then(|v| v.as_u64()) {
            let kind = match key {
                "surface_id" | "surface" | "parent" | "target" => Kind::Surface,
                "pane_id" | "pane" => Kind::Pane,
                "workspace_id" => Kind::Workspace,
                _ => unreachable!(),
            };
            return Some((key, ResourceId { kind, id: v as u32 }));
        }
    }
    None
}

/// `terminal.kill`/`terminal.release`/`terminal.respawn`/`terminal.broadcast` 가
/// `--surface`(parent) 생략 시 기대는 host `single_parent()` 폴백은 **호출이 실제로
/// 라우팅된 그 window 안에서만** 유일성을 본다 — 다중 윈도우 세션에서는 애초에 어느
/// window 를 봐야 하는지가 정해지지 않는다. 이 4개 메서드가 리소스 id 없이(= 이
/// 함수 호출 시점에 `params_resource_id`/`workspace` 문자열 둘 다로 owner 를 못 찾은
/// 채) 호출됐는데 main window 가 2개 이상 열려 있으면, `find_request_owner` 가
/// focused window 로 조용히 새지 않고 명시적 `--surface` 를 요구한다(호출자가
/// 명시하지 않는 한 대상 window 를 추론할 근거가 없다). window 가 1개뿐이면 기존
/// 동작 그대로(하위 호환) — `single_parent()` 가 그 안에서 0/2+ parent 를 여전히
/// 스스로 거부한다.
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
        if let Some((_, rid)) = params_resource_id(params) {
            let found = match rid.kind {
                Kind::Surface => self.find_main_with_surface(rid.id),
                Kind::Workspace => self.find_main_with_workspace(rid.id),
                Kind::Pane => self.find_main_with_pane(rid.id),
            };
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
    use serde_json::json;

    fn rid(params: &serde_json::Value) -> (&str, Kind, u32) {
        let (key, r) = params_resource_id(params).expect("expected a resource id");
        (key, r.kind, r.id)
    }

    /// 표준 키(surface_id/pane_id/workspace_id)는 기존 그대로 인식된다(회귀 방지).
    #[test]
    fn standard_keys_still_recognized() {
        assert!(matches!(
            rid(&json!({ "surface_id": 7 })),
            ("surface_id", Kind::Surface, 7)
        ));
        assert!(matches!(
            rid(&json!({ "pane_id": 3 })),
            ("pane_id", Kind::Pane, 3)
        ));
        assert!(matches!(
            rid(&json!({ "workspace_id": 2 })),
            ("workspace_id", Kind::Workspace, 2)
        ));
    }

    /// `terminal.tell`/`terminal.state`/`terminal.parent`/`terminal.set_state` 가 쓰는
    /// `"surface"` 키(대상 child surface) — Surface 로 라우팅돼야 한다.
    #[test]
    fn terminal_surface_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "surface": 393 })),
            ("surface", Kind::Surface, 393)
        ));
    }

    /// `terminal.spawn` 이 쓰는 `"parent"` 키(부모 surface) — Surface 로 라우팅돼야
    /// 한다. codex/claude plugin 은 caller 의 `"surface"` 를 그대로 `"parent"` 로
    /// 전달하므로(TASTY_SURFACE_ID 자동 채움), 이게 실제로 가장 흔히 맞는 케이스다.
    #[test]
    fn terminal_parent_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "parent": 42, "workspace": "5" })),
            ("parent", Kind::Surface, 42)
        ));
    }

    /// `terminal.adopt` 가 쓰는 `"target"` 키(입양 대상 기존 surface) — Surface.
    #[test]
    fn terminal_target_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "surface": 1, "target": 6101 })),
            ("surface", Kind::Surface, 1)
        ));
    }

    /// `terminal.spawn` 의 선택적 `--pane` override(숫자) — Pane.
    #[test]
    fn terminal_pane_key_routes_as_pane() {
        assert!(matches!(
            rid(&json!({ "pane": 9 })),
            ("pane", Kind::Pane, 9)
        ));
    }

    /// 우선순위: surface 류(`surface`)가 workspace 보다 먼저 매칭돼야 한다 —
    /// `handle_spawn` 처럼 한 request 에 `parent`/`workspace` 가 동시에 있을 때
    /// 더 구체적인 대상(이미 존재가 보장된 parent surface)을 우선한다.
    #[test]
    fn surface_key_takes_priority_over_workspace_id() {
        assert!(matches!(
            rid(&json!({ "surface": 10, "workspace_id": 20 })),
            ("surface", Kind::Surface, 10)
        ));
    }

    /// `"child"` 는 `terminal.kill`/`terminal.release`/`terminal.respawn` 이 쓰는
    /// **부모별 index**지 surface id 가 아니다(TODO 문서에서 명시적으로 배제) —
    /// resource id 로 오인해선 안 된다.
    #[test]
    fn child_index_key_is_not_recognized() {
        assert!(params_resource_id(&json!({ "child": 5 })).is_none());
    }

    /// `attach.into_gui` 의 `"workspace"` 는 **remote** id(u64) 다 — 로컬 workspace
    /// 로 오인해 라우팅하면 안 되므로 `params_resource_id` 가 인식하지 않아야
    /// 한다(문자열 `"workspace"` 만 `find_request_owner` 가 별도로 다룬다).
    #[test]
    fn numeric_workspace_key_is_not_recognized_as_local_resource() {
        assert!(params_resource_id(&json!({ "workspace": 5u64, "port": 1234 })).is_none());
    }

    /// `debug_plugin`의 `"target"`(문자열)처럼 다른 타입의 동명 키는 무시된다
    /// (`.as_u64()` 가 실패해 다음 키로 넘어가거나 최종 `None`).
    #[test]
    fn string_typed_target_is_ignored() {
        assert!(params_resource_id(&json!({ "target": "some-string" })).is_none());
    }

    /// 아무 인식 키도 없으면 `None`.
    #[test]
    fn no_recognized_key_returns_none() {
        assert!(params_resource_id(&json!({ "text": "hello" })).is_none());
    }

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
