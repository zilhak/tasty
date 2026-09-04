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
    /// 탭은 창에 직접 매이지 않고 pane 을 통해 매인다 — `CoreState::find_pane_for_tab`
    /// 이 그 해석을 한다. `tab.close` 만 tab id 를 단독 대상으로 받는다(`tab.list` ·
    /// `tab.create` · `tab.move` 는 `pane_id` 를 함께 받아 그쪽으로 풀린다).
    Tab,
    /// headless pty(`PTY_ID_BASE` 이상). 창이 아니라 **engine 의 `pty_registry`** 에
    /// 산다 — 창마다 engine 이 따로이므로 창을 건너 찾아야 한다.
    HeadlessPty,
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
        "to_surface_id",
        "tab_id",
        "pane_id",
        "pane",
        "target_pane_id",
        "workspace_id",
        "target_workspace_id",
    ] {
        if let Some(v) = params.get(key).and_then(|v| v.as_u64()) {
            let kind = match key {
                "surface_id" | "surface" | "parent" | "target" | "to_surface_id" => Kind::Surface,
                "tab_id" => Kind::Tab,
                "pane_id" | "pane" | "target_pane_id" => Kind::Pane,
                "workspace_id" | "target_workspace_id" => Kind::Workspace,
                _ => unreachable!(),
            };
            return Some((key, ResourceId { kind, id: v as u32 }));
        }
    }
    None
}

/// `pty.*` 의 `"id"` 는 headless pty id 지만, 이 키는 위 목록에 넣을 수 **없다** —
/// `"id"` 는 host 핸들러 전체에서 25 곳이 각기 다른 의미로 쓰는 범용 키라(hook id ·
/// agent id · plugin id …) 무조건 pty 로 해석하면 오탐이 쏟아진다. 그래서 여기만
/// **메서드 이름으로 한정**한다.
///
/// `pty.attach_surface` 는 제외한다 — `"id"`(pty)와 함께 `"pane_id"` 를 받으므로
/// 위 목록이 이미 그쪽으로 푼다. `pty.spawn` 도 제외다: 대상이 아니라 **생성**이라
/// 실을 id 자체가 없다(그래서 지금도 포커스된 창의 engine 에 생긴다 — 이 함수가 고칠
/// 수 있는 형태가 아니다).
pub(crate) fn method_scoped_resource_id(
    method: &str,
    params: &serde_json::Value,
) -> Option<ResourceId> {
    if !matches!(method, "pty.write" | "pty.read" | "pty.wait" | "pty.kill") {
        return None;
    }
    params
        .get("id")
        .and_then(|v| v.as_u64())
        .map(|v| ResourceId {
            kind: Kind::HeadlessPty,
            id: v as u32,
        })
}

/// 이 engine 이 그 리소스를 갖고 있는가 — parked engine 탐색의 술어.
///
/// kind 분기를 여기 한 곳에만 둔다. 같은 `match` 가 세 곳에 복제돼 있었고
/// (`ipc/routing.rs` · `dispatch/intents.rs` · 그리고 창 순회 쪽), 복제된 분기는
/// 한쪽만 새 kind 를 알게 되는 순간 **그 리소스가 창에서도 parked 에서도 안 잡혀
/// 포커스 폴백으로 새는** 형태를 만든다. 창 쪽 대응물은
/// [`App::find_main_with_resource`](crate::app::App::find_main_with_resource) 다.
pub(crate) fn engine_has_resource(engine: &crate::core::CoreState, rid: ResourceId) -> bool {
    match rid.kind {
        Kind::Surface => engine.has_surface(rid.id),
        Kind::Workspace => engine.has_workspace(rid.id),
        Kind::Pane => engine.has_pane(rid.id),
        Kind::Tab => engine.find_pane_for_tab(rid.id).is_some(),
        Kind::HeadlessPty => engine.pty_registry.contains(rid.id),
    }
}

/// 이 요청이 겨누는 리소스 — 키 이름으로 뽑히는 것과 메서드로 한정되는 것을 합친다.
pub(crate) fn request_resource_id(method: &str, params: &serde_json::Value) -> Option<ResourceId> {
    params_resource_id(params)
        .map(|(_, rid)| rid)
        .or_else(|| method_scoped_resource_id(method, params))
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

    /// `tab.close` 는 tab id 만 실어 온다 — 그 탭을 담은 pane 을 가진 창이 주인이다.
    ///
    /// 고치기 전에는 이 키가 목록에 없어 `None` 이었고, 그러면 요청이 **포커스된 창**
    /// 으로 갔다. 창이 둘일 때 첫 창의 탭을 겨눈 `tab.close` 가 두 번째 창의 engine 에
    /// 도착해 `closed:false` 를 돌려주는 형태로 관측됐다.
    #[test]
    fn tab_close_routes_by_tab_id() {
        assert!(matches!(
            rid(&json!({ "tab_id": 9 })),
            ("tab_id", Kind::Tab, 9)
        ));
    }

    /// `message.send` 는 **받는 쪽**으로 라우팅한다.
    ///
    /// 큐가 `to` 에 매여 있기 때문이다 — `CoreState::send_message` 가
    /// `surface_messages.entry(to)` 에 넣고, `message.read` 는 `surface_id`(= 받는 쪽)로
    /// 읽는다. 보내는 쪽 engine 에 넣으면 읽는 쪽이 영원히 못 본다.
    #[test]
    fn message_send_routes_by_the_recipient() {
        assert!(matches!(
            rid(&json!({ "to_surface_id": 5, "from_surface_id": 7 })),
            ("to_surface_id", Kind::Surface, 5)
        ));
    }

    /// `from_surface_id` 는 **라우팅 키가 아니다** — 보낸 사람을 적는 메타데이터다.
    ///
    /// 이 단언이 위 테스트의 대조다. 둘 다 인식하게 만들면 키 순서에 따라 어느 쪽이
    /// 이기는지가 정해지고, 보내는 쪽이 이기는 순간 메시지가 받는 쪽이 안 보는
    /// engine 에 쌓인다. "id 처럼 생겼으니 넣는다" 가 왜 틀린지의 실례다.
    #[test]
    fn the_sender_id_alone_does_not_pick_a_window() {
        assert!(params_resource_id(&json!({ "from_surface_id": 7 })).is_none());
    }

    /// `preset.apply` 의 명시적 대상.
    #[test]
    fn preset_apply_routes_by_its_explicit_target() {
        assert!(matches!(
            rid(&json!({ "target_pane_id": 4 })),
            ("target_pane_id", Kind::Pane, 4)
        ));
        assert!(matches!(
            rid(&json!({ "target_workspace_id": 2 })),
            ("target_workspace_id", Kind::Workspace, 2)
        ));
    }

    /// headless pty 의 `"id"` 는 **그 키를 그 뜻으로 쓰는 메서드에서만** 대상이다.
    ///
    /// `"id"` 는 host 핸들러 전반이 각기 다른 의미로 쓰는 범용 키다. 키 목록에 넣으면
    /// `hook.unset {id}` 같은 요청이 pty 로 해석돼 엉뚱한 창으로 간다 — 아래 두 번째
    /// 단언이 그 오탐을 막는 대조다.
    #[test]
    fn a_pty_id_is_a_target_only_for_the_methods_that_take_one() {
        let params = json!({ "id": 0x8000_0001u32 });
        let got = method_scoped_resource_id("pty.wait", &params).expect("pty.wait 는 대상이 있다");
        assert!(matches!(got.kind, Kind::HeadlessPty));
        assert_eq!(got.id, 0x8000_0001);

        assert!(method_scoped_resource_id("hook.unset", &params).is_none());
        assert!(params_resource_id(&params).is_none());
    }

    /// `pty.attach_surface` 는 이 한정 목록에 **없다** — `pane_id` 로 이미 풀리기 때문이다.
    ///
    /// 이것이 없으면 "pty 를 받는 메서드는 전부 넣어야 한다" 로 읽혀 목록이 넓어진다.
    /// 넓히면 틀리지는 않지만, 어느 키가 실제로 주인을 정하는지가 흐려진다.
    #[test]
    fn attach_surface_is_resolved_by_its_pane_not_its_pty() {
        let params = json!({ "id": 0x8000_0002u32, "pane_id": 3 });
        assert!(matches!(rid(&params), ("pane_id", Kind::Pane, 3)));
        assert!(method_scoped_resource_id("pty.attach_surface", &params).is_none());
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
