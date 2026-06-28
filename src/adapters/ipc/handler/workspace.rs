use serde_json::json;

use crate::model::{WorkspaceAttachMapping, WorkspaceAttachTarget};
use crate::state::AppState;
use tasty_ipc::protocol::JsonRpcResponse;

/// 단계 7 — workspace.create/update params 에서 SSH attach 매핑을 파싱한다.
/// `attach_profile`(저장 프로필) 우선, 없으면 `attach_ssh`(1회성 인라인).
/// 둘 다 없으면 None(매핑 없음).
fn parse_attach_mapping(params: &serde_json::Value) -> Option<WorkspaceAttachMapping> {
    let remote_workspace = params
        .get("attach_remote_workspace")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    if let Some(name) = params
        .get("attach_profile")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(WorkspaceAttachMapping {
            target: WorkspaceAttachTarget::Profile {
                name: name.to_string(),
            },
            remote_workspace,
        });
    }
    if let Some(host) = params
        .get("attach_ssh")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
    {
        return Some(WorkspaceAttachMapping {
            target: WorkspaceAttachTarget::Inline {
                host: host.to_string(),
                remote_tasty: None,
                port_mode: None,
            },
            remote_workspace,
        });
    }
    None
}

/// `attach_ssh` host 가 self(loopback) 대상(`127.0.0.1:PORT`/`localhost:PORT`/
/// `[::1]:PORT`)인지 판정한다. release 빌드의 self-attach 매핑 차단에 쓴다(원칙 1 ②).
/// (`src/app/auto_attach.rs::parse_loopback_port` 의 판정과 동일 규칙 — 그쪽은
/// 공유 실행단 헬퍼라 보존하고, 여기선 입력단 거부용 술어만 둔다.)
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

/// release 빌드에서 self(loopback) attach 매핑을 입력단에서 거부한다(원칙 1 ②).
/// 로컬 self-mirror 는 사용자 입력 재현 성격이라 debug 빌드 `tasty debug attach`
/// 전용 — workspace 매핑(`--ssh 127.0.0.1:PORT`)으로 우회 self-attach 를 막는다.
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
    let Some(v) = params.get("category") else {
        return Ok(None);
    };
    if v.is_null() {
        return Ok(None);
    }
    let token = if let Some(n) = v.as_u64() {
        n.to_string()
    } else if let Some(s) = v.as_str() {
        if s.trim().is_empty() {
            return Ok(None);
        }
        s.to_string()
    } else {
        return Err("'category' must be a category id (number) or name (string)".to_string());
    };
    match engine.resolve_category(&token) {
        Some(id) => Ok(Some(id)),
        None => Err(format!("unknown category: {token}")),
    }
}

/// 매핑을 JSON 으로 노출(workspace.list 의 read — 원칙 3 read 허용).
fn mapping_to_json(mapping: &Option<WorkspaceAttachMapping>) -> serde_json::Value {
    match mapping {
        Some(m) => serde_json::to_value(m).unwrap_or(serde_json::Value::Null),
        None => serde_json::Value::Null,
    }
}

pub fn handle_workspace_list(
    state: &AppState,
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
                "active": i == state.active_workspace,
                "pane_count": ws.pane_layout().all_pane_ids().len(),
                "busy_count": engine.busy_count(&sids),
                "attach_mapping": mapping_to_json(&ws.attach_mapping),
                "category": ws.category,
                "category_name": engine.category_name(ws.category),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!(workspaces))
}

pub fn handle_workspace_create(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    // release: self(loopback) attach 매핑 입력단 거부(원칙 1 ②).
    #[cfg(not(debug_assertions))]
    if let Some(resp) = reject_loopback_attach(params, &id) {
        return resp;
    }
    let explicit_cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(std::path::PathBuf::from);
    // CLI 가 absolute path 로 정규화해 보낸다는 contract — 2 차 방어로 호스트도
    // 디렉토리 존재 검증. plugin 의 직접 IPC 경로도 함께 보호.
    if let Some(p) = &explicit_cwd
        && !p.is_dir()
    {
        return JsonRpcResponse::invalid_params(id, format!("cwd does not exist: {}", p.display()));
    }
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // 필수 파라미터 검증 (registry create 함수도 검사하지만, 명확한 에러 메시지를 위해 선검증)
    if kind == "markdown"
        && params
            .get("file")
            .and_then(|v| v.as_str())
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return JsonRpcResponse::invalid_params(id, "Missing 'file' parameter for markdown type");
    }

    // terminal 의 cwd inherit 은 호출자가 미리 결정해 payload 로 넘긴다 (Core 는
    // focus state 모름). 그 외 kind 는 cwd 미사용.
    let resolved_cwd = if kind == "terminal" {
        explicit_cwd.or_else(|| state.resolve_inherit_cwd(engine))
    } else {
        None
    };

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
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return JsonRpcResponse::internal_error(id, e.to_string()),
    };

    // events 안에 정확히 하나의 WorkspaceCreated 가 들어있다.
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

    // cascade: host event 발화 (rename 필드 있을 때). IPC 는 Agent origin 이므로
    // active 전환은 하지 않는다 (`cascade_workspace_created` 가 origin 보고 분기).
    let agent_origin = crate::intent::IntentOrigin::Agent {
        source: crate::intent::AgentSource::Ipc,
    };
    crate::app::dispatch_domain::cascade_workspace_created(
        state,
        engine,
        &agent_origin,
        workspace_id,
        index,
        0,
        surface_id,
        renamed_name.clone(),
        renamed_subtitle.clone(),
        renamed_description.clone(),
    );

    // S-WSCAT — 카테고리 소속 설정(있으면). 미지정이면 normal(생성자 기본값) 유지.
    // 카테고리 변경은 사용자 active 에 닿지 않는다(원칙 1·3). attach_mapping 과 동형으로
    // 직접 set + dirty.
    match resolve_category_param(engine, params) {
        Ok(Some(cat_id)) => {
            engine.workspaces[index].set_category(cat_id);
            engine.mark_layout_dirty();
        }
        Ok(None) => {}
        Err(msg) => return JsonRpcResponse::invalid_params(id, msg),
    }

    // 단계 7 — SSH attach 매핑 설정(있으면). layout.json 영속을 위해 dirty 표시.
    if let Some(mapping) = parse_attach_mapping(params) {
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
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    // release: self(loopback) attach 매핑 입력단 거부(원칙 1 ②).
    #[cfg(not(debug_assertions))]
    if let Some(resp) = reject_loopback_attach(params, &id) {
        return resp;
    }
    // workspace_id resolve — `id` 우선, 없으면 `index` 로 lookup.
    let workspace_id = if let Some(ws_id) = params.get("id").and_then(|v| v.as_u64()) {
        ws_id as u32
    } else if let Some(i) = params.get("index").and_then(|v| v.as_u64()) {
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

    crate::app::dispatch_domain::cascade_workspace_meta_updated(
        state,
        workspace_id,
        name,
        subtitle,
        description,
    );

    // S-WSCAT — 카테고리 소속 변경(있으면). 사용자 active 불변(원칙 1·3).
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

    // 단계 7 — SSH attach 매핑 갱신/해제. `attach_clear` 가 우선(해제), 아니면 파싱한
    // 매핑이 있으면 설정. 어느 쪽이든 layout.json 영속을 위해 dirty 표시.
    let clear = params
        .get("attach_clear")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if clear {
        engine.workspaces[index].set_attach_mapping(None);
        engine.mark_layout_dirty();
    } else if let Some(mapping) = parse_attach_mapping(params) {
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

pub fn handle_workspace_move(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let from = match params.get("from_index").and_then(|v| v.as_u64()) {
        Some(f) => f as usize,
        None => return JsonRpcResponse::invalid_params(id, "Missing 'from_index' parameter"),
    };
    let to = match params.get("to_index").and_then(|v| v.as_u64()) {
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
        crate::app::dispatch_domain::cascade_workspace_moved(state, from, to);
    }
    JsonRpcResponse::success(id, json!({ "moved": moved }))
}

// workspace.select removed: focus is user-only (shortcuts/clicks).
