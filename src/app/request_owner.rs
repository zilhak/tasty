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

impl App {
    /// request.params 에 resource id 가 있으면 그 리소스를 가진 MainView 의 id 반환.
    ///
    /// 숫자 키(`params_resource_id`)로 못 찾으면 `terminal.spawn` 의 `"workspace"`
    /// (문자열, id 또는 표시 이름)를 마지막으로 시도한다 — 이건 u64 전용 추출
    /// 루프로는 못 뽑는다(이름일 수 있어서). `"parent"`/`"surface"` 가 같은
    /// request 에 있으면 이미 그쪽에서 찾아졌을 것이므로, 이 폴백은 사실상
    /// 순수 `terminal.spawn` 직접 호출(parent 없이 workspace 만 지정)에만 닿는다.
    ///
    /// `Err`은 workspace 이름이 2개 이상 window 에 걸쳐 모호하게 일치할 때만
    /// 반환된다(`find_main_with_workspace_target` 참고) — 호출자는 이 경우
    /// focused window 로 조용히 폴백하지 말고 명확한 에러를 클라이언트에 돌려줘야
    /// 한다.
    pub(crate) fn find_request_owner(
        &self,
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
        let Some(target) = params.get("workspace").and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        self.find_main_with_workspace_target(target)
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
}
