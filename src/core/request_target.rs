//! 요청이 **무엇을 대상으로 지목했는가** — `App` 없이 판정하는 순수 부분.
//!
//! gui 는 창이 여럿이라 이 판정으로 주인 창을 고르고, 헤드리스는 engine 이 하나라
//! 고를 것이 없다. 그래도 두 조합이 **같은 판정**을 써야 한다: 지목한 대상을 아무도
//! 안 가졌을 때 한쪽만 거절하면 같은 요청이 조합에 따라 다르게 끝난다.
//! 그래서 `App`/`winit` 에 안 매인 부분을 여기 두고 양쪽이 함께 쓴다.
//!
//! 창을 순회해 주인을 찾는 부분은 `app/request_owner.rs`(gui 전용)에 남는다.

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
    /// surface hook — `engine.hook_manager` 소유다. `global_hook.*` 의 훅은 다른 저장소
    /// (`global_hook_manager`)에 있어 [`Kind::GlobalHook`] 으로 따로 푼다. 같은 `hook_id`
    /// 키를 쓰므로 키가 아니라 **메서드로** 한정한다.
    Hook,
    /// global hook — `engine.global_hook_manager` 소유.
    ///
    /// **이름과 달리 창에 매인다.** 그 매니저는 `CoreState` 의 필드라 engine(창)마다
    /// 하나다. 그래서 다른 창의 global hook 은 그 창의 engine 에서만 찾을 수 있고, 이
    /// kind 가 없던 동안에는 요청이 포커스된 창으로 새서 **존재하는데 어떤 요청으로도
    /// 닿지 않는** 훅이 남았다(실측: `unset global-hook --hook 1` 이 두 번째 호출에서
    /// `removed: false`).
    GlobalHook,
    /// output observer — `engine.observer_router` 소유.
    Observer,
    /// workspace category — `engine.categories` 소유. 예약된 `normal`(id 0)은 **모든
    /// engine 에 상수로 존재**하므로 이 kind 로 풀지 않는다(어느 창인지 정해지지 않는다).
    /// 그래도 되는 이유: normal 은 rename · delete · move 가 전부 거부하는 항목이라
    /// 애초에 어떤 요청의 대상이 아니다.
    Category,
}

impl Kind {
    /// 에러 메시지에 쓰는 이름. 호출자가 "무엇의 id 였는지" 를 알아야 고칠 수 있다.
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
    /// hook id 와 observer id 가 `u64` 라 여기서 좁히지 않는다. 창에 매인 리소스
    /// (surface/pane/workspace/tab)는 `u32` 라 해석 시점에 `try_from` 으로 좁히고,
    /// 안 들어가면 그런 리소스가 아니므로 주인이 없다.
    pub id: u64,
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
            return Some((key, ResourceId { kind, id: v }));
        }
    }
    None
}

/// `pty.*` 의 `"id"` 는 headless pty id 지만, 이 키는 위 목록에 넣을 수 **없다** —
/// `"id"` 는 host 핸들러 전반이 **각기 다른 의미로** 쓰는 범용 키라(hook id ·
/// agent id · plugin id …) 무조건 pty 로 해석하면 오탐이 쏟아진다. 그래서 여기만
/// **메서드 이름으로 한정**한다.
///
/// 몇 곳인지는 적지 않는다 — 그 수는 핸들러가 하나 늘 때마다 낡고, 낡았는지 확인
/// 하려면 저장소를 훑어야 해서 아무도 확인하지 않는다. 필요하면 재라:
/// `grep -rc '"id"' src/adapters/ipc/handler/`.
///
/// `pty.attach_surface` 는 제외한다 — `"id"`(pty)와 함께 `"pane_id"` 를 받으므로
/// 위 목록이 이미 그쪽으로 푼다. `pty.spawn` 도 제외다: 대상이 아니라 **생성**이라
/// 실을 id 자체가 없다(그래서 지금도 포커스된 창의 engine 에 생긴다 — 이 함수가 고칠
/// 수 있는 형태가 아니다).
pub(crate) fn method_scoped_resource_id(
    method: &str,
    params: &serde_json::Value,
) -> Option<ResourceId> {
    // surface hook 은 `engine.hook_manager` 에 있고 global hook 은 `global_hook_manager`
    // 에 있다. 두 표면이 `hook_id` 라는 **같은 키**를 쓰므로 키로는 못 가른다 — 저장소가
    // 다를 뿐 둘 다 engine 소유라, 메서드로 갈라 각각의 kind 로 푼다.
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
    // observer 를 **만드는** `output.observe_start` 는 대상 id 가 없다(그래서 지금도
    // 포커스된 창의 engine 에 등록된다 — `pty.spawn` 과 같은 형태다). 나머지 둘만 대상이 있다.
    if matches!(method, "output.observe_stop" | "output.observe_info") {
        return numeric(params, "observer_id").map(|id| ResourceId {
            kind: Kind::Observer,
            id,
        });
    }
    // `preset.capture` 의 `source_id` 는 `kind` 가 무엇을 가리키는지 정한다.
    if method == "preset.capture" {
        let kind = match params.get("kind").and_then(|v| v.as_str()) {
            Some("workspace") => Kind::Workspace,
            Some("tab") => Kind::Tab,
            Some("pane") => Kind::Pane,
            _ => return None,
        };
        return numeric(params, "source_id").map(|id| ResourceId { kind, id });
    }
    // `workspace.close` / `workspace.update` 의 `"id"` 는 workspace id 다. workspace id 는
    // engine 을 건너 유일하므로(`CoreState::has_workspace` 가 보는 그 값) 주인을 정확히
    // 짚을 수 있는데, 종전에는 이 키가 인식 밖이라 요청이 **포커스된 창**으로 갔다 —
    // 실측(2026-09-05): 비포커스 창이 가진 workspace 1 을 지목한 `workspace.update` 가
    // "Workspace 1 not found" 로 실패했다. 같은 요청이 사용자가 어디를 클릭했느냐에 따라
    // 성공하기도 실패하기도 한다(`docs/design/policies/focus.md` 의 활성 상태 의존 금지).
    //
    // `"id"` 가 없는 형태(`index` 로 지목)는 여기서 `None` 이 되어 종전 경로를 탄다 —
    // index 는 창 안에서의 위치라 창을 건너 해석할 수 있는 값이 아니다.
    // `workspace_category.rename` / `.delete` 의 `"id"` 는 카테고리 id 다. 카테고리 id 는
    // 공유 카운터(`IdGenerator.category`)가 1 부터 발급해 engine 을 건너 유일하다 —
    // 실측(2026-09-05, 창 둘): 비포커스 창이 가진 카테고리를 지목한 rename 이
    // "category not found" 로 실패했고 포커스를 옮기면 성공했다.
    //
    // **`normal`(id 0)은 뺀다.** 그것만은 모든 engine 에 상수로 있어 id 로 창이
    // 정해지지 않는다. 빼도 잃는 것이 없다 — normal 은 rename·delete 가 `IsNormal` 로
    // 거부하는 항목이라 대상이 될 일이 없고, 여기서 `None` 이 되면 종전 경로가 그
    // 거절 문구를 그대로 낸다.
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
    // `split` 은 대상을 **반드시** 지목한다 — `target_surface` 와 `target_pane` 중 하나가
    // 필수이고 둘 다 주면 핸들러가 거절한다. 그런데 그 두 키가 인식 밖이라 모든 split 이
    // 포커스된 창으로 갔다. 실측(2026-09-05, 창 둘 · 두 surface 다 살아 있는 상태):
    //
    //     포커스 B: split{target_surface:1} → "surface 1 not found"   split{target_surface:2} → 성공
    //     포커스 A: split{target_surface:1} → 성공                     split{target_surface:2} → "surface 2 not found"
    //
    // 같은 요청이 **사용자가 어디를 클릭했느냐**에 따라 성공하기도 실패하기도 했다
    // (`docs/design/policies/focus.md` 의 활성 상태 의존 금지). 바로 앞의
    // `workspace.close`/`update`/`move` 갈래가 같은 이유로 생겼다 — 그 갈래를
    // 이름으로 가리키는 이유는 "아래" 같은 방향이 코드가 움직이면 조용히 틀려서다.
    //
    // 채널: 그 갈래가 **살아 있다**는 것은
    // `workspace_id_key_routes_only_for_the_methods_that_mean_workspace` 가 지킨다(세
    // 메서드를 이름으로 순회한다). 갈래가 지워지면 그 시험이 죽으므로 이 참조는 매달리지
    // 않는다. 반면 "같은 이유로 생겼다" 는 **연혁**이라 그것을 재는 채널은 없다 — 없다고
    // 적어 두는 편이, 지킨다고 읽히는 단정을 하나 더 두는 것보다 낫다.
    if method == "split" {
        if let Some(id) = numeric(params, "target_pane") {
            return Some(ResourceId {
                kind: Kind::Pane,
                id,
            });
        }
        // CLI 는 이 값을 **문자열로** 보낸다(같은 인자가 nickname 도 받아서 한 타입으로
        // 고정돼 있다) — 숫자만 보면 CLI 경로가 통째로 안 풀린다. 숫자로 안 읽히는
        // 값은 nickname 이고, 그 해석은 memory store 를 봐야 해서 이 순수 함수 밖이다
        // (그 경우는 여기서 `None` 이 되어 종전 경로를 탄다).
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

/// 숫자, 또는 **숫자로 읽히는 문자열**. 핸들러가 같은 순서로 읽는 자리에만 쓴다
/// (`pane::resolve_surface_target` 도 문자열을 먼저 `parse::<u32>()` 하고 실패하면
/// nickname 으로 넘어간다) — 라우팅이 핸들러보다 더 관대하면 라우팅만 성공하는
/// 값이 생긴다.
fn numeric_or_numeric_string(params: &serde_json::Value, key: &str) -> Option<u64> {
    let v = params.get(key)?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    v.as_str()?.parse::<u32>().ok().map(u64::from)
}

/// 이 engine 이 그 리소스를 갖고 있는가 — parked engine 탐색의 술어.
///
/// kind 분기를 여기 한 곳에만 둔다. 같은 `match` 가 세 곳에 복제돼 있었고
/// (`ipc/routing.rs` · `dispatch/intents.rs` · 그리고 창 순회 쪽), 복제된 분기는
/// 한쪽만 새 kind 를 알게 되는 순간 **그 리소스가 창에서도 parked 에서도 안 잡혀
/// 포커스 폴백으로 새는** 형태를 만든다. 창 쪽 대응물은
/// [`App::find_main_with_resource`](crate::app::App::find_main_with_resource) 다.
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

/// 이 요청이 겨누는 리소스 — 키 이름으로 뽑히는 것과 메서드로 한정되는 것을 합친다.
pub(crate) fn request_resource_id(method: &str, params: &serde_json::Value) -> Option<ResourceId> {
    params_resource_id(params)
        .map(|(_, rid)| rid)
        .or_else(|| method_scoped_resource_id(method, params))
}

/// 이 메서드의 prefix 를 plugin 이 점유할 수 있는가 — **없으면** 호스트 전용이다.
///
/// 예약 목록은 매니페스트 검증이 `[[contributes.ipc_namespace]]` 를 거절하는 데 쓰는
/// 그것이고([ADR-0140](../../docs/adr/0140-host-ipc-prefixes-are-reserved-where-they-can-be-enforced.md)),
/// 그래서 예약된 prefix 의 메서드는 **어떤 plugin 에게도 forward 되지 않는다.**
/// 그 사실이 헤드리스에서 소유 검사를 engine handler **앞**에 둘 수 있게 한다 —
/// forward 될 수 있는 메서드였다면 검사가 그 경로를 먼저 잘라 버렸을 것이다.
///
/// 점 없는 메서드(`split` · `tree`)는 이름 전체가 prefix 자리이고 둘 다 예약이다.
///
/// gui 에는 필요 없다 — 거기서는 namespace forward 가 engine handler **앞**이라
/// plugin 이 가져갈 호출이 소유 검사에 닿지 않는다. 순서가 반대인 헤드리스에서만
/// 이 판별이 그 역할을 대신한다.
#[cfg(not(feature = "gui"))]
pub(crate) fn prefix_is_host_reserved(method: &str) -> bool {
    let prefix = method.split('.').next().unwrap_or(method);
    tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES.contains(&prefix)
}

/// 요청이 지목한 대상을 **아무 engine 도 안 가졌을 때** 돌려줄 메시지.
///
/// 문장 자체는 [`tasty_utils::target::unowned_target_message`] 가 정본이다 — 같은 문구를
/// plugin 프로세스도 내야 하는데 프로세스가 갈려 타입을 공유할 수 없어서, 두 벌이 따로
/// 표류하지 않도록 문자열 조립만 leaf crate 로 내렸다. 여기 남는 것은 **본체 타입에서
/// 그 인자를 뽑는 일**뿐이다(utils 가 `ResourceId` 를 알면 leaf 가 깨진다).
pub(crate) fn unowned_target_message(rid: ResourceId, method: &str) -> String {
    tasty_utils::target::unowned_target_message(rid.kind.label(), rid.id, method)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rid(params: &serde_json::Value) -> (&str, Kind, u64) {
        let (key, r) = params_resource_id(params).expect("expected a resource id");
        (key, r.kind, r.id)
    }

    /// `split` 은 **대상이 필수**인데 그 두 키가 인식 밖이라 모든 split 이 포커스된
    /// 창으로 갔다. 실측(창 둘): 같은 요청이 포커스에 따라 성공하기도
    /// `"surface N not found"` 로 실패하기도 했다 — 두 surface 다 살아 있는 채로.
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
        // ★ CLI 는 이 값을 **문자열로** 보낸다(같은 인자가 nickname 도 받는다).
        // 숫자만 보면 CLI 로 들어온 split 이 통째로 안 풀린다.
        assert!(
            matches!(
                by(&json!({ "level": "surface", "target_surface": "7" })),
                Some((Kind::Surface, 7))
            ),
            "CLI 가 보내는 문자열 형태가 안 풀린다 — 실제 사용 경로 전부가 폴백으로 간다"
        );
        // nickname 은 memory store 를 봐야 풀린다 — 순수 함수 밖이라 여기선 `None` 이고
        // `App::find_request_owner` 가 이어받는다.
        assert!(by(&json!({ "target_surface": "faraway" })).is_none());
        assert!(by(&json!({ "level": "surface" })).is_none());
        // 자르지 않는다 — 잘린 값은 실재하는 다른 surface 를 가리킨다.
        assert!(
            by(&json!({ "target_surface": (u64::from(u32::MAX) + 2).to_string() })).is_none(),
            "32 비트를 넘는 값을 잘라서 다른 surface 로 만들었다"
        );
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

    /// `workspace_category.rename`/`.delete` 의 `"id"` 는 카테고리를 지목한다 —
    /// **예약된 `normal`(id 0)만 빼고.**
    ///
    /// 카테고리 id 는 공유 카운터가 1 부터 발급해 engine 을 건너 유일하지만 `normal` 만
    /// 모든 engine 에 상수로 있다. 그것까지 지목으로 치면 "어느 창의 normal 인가" 가
    /// 정해지지 않는다. 빼도 잃는 것이 없다 — normal 은 rename·delete 가 거부하는
    /// 항목이라 대상이 될 일이 없고, 그 거절 문구는 종전 경로가 그대로 낸다.
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
        // 예약 카테고리는 지목 대상이 아니다.
        for m in [
            "workspace_category.rename",
            "workspace_category.delete",
            "workspace_category.move",
        ] {
            assert!(
                method_scoped_resource_id(m, &json!({ "id": 0, "name": "x" })).is_none(),
                "{m} 이 normal 을 지목으로 쳤다 — 창이 정해지지 않는다"
            );
        }
        // 같은 `"id"` 키를 쓰는 다른 메서드로 새지 않는다.
        assert!(method_scoped_resource_id("workspace_category.create", &p).is_none());
        assert!(method_scoped_resource_id("memory.get", &p).is_none());
    }

    /// `workspace.close`/`workspace.update` 의 `"id"` 는 workspace 를 지목한다.
    ///
    /// 이 규칙이 없던 동안 이 둘은 **포커스된 창**으로 갔다 — 실측(2026-09-05, 창 둘):
    /// 비포커스 창이 가진 workspace 1 을 지목한 `workspace.update` 가
    /// "Workspace 1 not found" 로 실패했고, 포커스를 옮기면 같은 요청이 성공했다.
    #[test]
    fn workspace_id_key_routes_only_for_the_methods_that_mean_workspace() {
        let p = json!({ "id": 4, "name": "x" });
        for m in ["workspace.close", "workspace.update", "workspace.move"] {
            let rid = method_scoped_resource_id(m, &p).expect("대상이 잡혀야 한다");
            assert!(matches!(rid.kind, Kind::Workspace), "{m}");
            assert_eq!(rid.id, 4);
        }
        // `"id"` 는 호스트 전체에서 뜻이 갈리는 범용 키라 **메서드로 한정**한다 —
        // 넓히면 hook id · agent id · plugin id 가 workspace 로 오해된다.
        assert!(method_scoped_resource_id("memory.get", &p).is_none());
        assert!(method_scoped_resource_id("approval.cancel", &p).is_none());
        // index 로 지목한 형태는 여전히 대상 없음이다 — index 는 창 안의 위치라
        // 창을 건너 해석할 수 있는 값이 아니다.
        assert!(
            method_scoped_resource_id("workspace.close", &json!({ "index": 0 })).is_none(),
            "index 는 창을 건너 해석되면 안 된다"
        );
        assert!(
            method_scoped_resource_id("workspace.move", &json!({ "from_index": 2, "to_index": 0 }))
                .is_none(),
            "from_index 만 준 이동은 여전히 대상이 없다 — 창이 정해지지 않는다"
        );
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

    /// `pty.spawn` 도 그 목록에 없다 — **생성**이라 실을 id 가 없다(문서가 그렇게 적고
    /// 있다). 목록이 화이트리스트라 실수로 들어올 일은 없지만, 이웃
    /// `pty.attach_surface` 는 단정이 있고 이것만 없으면 "빠뜨린 것" 과 "뺀 것" 이
    /// 구분되지 않는다.
    #[test]
    fn spawn_is_not_a_target_because_it_creates_one() {
        let params = json!({ "id": 0x8000_0002u32 });
        assert!(method_scoped_resource_id("pty.spawn", &params).is_none());
    }

    /// 라우팅이 대상을 **푸는 것**과 그 대상을 **가진 engine 을 찾는 것**은 다른 단계다.
    /// 위 테스트가 앞 단계를 잡고, 이것이 뒷 단계(`engine_has_resource`)를 잡는다 —
    /// 앞만 있으면 kind 는 생겼는데 어느 창도 그 자원을 "가졌다" 고 답하지 않아 요청이
    /// 여전히 새어 나간다.
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

    /// 두 hook 표면은 **같은 `hook_id` 키**를 쓰지만 저장소가 다르다
    /// (`engine.hook_manager` / `engine.global_hook_manager`). 키로는 못 가르므로
    /// 메서드로 한정하고, 각각 다른 `Kind` 로 푼다.
    ///
    /// global 쪽이 한때 "대상이 아니다" 였던 것은 그 이름 때문에 **창에 안 매인다고 읽은
    /// 것**이고, 실제로는 그 매니저가 `CoreState` 필드라 창마다 하나다. 그동안 그 요청은
    /// 포커스된 창으로 새서 다른 창의 훅에 닿지 못했다.
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

        // 키만으로는 여전히 안 풀린다 — 가르는 것은 메서드다.
        assert!(params_resource_id(&params).is_none());
        assert!(params_resource_id(&params).is_none());
    }

    /// observer 를 **쓰는** 메서드만 대상이 있다 — **만드는** 쪽은 실을 id 가 없다.
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

    /// `preset.capture` 의 `source_id` 가 무엇인지는 `kind` 가 정한다.
    ///
    /// 이 키 하나가 세 종류를 가리키므로 키 목록에 못 넣는다. `kind` 를 모르면 대상도
    /// 모른다 — 마지막 단언이 그 경우 조용히 아무거나 고르지 않는다는 것을 못박는다.
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

    /// `u32` 에 안 들어가는 값은 창에 매인 리소스의 id 일 수 없다.
    ///
    /// hook·observer id 가 `u64` 라 [`ResourceId`] 가 `u64` 를 든다. 좁히면서 자르면
    /// **다른 리소스의 id 로 둔갑**해 엉뚱한 창이 주인이 된다. 자르지 않고 판정한다.
    #[test]
    fn an_id_too_large_for_a_window_resource_has_no_owner() {
        let big = json!({ "surface_id": u64::from(u32::MAX) + 1 });
        let (_, rid) = params_resource_id(&big).expect("키는 인식된다");
        assert!(matches!(rid.kind, Kind::Surface));
        assert!(u32::try_from(rid.id).is_err(), "좁히기가 실패해야 한다");
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

#[cfg(all(test, not(feature = "gui")))]
mod headless_prefix_tests {
    use super::prefix_is_host_reserved;

    /// 판별식이 서는 자리 — 예약된 prefix 는 forward 되지 않고, 안 된 prefix 는 된다.
    #[test]
    fn a_reserved_prefix_is_host_only_and_an_unreserved_one_is_not() {
        assert!(prefix_is_host_reserved("workspace.create"));
        assert!(prefix_is_host_reserved("surface.close"));
        // 점 없는 메서드도 이름 전체가 prefix 자리다.
        assert!(prefix_is_host_reserved("split"));
        assert!(prefix_is_host_reserved("tree"));
        // 번들 plugin 이 점유해서 예약할 수 없는 둘 — 여기서 잘리면 forward 가 죽는다.
        assert!(!prefix_is_host_reserved("image.open"));
        assert!(!prefix_is_host_reserved("markdown.recent"));
        // 아무도 안 쓰는 이름도 예약이 아니다(plugin 이 가질 수 있다).
        assert!(!prefix_is_host_reserved("codex.spawn"));
    }
}
