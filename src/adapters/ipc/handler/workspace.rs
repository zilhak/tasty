use serde_json::json;

use super::params::{self, p_try};
use crate::model::{WorkspaceAttachMapping, WorkspaceAttachTarget};
use tasty_ipc::protocol::JsonRpcResponse;

/// attach_profile을 우선하고 없으면 attach_ssh를 읽는다. 둘 다 없으면 매핑 없음이다.
/// 잘못된 원격 workspace 값은 생략으로 처리하지 않고 거절한다.
fn parse_attach_mapping(
    params: &serde_json::Value,
) -> Result<Option<WorkspaceAttachMapping>, String> {
    let remote_workspace = params::read_int::<u32>(params, "attach_remote_workspace")?;
    if let Some(name) = params
        .get("attach_profile")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Ok(Some(WorkspaceAttachMapping {
            target: WorkspaceAttachTarget::Profile {
                name: name.to_string(),
            },
            remote_workspace,
        }));
    }
    if let Some(host) = params
        .get("attach_ssh")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Ok(Some(WorkspaceAttachMapping {
            target: WorkspaceAttachTarget::Inline {
                host: host.to_string(),
                remote_tasty: None,
                port_mode: None,
                port_file: None,
            },
            remote_workspace,
        }));
    }
    Ok(None)
}

/// release 입력 제한에 쓰는 loopback 주소 형식 검사.
#[cfg(not(debug_assertions))]
fn is_loopback_attach_host(host: &str) -> bool {
    let h = if host.strip_prefix("[::1]:").is_some() {
        "::1"
    } else if let Some((h, _port)) = host.rsplit_once(':') {
        h
    } else {
        return false;
    };
    matches!(h, "127.0.0.1" | "localhost" | "::1")
}

/// release에서는 loopback attach 매핑을 거절한다. 로컬 mirror 시험은 debug attach를 쓴다.
#[cfg(not(debug_assertions))]
fn reject_loopback_attach(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    let host = params
        .get("attach_ssh")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    if is_loopback_attach_host(host) {
        return Some(JsonRpcResponse::invalid_params(
            id.clone(),
            "loopback(self) attach 매핑은 release 빌드에서 지원되지 않습니다 \
             (로컬 self-attach 는 debug 빌드 `tasty debug attach` 전용).",
        ));
    }
    None
}

/// create/update params 의 `category` 를 카테고리 id 로 해석한다.
/// - 필드 없음 → `Ok(None)` (호출자가 normal 기본값 유지).
/// - 숫자(id) 또는 문자열(이름/id 토큰) → 존재하면 `Ok(Some(id))`.
/// - 주어졌으나 해석 불가 → `Err(메시지)`.
fn resolve_category_param(
    engine: &crate::core::CoreState,
    params: &serde_json::Value,
) -> Result<Option<crate::model::WorkspaceCategoryId>, String> {
    let Some(token) = params::read_id_or_name(params, "category")? else {
        return Ok(None);
    };
    match engine.resolve_category(&token) {
        Some(id) => Ok(Some(id)),
        None => Err(format!("unknown category: {token}")),
    }
}

fn mapping_to_json(mapping: &Option<WorkspaceAttachMapping>) -> serde_json::Value {
    match mapping {
        Some(m) => serde_json::to_value(m).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    }
}

pub fn handle_workspace_list(
    window: &dyn crate::ipc::window_port::IpcWindow,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let workspaces: Vec<_> = engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let sids = ws.all_surface_ids();
            json!({
                "id": ws.id,
                "name": ws.name,
                "subtitle": ws.subtitle,
                "description": ws.description,
                "active": i == window.active_workspace_index(),
                "pane_count": ws.pane_layout().all_pane_ids().len(),
                "busy_count": engine.busy_count(&sids),
                "attach_mapping": mapping_to_json(&ws.attach_mapping),
                // 연결 설정과 현재 mirror 여부는 다르다.
                "mirror": ws.mirror,
                "category": ws.category,
                "category_name": engine.category_name(ws.category),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(workspaces))
}

/// surface_id를 지정하면 그 대상의 cwd를 쓴다. 생략한 경우만 창의 포커스 surface를 따른다.
fn inherit_cwd_for_create(
    window: &dyn crate::ipc::window_port::IpcWindow,
    engine: &crate::core::CoreState,
    named_surface: Option<u32>,
) -> Option<std::path::PathBuf> {
    match named_surface {
        Some(surface_id) => window.resolve_inherit_cwd_from_surface(engine, surface_id),
        None => window.resolve_inherit_cwd(engine),
    }
}

/// terminal 생성의 cwd를 정한다. 명시값이 우선하고 다른 kind에서는 사용하지 않는다.
/// 원격 mirror의 경로는 로컬 workspace에 상속하지 않는다.
/// 잘못된 surface_id는 포커스 대상으로 대체하지 않고 거절한다.
fn resolve_create_cwd(
    params: &serde_json::Value,
    kind: &str,
    window: &dyn crate::ipc::window_port::IpcWindow,
    engine: &crate::core::CoreState,
    id: &serde_json::Value,
) -> Result<Option<std::path::PathBuf>, JsonRpcResponse> {
    let explicit_cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    if let Some(p) = &explicit_cwd
        && !p.is_dir()
    {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("cwd does not exist: {}", p.display()),
        ));
    }
    let named_surface = params::opt_int::<u32>(params, "surface_id", id)?;
    Ok(if kind == "terminal" {
        explicit_cwd.or_else(|| inherit_cwd_for_create(window, engine, named_surface))
    } else {
        None
    })
}

pub fn handle_workspace_create(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    #[cfg(not(debug_assertions))]
    if let Some(resp) = reject_loopback_attach(params, &id) {
        return resp;
    }
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");
    let resolved_cwd = p_try!(resolve_create_cwd(params, kind, window, engine, &id));

    if let Some(def) = engine.surface_registry.get_live(kind)
        && let Some(missing) = def.first_missing_required_param(params)
    {
        return JsonRpcResponse::invalid_params(
            id,
            format!("Missing '{missing}' parameter for {kind} type"),
        );
    }

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let subtitle = params
        .get("subtitle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let intent = crate::core::intent::DomainIntent::CreateWorkspace {
        cwd: resolved_cwd,
        kind: kind.to_string(),
        surface_params: params.clone(),
        name,
        subtitle,
        description,
        category: None,
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    let Some(crate::core::intent::CoreEvent::WorkspaceCreated {
        id: workspace_id,
        index,
        surface_id,
        renamed_name,
        renamed_subtitle,
        renamed_description,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(
            id,
            "Core::apply returned no WorkspaceCreated event",
        );
    };

    // 생성 이벤트를 알리되 에이전트 요청이므로 활성 workspace는 바꾸지 않는다.
    let agent_origin = crate::intent::IntentOrigin::Agent {
        source: crate::intent::AgentSource::Ipc,
    };
    window.cascade_workspace_created(
        engine,
        &agent_origin,
        0,
        crate::app::dispatch_domain::WorkspaceCreatedCascade {
            workspace_id,
            index,
            surface_id,
            renamed_name: renamed_name.clone(),
            renamed_subtitle: renamed_subtitle.clone(),
            renamed_description: renamed_description.clone(),
        },
    );

    // 카테고리를 지정하지 않으면 기본 normal을 유지한다. 사용자 선택은 바꾸지 않는다.
    match resolve_category_param(engine, params) {
        Ok(Some(cat_id)) => {
            engine.workspaces[index].set_category(cat_id);
            engine.mark_layout_dirty();
        }
        Ok(None) => {}
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    }

    if let Some(mapping) = match parse_attach_mapping(params) {
        Ok(v) => v,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    } {
        engine.workspaces[index].set_attach_mapping(Some(mapping));
        engine.mark_layout_dirty();
    }

    let ws = &engine.workspaces[index];
    JsonRpcResponse::success(
        id,
        json!({
            "id": ws.id,
            "name": ws.name,
            "subtitle": ws.subtitle,
            "description": ws.description,
            "index": index,
            "surface_id": surface_id,
            "attach_mapping": mapping_to_json(&ws.attach_mapping),
            "category": ws.category,
        }),
    )
}

pub fn handle_workspace_update(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    #[cfg(not(debug_assertions))]
    if let Some(resp) = reject_loopback_attach(params, &id) {
        return resp;
    }
    // id가 없을 때만 index를 쓴다. 잘못된 id를 index로 대체하면 다른 대상을 바꿀 수 있다.
    let id_param = match super::params::optional_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let workspace_id = if let Some(ws_id) = id_param {
        ws_id
    } else if let Some(i) = p_try!(params::opt_int::<u64>(params, "index", &id)) {
        let idx = i as usize;
        if idx >= engine.workspaces.len() {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Workspace index {} out of range (0..{})",
                    idx,
                    engine.workspaces.len()
                ),
            );
        }
        engine.workspaces[idx].id
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' or 'index' parameter");
    };

    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let subtitle = params
        .get("subtitle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let intent = crate::core::intent::DomainIntent::UpdateWorkspaceMeta {
        workspace_id,
        name,
        subtitle,
        description,
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::invalid_params(id, e.to_string()),
    };

    let Some(crate::core::intent::CoreEvent::WorkspaceMetaUpdated {
        workspace_id,
        index,
        name,
        subtitle,
        description,
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(
            id,
            "Core::apply returned no WorkspaceMetaUpdated event",
        );
    };

    window.cascade_workspace_meta_updated(workspace_id, name, subtitle, description);

    match resolve_category_param(engine, params) {
        Ok(Some(cat_id)) => {
            if let Err(e) = engine.set_workspace_category(workspace_id, cat_id) {
                return JsonRpcResponse::invalid_params(id, e.to_string());
            }
            engine.mark_layout_dirty();
        }
        Ok(None) => {}
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    }

    // attach_clear가 우선이다. 변경은 layout.json에 저장하도록 표시한다.
    let clear = params
        .get("attach_clear")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if clear {
        engine.workspaces[index].set_attach_mapping(None);
        engine.mark_layout_dirty();
    } else if let Some(mapping) = match parse_attach_mapping(params) {
        Ok(v) => v,
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    } {
        engine.workspaces[index].set_attach_mapping(Some(mapping));
        engine.mark_layout_dirty();
    }

    let ws = &engine.workspaces[index];
    JsonRpcResponse::success(
        id,
        json!({
            "id": ws.id,
            "name": ws.name,
            "subtitle": ws.subtitle,
            "description": ws.description,
            "index": index,
            "attach_mapping": mapping_to_json(&ws.attach_mapping),
            "category": ws.category,
        }),
    )
}

/// ID로 workspace를 닫는다. index도 호환 입력으로 받는다.
/// 사용자 복원 기록은 남기지 않고 기존 활성 대상이 유지되도록 위치를 보정한다.
/// 활성 대상 자체를 닫았을 때만 다른 workspace로 이동한다.
/// 자기 surface 포함·mirror·hard 점유·마지막 workspace는 거절한다(ADR-0017).
/// 창 종료는 별도 window.close 요청이며 마지막 workspace 닫기로 대신하지 않는다.
pub fn handle_workspace_close(
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let id_param = match super::params::optional_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let ws_idx = if let Some(ws_id) = id_param {
        match engine.workspaces.iter().position(|w| w.id == ws_id) {
            Some(i) => i,
            None => {
                return JsonRpcResponse::invalid_params(id, format!("Workspace {ws_id} not found"));
            }
        }
    } else if let Some(i) = p_try!(params::opt_int::<u64>(params, "index", &id)) {
        let idx = i as usize;
        if idx >= engine.workspaces.len() {
            return JsonRpcResponse::invalid_params(
                id,
                format!(
                    "Workspace index {} out of range (0..{})",
                    idx,
                    engine.workspaces.len()
                ),
            );
        }
        idx
    } else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'id' or 'index' parameter");
    };

    if let Some(caller) = super::caller_surface_id(params)
        && engine
            .find_workspace_index_for_surface(caller)
            .map(|(i, _)| i)
            == Some(ws_idx)
    {
        return JsonRpcResponse::invalid_params(
            id,
            "Cannot close a workspace that contains your own surface. Move elsewhere first, \
             or use 'tasty close self' to close just your surface.",
        );
    }

    // mirror는 터미널·busy·attention·mesh를 함께 정리하는 attach 해제 경로를 사용해야 한다.
    if engine.workspaces[ws_idx].mirror {
        return JsonRpcResponse::invalid_params(
            id,
            "Workspace is a mirror of a remote attach session — detach it from that session \
             instead of closing it here",
        );
    }

    // 로컬 소유 surface라도 원격 클라이언트가 점유 중이면 닫지 않는다.
    if let Some(occupied) = engine.workspaces[ws_idx]
        .all_surface_ids()
        .into_iter()
        .find(|sid| engine.attach.is_hard_occupied(*sid))
    {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "Workspace holds surface {occupied}, which is occupied by a remote attach \
                 session (hard-occupied) — someone is working in that terminal right now. \
                 Release it from the attaching instance first."
            ),
        );
    }

    if engine.workspaces.len() == 1 {
        return JsonRpcResponse::invalid_params(id, last_workspace_refusal());
    }

    let workspace_id = engine.workspaces[ws_idx].id;
    let closed =
        window.close_workspace_at(engine, ws_idx, crate::state::WorkspaceCloseOrigin::Agent);
    JsonRpcResponse::success(id, json!({ "closed": closed, "id": workspace_id }))
}

/// GUI에는 별도 창 닫기를 안내하고 그 기능이 없는 헤드리스에는 권하지 않는다.
fn last_workspace_refusal() -> &'static str {
    if cfg!(feature = "gui") {
        "Refusing to close the last workspace — closing the window instead is a separate \
         decision; use 'window.close' explicitly if that is what you want"
    } else {
        "Refusing to close the last workspace — the last workspace of a headless instance \
         cannot be closed (there is no window to close instead)"
    }
}

/// id로 소유 창과 workspace를 선택한다. from_index는 호환 입력으로 포커스된 창을 사용한다.
/// 둘을 함께 지정하면 거절한다. to_index는 선택된 창 안의 목적지다.
pub fn handle_workspace_move(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let named = p_try!(params::opt_int::<u64>(params, "id", &id));
    let from_index = p_try!(params::opt_int::<u64>(params, "from_index", &id));
    let from = match (named, from_index) {
        (Some(_), Some(_)) => {
            return JsonRpcResponse::invalid_params(
                id,
                "give either 'id' (the workspace to move) or 'from_index', not both",
            );
        }
        (Some(ws_id), None) => match engine.find_workspace_index_for_id(ws_id as u32) {
            Some(i) => i,
            None => {
                return JsonRpcResponse::invalid_params(id, format!("no workspace {ws_id}"));
            }
        },
        (None, Some(f)) => f as usize,
        (None, None) => {
            return JsonRpcResponse::invalid_params(id, "Missing 'id' or 'from_index' parameter");
        }
    };
    let to = match p_try!(params::opt_int::<u64>(params, "to_index", &id)) {
        Some(t) => t as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'to_index' parameter"),
    };

    let intent = crate::core::intent::DomainIntent::MoveWorkspace {
        from_index: from,
        to_index: to,
    };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    let moved = matches!(
        events.first(),
        Some(crate::core::intent::CoreEvent::WorkspaceMoved { moved: true, .. })
    );
    if moved {
        window.fix_workspace_pointers_after_move(from, to);
    }
    JsonRpcResponse::success(id, json!({ "moved": moved }))
}

#[cfg(test)]
mod close_tests {
    use super::*;
    use serde_json::json;

    fn add_workspace(engine: &mut crate::core::CoreState) -> u32 {
        let event = crate::core::apply_create_workspace_inner(
            engine,
            crate::core::WorkspaceCreationParams::terminal(),
        )
        .unwrap();
        let crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } = event else {
            panic!("expected WorkspaceCreated");
        };
        engine.workspaces[index].id
    }

    #[test]
    fn closing_the_last_workspace_is_refused() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        assert_eq!(engine.workspaces.len(), 1);
        let only = engine.workspaces[0].id;

        let res = handle_workspace_close(&mut state, &mut engine, json!(1), &json!({ "id": only }));

        let err = res.error.expect("마지막 워크스페이스는 거절해야 한다");
        assert_eq!(
            err.code, -32602,
            "코드는 두 조합 모두 invalid_params 그대로"
        );
        if cfg!(feature = "gui") {
            assert!(
                err.message.contains("use 'window.close' explicitly"),
                "{}",
                err.message
            );
        } else {
            assert!(
                err.message
                    .contains("the last workspace of a headless instance cannot be closed"),
                "{}",
                err.message
            );
            assert!(!err.message.contains("window.close"), "{}", err.message);
        }
        assert_eq!(
            engine.workspaces.len(),
            1,
            "거절이면 아무것도 닫히지 않는다"
        );
    }

    #[test]
    fn closing_a_workspace_a_remote_session_occupies_is_refused() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let target = add_workspace(&mut engine);
        let ws_idx = engine
            .workspaces
            .iter()
            .position(|w| w.id == target)
            .expect("방금 만든 워크스페이스가 있어야 한다");
        let occupied = engine.workspaces[ws_idx]
            .all_surface_ids()
            .first()
            .copied()
            .expect("워크스페이스에 surface 가 있어야 한다");
        engine
            .attach
            .acquire(occupied, 1)
            .expect("하드 점유를 잡을 수 있어야 한다");

        let res =
            handle_workspace_close(&mut state, &mut engine, json!(1), &json!({ "id": target }));

        let err = res
            .error
            .expect("하드 점유 중인 워크스페이스는 거절해야 한다");
        assert!(
            err.message.contains("hard-occupied"),
            "거절 사유가 점유임을 알려야 한다: {}",
            err.message
        );
        assert_eq!(
            engine.workspaces.len(),
            2,
            "거절이면 아무것도 닫히지 않는다"
        );
        assert!(
            engine.attach.is_hard_occupied(occupied),
            "거절 경로가 점유 상태를 건드리면 안 된다"
        );
    }

    #[test]
    fn closing_the_workspace_holding_your_own_surface_is_refused() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        add_workspace(&mut engine);
        let target = engine.workspaces[0].id;
        let caller = engine.workspaces[0]
            .all_surface_ids()
            .first()
            .copied()
            .expect("워크스페이스에 surface 가 있어야 한다");

        let res = handle_workspace_close(
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "id": target, "caller_surface_id": caller }),
        );

        assert!(
            res.error.is_some(),
            "자기 surface 가 든 대상은 거절해야 한다"
        );
        assert_eq!(engine.workspaces.len(), 2);
    }

    // 성공 경로에서 명시 대상·활성 포인터·복원 기록·응답 ID를 함께 확인한다.
    #[test]
    fn closing_a_workspace_the_user_is_not_looking_at_leaves_the_view_alone() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let target_id = add_workspace(&mut engine);
        add_workspace(&mut engine);
        assert_eq!(engine.workspaces.len(), 3);

        state.active_workspace = 2;
        let viewing_id = engine.workspaces[2].id;
        let target_idx = 1;
        assert_eq!(engine.workspaces[target_idx].id, target_id);
        assert_ne!(
            target_idx as u32, target_id,
            "인덱스와 id 가 같으면 응답이 어느 쪽을 실었는지 구분할 수 없다"
        );

        let res = handle_workspace_close(
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "id": target_id }),
        );

        assert!(res.error.is_none(), "성공해야 한다: {:?}", res.error);
        let result = res.result.expect("success 응답에는 result 가 있다");
        assert_eq!(result["closed"], true);
        assert_eq!(
            result["id"], target_id,
            "응답은 인덱스가 아니라 대상 워크스페이스 id 를 돌려줘야 한다"
        );

        assert_eq!(engine.workspaces.len(), 2);
        assert!(
            engine.workspaces.iter().all(|w| w.id != target_id),
            "대상이 아직 남아 있다"
        );

        // 앞쪽 항목을 지워 인덱스가 바뀌어도 사용자가 보던 ID는 유지되어야 한다.
        assert_eq!(
            engine.workspaces[state.active_workspace].id, viewing_id,
            "앞쪽 워크스페이스를 닫았는데 사용자 시야가 다른 워크스페이스로 옮겨갔다"
        );

        assert!(
            engine.closed_items.is_empty(),
            "에이전트가 닫은 것이 사용자의 되돌리기 스택에 들어갔다"
        );

        let events = state.take_pending_lifecycle_events();
        assert!(
            !events.is_empty(),
            "닫힌 surface 의 lifecycle 이벤트가 없다"
        );
        assert!(
            events.iter().all(|e| !e.is_user_close),
            "에이전트가 닫았는데 plugin 에는 사용자 close 로 나간다"
        );
    }

    #[test]
    fn closing_a_mirror_workspace_is_refused() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mirror_id = add_workspace(&mut engine);
        let mirror_idx = engine
            .workspaces
            .iter()
            .position(|w| w.id == mirror_id)
            .expect("방금 만든 워크스페이스");
        engine.workspaces[mirror_idx].mirror = true;

        let res = handle_workspace_close(
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "id": mirror_id }),
        );

        assert!(res.error.is_some(), "mirror 워크스페이스는 거절해야 한다");
        assert_eq!(
            engine.workspaces.len(),
            2,
            "거절이면 아무것도 닫히지 않는다"
        );
    }

    #[test]
    fn closing_an_unknown_workspace_id_is_refused() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        add_workspace(&mut engine);

        let res =
            handle_workspace_close(&mut state, &mut engine, json!(1), &json!({ "id": 999_999 }));

        assert!(res.error.is_some());
        assert_eq!(engine.workspaces.len(), 2);
    }
}

#[cfg(test)]
mod create_cwd_tests {
    use super::*;
    use serde_json::json;

    fn open_explorer(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        rel: &str,
    ) -> (u32, std::path::PathBuf) {
        let root = crate::test_support::abs_path(rel);
        let (_tab, sid) = state
            .add_kind_tab(
                engine,
                "explorer",
                &json!({ "path": root.to_string_lossy() }),
            )
            .expect("add explorer tab");
        (sid, root)
    }

    // 요청 대상과 포커스 대상을 다르게 준비한다.
    #[test]
    fn a_named_surface_is_the_inherit_source_not_the_focus() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let (named, named_root) = open_explorer(&mut state, &mut engine, "named/proj");
        let (focused, focused_root) = open_explorer(&mut state, &mut engine, "focused/proj");
        assert_eq!(state.focused_surface_id(&engine), Some(focused));
        assert_ne!(named_root, focused_root);

        assert_eq!(
            inherit_cwd_for_create(&state, &engine, Some(named)),
            Some(named_root),
        );
    }

    #[test]
    fn without_a_named_surface_the_focus_is_the_inherit_source() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let (_named, _) = open_explorer(&mut state, &mut engine, "named/proj");
        let (_focused, focused_root) = open_explorer(&mut state, &mut engine, "focused/proj");

        assert_eq!(
            inherit_cwd_for_create(&state, &engine, None),
            Some(focused_root),
        );
    }

    #[test]
    fn a_malformed_surface_id_is_rejected_not_ignored() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let before = engine.workspaces.len();

        let res = handle_workspace_create(
            &mut core,
            &mut state,
            &mut engine,
            json!(1),
            &json!({ "surface_id": "7" }),
        );

        let err = res.error.expect("숫자가 아닌 surface_id 는 거절해야 한다");
        assert_eq!(err.code, -32602, "{}", err.message);
        assert!(err.message.contains("surface_id"), "{}", err.message);
        assert_eq!(engine.workspaces.len(), before);
    }

    // 헬퍼뿐 아니라 요청 파라미터가 실제 상속 원본으로 전달되는지도 확인한다.
    #[test]
    fn the_params_surface_id_reaches_the_inherit_source() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let (named, named_root) = open_explorer(&mut state, &mut engine, "named/proj");
        let (_focused, focused_root) = open_explorer(&mut state, &mut engine, "focused/proj");
        assert_ne!(named_root, focused_root);

        let got = resolve_create_cwd(
            &json!({ "surface_id": named }),
            "terminal",
            &state,
            &engine,
            &json!(1),
        )
        .expect("정상 params");
        assert_eq!(got, Some(named_root));

        let got = resolve_create_cwd(&json!({}), "terminal", &state, &engine, &json!(1))
            .expect("정상 params");
        assert_eq!(got, Some(focused_root));
    }

    #[test]
    fn explicit_cwd_wins_and_non_terminal_kinds_take_no_cwd() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let (named, _) = open_explorer(&mut state, &mut engine, "named/proj");
        let explicit = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let params = json!({ "surface_id": named, "cwd": explicit.to_string_lossy() });

        let got = resolve_create_cwd(&params, "terminal", &state, &engine, &json!(1))
            .expect("정상 params");
        assert_eq!(got, Some(explicit));

        let got = resolve_create_cwd(
            &json!({ "surface_id": named }),
            "explorer",
            &state,
            &engine,
            &json!(1),
        )
        .expect("정상 params");
        assert_eq!(got, None);
    }

    // 실제 PTY의 cwd를 조회해 계산한 경로가 생성까지 전달되는지 확인한다.
    // 사용자 rc 영향을 피하도록 /bin/sh를 사용한다. Windows는 get_cwd_of_pid가 없어 제외한다.
    // 이 시험은 Windows의 실제 cwd를 검증하지 않으며 생성 payload 코드는 플랫폼 공통이다.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_resolved_cwd_reaches_the_new_terminals_shell() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        engine.settings.general.shell = "/bin/sh".to_string();

        let explicit_dir = tempfile::tempdir().expect("tmp");
        let explicit = explicit_dir.path().canonicalize().expect("canonical");
        let inherit_dir = tempfile::tempdir().expect("tmp");
        let inherited = inherit_dir.path().canonicalize().expect("canonical");
        let (_tab, named) = state
            .add_kind_tab(
                &mut engine,
                "explorer",
                &json!({ "path": inherited.to_string_lossy() }),
            )
            .expect("add explorer tab");

        for (params, want) in [
            (json!({ "cwd": explicit.to_string_lossy() }), &explicit),
            (json!({ "surface_id": named }), &inherited),
        ] {
            let res =
                handle_workspace_create(&mut core, &mut state, &mut engine, json!(1), &params);
            let result = res
                .result
                .unwrap_or_else(|| panic!("{params}: {:?}", res.error));
            let sid = result["surface_id"].as_u64().expect("surface_id") as u32;
            assert_eq!(
                engine.local_surface_cwd(sid).as_ref(),
                Some(want),
                "{params}: 새 터미널의 셸이 계산된 cwd 에서 뜨지 않았다",
            );
        }
    }

    #[test]
    fn a_named_surface_respects_inherit_cwd_off() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let (named, _) = open_explorer(&mut state, &mut engine, "named/proj");
        engine.settings.general.inherit_cwd = false;

        assert_eq!(inherit_cwd_for_create(&state, &engine, Some(named)), None,);
    }
}
