//! 요청의 대상 ID와 engine의 보유 여부를 GUI·헤드리스가 같은 규칙으로 판단한다.
//! 여러 창을 순회해 소유 창을 찾는 일은 app::request_owner가 맡는다.

#[derive(Debug, Clone, Copy)]
pub(crate) enum Kind {
    Surface,
    Workspace,
    Pane,
    /// 탭을 가진 pane을 통해 engine을 찾는다.
    Tab,
    /// surface 없는 PTY도 engine별 pty_registry에서 찾아야 한다.
    HeadlessPty,
    /// surface hook과 global hook은 같은 키 이름을 써도 저장소가 달라 메서드로 구별한다.
    Hook,
    /// 이름과 달리 engine별 global_hook_manager에 저장된다.
    GlobalHook,
    Observer,
    /// 기본 normal ID 0은 모든 engine에 있어 이 ID만으로 창을 고를 수 없다.
    Category,
}

impl Kind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Surface => "surface",
            Kind::Workspace => "workspace",
            Kind::Pane => "pane",
            Kind::Tab => "tab",
            Kind::HeadlessPty => "headless pty",
            Kind::Hook => "surface hook",
            Kind::GlobalHook => "global hook",
            Kind::Observer => "output observer",
            Kind::Category => "workspace category",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResourceId {
    pub kind: Kind,
    /// hook·observer의 u64 ID를 보존한다. u32 종류는 조회할 때 범위를 검사한다.
    pub id: u64,
}

/// 앞쪽 키부터 부호 없는 숫자 ID를 찾는다. 잘못된 타입은 건너뛰며 요청 전체의 유효성을 검증하지는 않는다.
/// 숫자 workspace 키는 원격 attach 대상일 수 있어 여기에 넣지 않는다.
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
            return Some((key, ResourceId { kind, id: v }));
        }
    }
    None
}

/// id·hook_id처럼 뜻이 겹치는 키는 메서드로 구별한다.
/// pty.attach_surface는 pane_id로 찾고, pty.spawn은 새 리소스를 만들므로 여기서 대상 ID를 찾지 않는다.
pub(crate) fn method_scoped_resource_id(
    method: &str,
    params: &serde_json::Value,
) -> Option<ResourceId> {
    if method == "file_handler.dispatch" {
        return numeric(params, "origin_surface_id").map(|id| ResourceId {
            kind: Kind::Surface,
            id,
        });
    }
    if method == "hook.unset" {
        return numeric(params, "hook_id").map(|id| ResourceId {
            kind: Kind::Hook,
            id,
        });
    }
    if method == "global_hook.unset" {
        return numeric(params, "hook_id").map(|id| ResourceId {
            kind: Kind::GlobalHook,
            id,
        });
    }
    if matches!(method, "output.observe_stop" | "output.observe_info") {
        return numeric(params, "observer_id").map(|id| ResourceId {
            kind: Kind::Observer,
            id,
        });
    }
    if method == "preset.capture" {
        let kind = match params.get("kind").and_then(|v| v.as_str()) {
            Some("workspace") => Kind::Workspace,
            Some("tab") => Kind::Tab,
            Some("pane") => Kind::Pane,
            _ => return None,
        };
        return numeric(params, "source_id").map(|id| ResourceId { kind, id });
    }
    // normal은 모든 engine에 같은 ID로 있어 소유 창을 정할 수 없다. 변경 거절은 핸들러가 처리한다.
    if matches!(
        method,
        "workspace_category.rename" | "workspace_category.delete" | "workspace_category.move"
    ) {
        return numeric(params, "id")
            .filter(|id| *id != u64::from(crate::model::NORMAL_CATEGORY_ID))
            .map(|id| ResourceId {
                kind: Kind::Category,
                id,
            });
    }
    if matches!(
        method,
        "workspace.close" | "workspace.update" | "workspace.move"
    ) {
        return numeric(params, "id").map(|id| ResourceId {
            kind: Kind::Workspace,
            id,
        });
    }
    if method == "split" {
        if let Some(id) = numeric(params, "target_pane") {
            return Some(ResourceId {
                kind: Kind::Pane,
                id,
            });
        }
        // CLI는 nickname도 받는 인자를 문자열로 보낸다. 숫자가 아닌 이름 조회는 이 함수 밖에서 한다.
        return numeric_or_numeric_string(params, "target_surface").map(|id| ResourceId {
            kind: Kind::Surface,
            id,
        });
    }
    if matches!(method, "pty.write" | "pty.read" | "pty.wait" | "pty.kill") {
        return numeric(params, "id").map(|id| ResourceId {
            kind: Kind::HeadlessPty,
            id,
        });
    }
    None
}

fn numeric(params: &serde_json::Value, key: &str) -> Option<u64> {
    params.get(key).and_then(|v| v.as_u64())
}

/// 숫자는 u64, 숫자 문자열은 u32로 읽는다. ID를 자르지 않으며 nickname 조회는 여기서 하지 않는다.
fn numeric_or_numeric_string(params: &serde_json::Value, key: &str) -> Option<u64> {
    let v = params.get(key)?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    v.as_str()?.parse::<u32>().ok().map(u64::from)
}

/// engine별 자원 보유 판정. u32 종류의 범위를 넘는 ID는 다른 ID로 잘라 조회하지 않는다.
pub(crate) fn engine_has_resource(engine: &crate::core::CoreState, rid: ResourceId) -> bool {
    let narrow = u32::try_from(rid.id).ok();
    match rid.kind {
        Kind::Surface => narrow.is_some_and(|id| engine.has_surface(id)),
        Kind::Workspace => narrow.is_some_and(|id| engine.has_workspace(id)),
        Kind::Pane => narrow.is_some_and(|id| engine.has_pane(id)),
        Kind::Tab => narrow.is_some_and(|id| engine.find_pane_for_tab(id).is_some()),
        Kind::HeadlessPty => narrow.is_some_and(|id| engine.pty_registry.contains(id)),
        Kind::Hook => engine
            .hook_manager
            .list_hooks(None)
            .iter()
            .any(|h| h.id == rid.id),
        Kind::GlobalHook => narrow.is_some_and(|id| engine.global_hook_manager.get(id).is_some()),
        Kind::Observer => engine.observer_router.info(rid.id).is_some(),
        Kind::Category => narrow.is_some_and(|id| engine.category_index(id).is_some()),
    }
}

/// 일반 키를 먼저 찾고 없으면 메서드별 키를 찾는다. file_handler.dispatch는 명시된 origin 키만 사용한다.
pub(crate) fn request_resource_id(method: &str, params: &serde_json::Value) -> Option<ResourceId> {
    // 다른 부가 키가 파일 열기의 origin 대상을 바꾸지 않게 한다.
    if method == "file_handler.dispatch" {
        return method_scoped_resource_id(method, params);
    }
    params_resource_id(params)
        .map(|(_, rid)| rid)
        .or_else(|| method_scoped_resource_id(method, params))
}

/// 헤드리스에서 소유 검사를 먼저 적용할 호스트 예약 prefix인지 확인한다.
/// plugin 매니페스트 검증과 같은 예약 목록을 사용한다. 점이 없으면 메서드 이름 전체로 확인한다.
#[cfg(not(feature = "gui"))]
pub(crate) fn prefix_is_host_reserved(method: &str) -> bool {
    let prefix = method.split('.').next().unwrap_or(method);
    tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES.contains(&prefix)
}

/// 본체와 plugin이 같은 대상 없음 문구를 쓰도록 utils의 생성기에 종류·ID를 넘긴다.
pub(crate) fn unowned_target_message(rid: ResourceId, method: &str) -> String {
    tasty_utils::target::unowned_target_message(rid.kind.label(), rid.id, method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn file_dispatch_origin_is_its_only_method_scoped_target() {
        let params = json!({ "origin_surface_id": 41, "surface_id": 99 });
        let got = request_resource_id("file_handler.dispatch", &params).unwrap();
        assert!(matches!(got.kind, Kind::Surface));
        assert_eq!(got.id, 41);
        assert!(method_scoped_resource_id("surface.close", &params).is_none());
        assert!(request_resource_id("file_handler.dispatch", &json!({"surface_id":99})).is_none());
        assert!(
            request_resource_id("file_handler.dispatch", &json!({"origin_surface_id":null}))
                .is_none()
        );
    }

    fn rid(params: &serde_json::Value) -> (&str, Kind, u64) {
        let (key, r) = params_resource_id(params).expect("expected a resource id");
        (key, r.kind, r.id)
    }

    #[test]
    fn split_names_its_target_and_routing_reads_it() {
        let by =
            |p: &serde_json::Value| method_scoped_resource_id("split", p).map(|r| (r.kind, r.id));
        assert!(matches!(
            by(&json!({ "level": "pane", "target_pane": 3 })),
            Some((Kind::Pane, 3))
        ));
        assert!(matches!(
            by(&json!({ "level": "surface", "target_surface": 7 })),
            Some((Kind::Surface, 7))
        ));
        assert!(
            matches!(
                by(&json!({ "level": "surface", "target_surface": "7" })),
                Some((Kind::Surface, 7))
            ),
            "CLI의 숫자 문자열도 surface ID로 읽어야 한다"
        );
        assert!(by(&json!({ "target_surface": "faraway" })).is_none());
        assert!(by(&json!({ "level": "surface" })).is_none());
        // 범위를 넘는 문자열 ID를 잘라 실제 다른 대상으로 바꾸면 안 된다.
        assert!(
            by(&json!({ "target_surface": (u64::from(u32::MAX) + 2).to_string() })).is_none(),
            "32 비트를 넘는 값을 잘라서 다른 surface 로 만들었다"
        );
    }

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

    #[test]
    fn terminal_surface_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "surface": 393 })),
            ("surface", Kind::Surface, 393)
        ));
    }

    #[test]
    fn terminal_parent_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "parent": 42, "workspace": "5" })),
            ("parent", Kind::Surface, 42)
        ));
    }

    #[test]
    fn category_id_routes_only_for_category_methods_and_never_for_normal() {
        let p = json!({ "id": 4, "name": "x" });
        for m in [
            "workspace_category.rename",
            "workspace_category.delete",
            "workspace_category.move",
        ] {
            let rid = method_scoped_resource_id(m, &p).expect("대상이 잡혀야 한다");
            assert!(matches!(rid.kind, Kind::Category), "{m}");
            assert_eq!(rid.id, 4);
        }
        for m in [
            "workspace_category.rename",
            "workspace_category.delete",
            "workspace_category.move",
        ] {
            assert!(
                method_scoped_resource_id(m, &json!({ "id": 0, "name": "x" })).is_none(),
                "{m}에서 모든 창이 공유하는 normal ID로 소유 창을 고르면 안 된다"
            );
        }
        assert!(method_scoped_resource_id("workspace_category.create", &p).is_none());
        assert!(method_scoped_resource_id("memory.get", &p).is_none());
    }

    #[test]
    fn workspace_id_key_routes_only_for_the_methods_that_mean_workspace() {
        let p = json!({ "id": 4, "name": "x" });
        for m in ["workspace.close", "workspace.update", "workspace.move"] {
            let rid = method_scoped_resource_id(m, &p).expect("대상이 잡혀야 한다");
            assert!(matches!(rid.kind, Kind::Workspace), "{m}");
            assert_eq!(rid.id, 4);
        }
        assert!(method_scoped_resource_id("memory.get", &p).is_none());
        assert!(method_scoped_resource_id("approval.cancel", &p).is_none());
        assert!(
            method_scoped_resource_id("workspace.close", &json!({ "index": 0 })).is_none(),
            "index 는 창을 건너 해석되면 안 된다"
        );
        assert!(
            method_scoped_resource_id("workspace.move", &json!({ "from_index": 2, "to_index": 0 }))
                .is_none(),
            "from_index만으로 소유 창을 정하면 안 된다"
        );
    }

    #[test]
    fn tab_close_routes_by_tab_id() {
        assert!(matches!(
            rid(&json!({ "tab_id": 9 })),
            ("tab_id", Kind::Tab, 9)
        ));
    }

    #[test]
    fn message_send_routes_by_the_recipient() {
        assert!(matches!(
            rid(&json!({ "to_surface_id": 5, "from_surface_id": 7 })),
            ("to_surface_id", Kind::Surface, 5)
        ));
    }

    /// 발신 surface는 메타데이터다. 메시지 큐가 있는 수신 surface로 라우팅해야 한다.
    #[test]
    fn the_sender_id_alone_does_not_pick_a_window() {
        assert!(params_resource_id(&json!({ "from_surface_id": 7 })).is_none());
    }

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

    #[test]
    fn a_pty_id_is_a_target_only_for_the_methods_that_take_one() {
        let params = json!({ "id": 0x8000_0001u32 });
        let got = method_scoped_resource_id("pty.wait", &params).expect("pty.wait 는 대상이 있다");
        assert!(matches!(got.kind, Kind::HeadlessPty));
        assert_eq!(got.id, 0x8000_0001);

        assert!(method_scoped_resource_id("hook.unset", &params).is_none());
        assert!(params_resource_id(&params).is_none());
    }

    /// attach_surface는 PTY의 id가 아니라 목적지 pane_id로 engine을 찾는다.
    #[test]
    fn attach_surface_is_resolved_by_its_pane_not_its_pty() {
        let params = json!({ "id": 0x8000_0002u32, "pane_id": 3 });
        assert!(matches!(rid(&params), ("pane_id", Kind::Pane, 3)));
        assert!(method_scoped_resource_id("pty.attach_surface", &params).is_none());
    }

    #[test]
    fn spawn_is_not_a_target_because_it_creates_one() {
        let params = json!({ "id": 0x8000_0002u32 });
        assert!(method_scoped_resource_id("pty.spawn", &params).is_none());
    }

    /// 대상 해석뿐 아니라 해당 종류를 engine에서 찾는 함수도 확인한다.
    #[test]
    fn an_engine_reports_the_global_hook_it_owns() {
        use crate::host_api::hooks::global::HookCondition;
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, waker).expect("engine");
        let id = engine.global_hook_manager.add(
            HookCondition::Interval(std::time::Duration::from_secs(60)),
            "echo x".into(),
            None,
        );
        assert!(engine_has_resource(
            &engine,
            ResourceId {
                kind: Kind::GlobalHook,
                id: u64::from(id),
            }
        ));
        assert!(
            !engine_has_resource(
                &engine,
                ResourceId {
                    kind: Kind::GlobalHook,
                    id: u64::from(id) + 1,
                }
            ),
            "없는 id 를 가졌다고 답하면 라우팅이 아무 창이나 고른다"
        );
    }

    #[test]
    fn both_hook_surfaces_are_targets_but_of_different_kinds() {
        let params = json!({ "hook_id": 12u64 });
        let g = method_scoped_resource_id("global_hook.unset", &params)
            .expect("global hook 도 대상이다");
        assert!(matches!(g.kind, Kind::GlobalHook));
        assert_eq!(g.id, 12);

        let got =
            method_scoped_resource_id("hook.unset", &params).expect("surface hook 은 대상이다");
        assert!(matches!(got.kind, Kind::Hook));
        assert_eq!(got.id, 12);

        assert!(params_resource_id(&params).is_none());
        assert!(params_resource_id(&params).is_none());
    }

    #[test]
    fn an_observer_is_a_target_only_once_it_exists() {
        let params = json!({ "observer_id": 3u64 });
        for method in ["output.observe_stop", "output.observe_info"] {
            let got = method_scoped_resource_id(method, &params)
                .unwrap_or_else(|| panic!("{method} 는 대상이 있다"));
            assert!(matches!(got.kind, Kind::Observer));
        }
        assert!(method_scoped_resource_id("output.observe_start", &params).is_none());
    }

    #[test]
    fn preset_capture_reads_its_source_kind_from_the_request() {
        let cases = [
            ("workspace", Kind::Workspace),
            ("tab", Kind::Tab),
            ("pane", Kind::Pane),
        ];
        for (kind_str, want) in cases {
            let params = json!({ "kind": kind_str, "source_id": 11u64 });
            let got = method_scoped_resource_id("preset.capture", &params)
                .unwrap_or_else(|| panic!("kind={kind_str} 는 대상이 있다"));
            assert!(
                std::mem::discriminant(&got.kind) == std::mem::discriminant(&want),
                "kind={kind_str} 가 {:?} 로 풀렸다",
                got.kind
            );
        }
        let unknown = json!({ "kind": "galaxy", "source_id": 11u64 });
        assert!(method_scoped_resource_id("preset.capture", &unknown).is_none());
    }

    #[test]
    fn an_id_too_large_for_a_window_resource_has_no_owner() {
        let big = json!({ "surface_id": u64::from(u32::MAX) + 1 });
        let (_, rid) = params_resource_id(&big).expect("키는 인식된다");
        assert!(matches!(rid.kind, Kind::Surface));
        assert!(u32::try_from(rid.id).is_err(), "좁히기가 실패해야 한다");
    }

    #[test]
    fn terminal_target_key_routes_as_surface() {
        assert!(matches!(
            rid(&json!({ "surface": 1, "target": 6101 })),
            ("surface", Kind::Surface, 1)
        ));
    }

    #[test]
    fn terminal_pane_key_routes_as_pane() {
        assert!(matches!(
            rid(&json!({ "pane": 9 })),
            ("pane", Kind::Pane, 9)
        ));
    }

    #[test]
    fn surface_key_takes_priority_over_workspace_id() {
        assert!(matches!(
            rid(&json!({ "surface": 10, "workspace_id": 20 })),
            ("surface", Kind::Surface, 10)
        ));
    }

    #[test]
    fn child_index_key_is_not_recognized() {
        assert!(params_resource_id(&json!({ "child": 5 })).is_none());
    }

    #[test]
    fn numeric_workspace_key_is_not_recognized_as_local_resource() {
        assert!(params_resource_id(&json!({ "workspace": 5u64, "port": 1234 })).is_none());
    }

    #[test]
    fn string_typed_target_is_ignored() {
        assert!(params_resource_id(&json!({ "target": "some-string" })).is_none());
    }

    #[test]
    fn no_recognized_key_returns_none() {
        assert!(params_resource_id(&json!({ "text": "hello" })).is_none());
    }
}

#[cfg(all(test, not(feature = "gui")))]
mod headless_prefix_tests {
    use super::prefix_is_host_reserved;

    #[test]
    fn a_reserved_prefix_is_host_only_and_an_unreserved_one_is_not() {
        assert!(prefix_is_host_reserved("workspace.create"));
        assert!(prefix_is_host_reserved("surface.close"));
        assert!(prefix_is_host_reserved("split"));
        assert!(prefix_is_host_reserved("tree"));
        assert!(!prefix_is_host_reserved("image.open"));
        assert!(!prefix_is_host_reserved("markdown.recent"));
        assert!(!prefix_is_host_reserved("codex.spawn"));
    }
}
