//! 자식 터미널의 생성·입력·조회·종료를 관리한다. 에이전트별 명령 구성과 훅은 플러그인이 맡는다.
//! 탭 생성·입력·닫기 등은 같은 프로세스의 공용 핸들러를 호출한다.
//! 부모·자식 관계와 soft 점유를 함께 관리한다(ADR-0021).

mod spawn_transaction;

use serde_json::{Value, json};

use crate::core::child_terminal::ChildEntry;
use crate::core::state::child_liveness::ChildLiveness;
use tasty_ipc::protocol::JsonRpcResponse;

use super::{surface, tab};

type Core = crate::core::Core;
type CoreState = crate::core::CoreState;

use super::params::{optional_u32, require_u32};

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

fn require_str(params: &Value, key: &str, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), format!("missing '{key}'")))
}

/// child는 surface ID가 아니라 부모별 index다. 잘못 지정했으면 올바른 인자를 안내한다.
/// 두 번호가 겹칠 수 있으므로 surface ID로 자동 해석하지 않는다.
fn child_not_found_message(
    reg: &crate::core::child_terminal::ChildTerminalRegistry,
    parent: u32,
    given: u32,
) -> String {
    let base = format!("child {given} not found under surface {parent}");

    if let Some(owner) = reg.parent_of_child(given)
        && let Some(index) = reg
            .list_children(owner)
            .iter()
            .find(|c| c.child_surface_id == given)
            .map(|c| c.index)
    {
        return if owner == parent {
            format!(
                "{base}. {given} is a child_surface_id, not a child index — use `--child {index}`"
            )
        } else {
            format!(
                "{base}. {given} is a child_surface_id under a different parent \
                 — use `--surface {owner} --child {index}`"
            )
        };
    }

    let mut indices: Vec<u32> = reg.list_children(parent).iter().map(|c| c.index).collect();
    if indices.is_empty() {
        return format!("{base} (no children registered under surface {parent})");
    }
    indices.sort_unstable();
    let count = indices.len();
    format!(
        "{base} (valid child indices: {}; {count} children)",
        format_index_ranges(&indices)
    )
}

/// 정렬된 index를 연속 구간으로 압축하되 빠진 번호를 포함하지 않는다.
fn format_index_ranges(sorted: &[u32]) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sorted.len() {
        let start = sorted[i];
        let mut end = start;
        while i + 1 < sorted.len() && sorted[i + 1] == end + 1 {
            i += 1;
            end = sorted[i];
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}-{end}")
        });
        i += 1;
    }
    parts.join(", ")
}

/// `--surface`(parent) 명시가 없으면 유일 parent 로 폴백. 0 또는 2+ 면 에러(호출자 명시 요구).
fn resolve_parent(engine: &CoreState, params: &Value, id: &Value) -> Result<u32, JsonRpcResponse> {
    if let Some(p) = optional_u32(params, "surface", id)? {
        return Ok(p);
    }
    engine.child_terminals.single_parent().ok_or_else(|| {
        JsonRpcResponse::invalid_params(
            id.clone(),
            "missing 'surface' parameter (0 or >1 parents — specify --surface)".to_string(),
        )
    })
}

/// in-process sibling 핸들러 응답을 `Result<Value, JsonRpcResponse>` 로 변환.
/// error 응답은 caller 의 id 를 유지한 채 그대로 전파한다.
fn unwrap_ok(resp: JsonRpcResponse, id: &Value) -> Result<Value, JsonRpcResponse> {
    match (resp.result, resp.error) {
        (Some(r), _) => Ok(r),
        (_, Some(e)) => Err(JsonRpcResponse::error(id.clone(), e.code, e.message)),
        _ => Err(JsonRpcResponse::internal_error(
            id.clone(),
            "sibling handler returned empty response".to_string(),
        )),
    }
}

/// 멀티라인 본문을 bracketed paste로 감싼다. 제출용 CR은 호출자가 따로 보낸다.
fn build_tell_payload(message: &str) -> String {
    if message.contains('\n') {
        format!("\u{1b}[200~{message}\u{1b}[201~")
    } else {
        message.to_string()
    }
}

/// 끝의 CR을 본문에서 분리해 별도 전송하게 한다. CR이 없으면 제출하지 않는다.
fn build_broadcast_payload(text: &str) -> (String, bool) {
    let submit = text.ends_with('\r');
    let body = text.trim_end_matches('\r');
    (build_tell_payload(body), submit)
}

/// 본문 ack와 제출 CR의 IPC 대기 상한. 기다리는 동안 메인 루프를 막지 않도록 별도 스레드를 쓴다.
/// ack가 오지 않아도 상한 뒤에는 CR 전송을 시도한다. 이 상한이 TUI의 입력 처리를 보장하지는 않는다.
const TELL_SUBMIT_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// ack 뒤 CR 전송 전 대기 시간. ack는 PTY 쓰기 완료이지 자식 TUI의 읽기 완료가 아니다.
/// 초기 측정에서 5/10ms는 간헐 실패했고 20ms에서 통과해 선택했다.
/// 입력을 한 번에 읽는 문제를 줄이는 지연이며 모든 실행 환경의 제출 성공을 보장하지는 않는다.
/// 메인 스레드가 아닌 ack 대기 스레드에서만 적용한다.
const TELL_SUBMIT_EXTRA_SETTLE_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

/// 원격 점유로 막힌 입력과 없는 surface를 구분한다.
enum SendTextError {
    HardOccupied,
    NotFound,
}

/// 점유를 확인하고 필요한 초기화를 거쳐 ack를 받을 수 있는 방식으로 본문을 보낸다.
fn send_text_to_surface_with_ack(
    engine: &mut CoreState,
    surface_id: u32,
    text: &str,
) -> Result<tasty_terminal::WriteAck, SendTextError> {
    if engine.attach.is_hard_occupied(surface_id) {
        return Err(SendTextError::HardOccupied);
    }
    engine.ensure_surface_initialized(surface_id);
    engine
        .find_terminal_by_id_mut(surface_id)
        .map(|terminal| terminal.send_key_with_ack(text))
        .ok_or(SendTextError::NotFound)
}

/// 본문을 보내고 별도 스레드에서 ack 대기와 제출 CR 주입을 수행한다.
/// CR도 surface.send를 거쳐 점유 검사를 받는다. ack 대기 만료 뒤에도 제출을 시도한다.
fn send_body_then_submit(
    engine: &mut CoreState,
    core: &Core,
    id: &Value,
    surface_id: u32,
    body: String,
) -> Result<(), JsonRpcResponse> {
    let ack = send_text_to_surface_with_ack(engine, surface_id, &body).map_err(|e| {
        let msg = match e {
            SendTextError::HardOccupied => format!(
                "Surface {surface_id} is attached elsewhere (hard-occupied) — release the \
                 attach or target a different surface"
            ),
            SendTextError::NotFound => format!("Surface {surface_id} not found"),
        };
        JsonRpcResponse::invalid_params(id.clone(), msg)
    })?;

    let injector = core.host_ipc_injector_arc().get().cloned();
    std::thread::spawn(move || {
        ack.wait(TELL_SUBMIT_ACK_TIMEOUT);
        std::thread::sleep(TELL_SUBMIT_EXTRA_SETTLE_DELAY);
        let Some(injector) = injector else {
            tracing::warn!(
                "terminal tell/spawn: host_ipc_injector unavailable — \\r submit for surface {surface_id} dropped"
            );
            return;
        };
        let params = json!({ "surface_id": surface_id, "text": "\r" });
        // 늦은 CR 재시도는 사용자의 다음 입력을 제출할 수 있어 실패를 기록하고 끝낸다.
        // 큐에서 기한이 지나면 실행하지 않지만 이미 시작된 명령은 중단할 수 없다.
        if let Err(e) = injector.dispatch("surface.send", params, TELL_SUBMIT_ACK_TIMEOUT) {
            tracing::warn!(
                "terminal tell/spawn: submit \\r re-injection failed (surface={surface_id}): {e}"
            );
        }
    });
    Ok(())
}

/// 숫자 ID를 먼저 찾고 없으면 실제 name과 비교한다.
fn resolve_workspace_id(engine: &CoreState, target: &str) -> Option<u32> {
    if let Ok(target_id) = target.parse::<u32>()
        && engine.workspaces.iter().any(|w| w.id == target_id)
    {
        return Some(target_id);
    }
    engine
        .workspaces
        .iter()
        .find(|w| w.name == target)
        .map(|w| w.id)
}

/// 표시 이름은 실제 name/ID와 다를 수 있어 실패 시 숫자 ID 조회 방법을 안내한다.
fn workspace_not_found_message(workspace_param: &str) -> String {
    format!(
        "workspace '{workspace_param}' not found — pass the numeric workspace id \
         (see `tasty list workspaces`), not a displayed name (a name shown in the UI \
         may not match the underlying id)"
    )
}

fn first_pane_in_workspace(engine: &CoreState, ws_id: u32) -> Option<u32> {
    let idx = engine.find_workspace_index_for_id(ws_id)?;
    engine.workspaces[idx]
        .pane_layout()
        .all_pane_ids()
        .into_iter()
        .next()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_spawn(
    core: &mut Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();

    let parent = match require_u32(params, "parent", &id) {
        Ok(p) => p,
        // `surface` alias 도 허용 — CLI 는 parent 를 caller surface 로 채운다.
        Err(_) => match require_u32(params, "surface", &id) {
            Ok(p) => p,
            Err(e) => return e,
        },
    };
    let workspace_param = match require_str(params, "workspace", &id) {
        Ok(w) => w,
        Err(e) => return e,
    };
    // command를 생략하면 자식만 등록한다. 플러그인은 받은 surface ID로 명령을 구성해 나중에 보낼 수 있다.
    let command = optional_str(params, "command");
    let pane_override = match optional_u32(params, "pane", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let cwd = optional_str(params, "cwd");
    let role = optional_str(params, "role");
    let nickname = optional_str(params, "nickname");

    let Some(ws_id) = resolve_workspace_id(engine, &workspace_param) else {
        return JsonRpcResponse::invalid_params(id, workspace_not_found_message(&workspace_param));
    };
    let pane_id = match pane_override {
        Some(p) => p,
        None => match first_pane_in_workspace(engine, ws_id) {
            Some(p) => p,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    format!("No panes in workspace {ws_id}"),
                );
            }
        },
    };
    // pane 지정이 workspace 인자를 덮을 수 있으므로 실제 생성 위치의 점유를 검사한다.
    // 원격 전달로 탭만 남는 일이 없도록 생성 전에 mirror 대상도 거절한다.
    if let Some(denied) = super::spawn_target_guard(engine, pane_id, &id) {
        return denied;
    }
    let ws_id = engine
        .find_workspace_index_for_pane(pane_id)
        .and_then(|i| engine.workspaces.get(i))
        .map(|w| w.id)
        .unwrap_or(ws_id);

    let index = engine.child_terminals.next_index_for(parent);
    let tab_name = format!("child{index}");
    let mut tab_params = json!({
        "pane_id": pane_id,
        "type": "terminal",
        "name": tab_name,
    });
    if let Some(c) = &cwd {
        tab_params["cwd"] = Value::String(c.clone());
    }
    let tab_resp = tab::handle_tab_create(core, window, engine, id.clone(), &tab_params);
    let tab_val = match unwrap_ok(tab_resp, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let Some(new_surface_id) = tab_val
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
    else {
        return JsonRpcResponse::internal_error(
            id,
            format!("tab.create response missing 'surface_id': {tab_val}"),
        );
    };

    if let Err(error) = spawn_transaction::finish(
        core,
        window,
        engine,
        &id,
        parent,
        ChildEntry {
            child_surface_id: new_surface_id,
            index,
            cwd,
            role,
            nickname,
        },
        command.as_deref(),
    ) {
        return error;
    }

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": new_surface_id,
            "child_index": index,
            "pane_id": pane_id,
            "workspace_id": ws_id,
        }),
    )
}

pub(crate) fn handle_tell(
    core: &mut Core,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let payload = build_tell_payload(&text);
    if let Err(e) = send_body_then_submit(engine, core, &id, surface_id, payload) {
        return e;
    }
    if clear_idle_for_new_prompt(&mut engine.child_terminals, surface_id) {
        engine.child_terminals.save();
    }
    JsonRpcResponse::success(id, json!({ "sent": true, "surface_id": surface_id }))
}

/// 입력을 보낸 등록 자식의 idle/needs_input을 해제한다. 이전 턴의 idle을 새 작업 완료로
/// 읽지 않도록 입력을 보낸 쪽에서 바로 갱신한다. 상태가 바뀌면 호출자가 저장한다.
/// 미등록 surface에는 잘못된 상태 보고 기록을 새로 만들지 않는다.
fn clear_idle_for_new_prompt(
    registry: &mut crate::core::child_terminal::ChildTerminalRegistry,
    surface_id: u32,
) -> bool {
    if registry.parent_of_child(surface_id).is_none() {
        return false;
    }
    registry.set_idle(surface_id, false);
    true
}

pub(crate) fn handle_children(
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let live = engine.live_surface_ids();
    let children: Vec<Value> = engine
        .child_terminals
        .list_children(parent)
        .iter()
        .map(|c| {
            let mut item =
                liveness_fields(engine.child_liveness_with_live(c.child_surface_id, &live));
            item.insert("index".into(), json!(c.index));
            item.insert("surface_id".into(), json!(c.child_surface_id));
            item.insert("role".into(), json!(c.role));
            item.insert("nickname".into(), json!(c.nickname));
            item.insert("cwd".into(), json!(c.cwd));
            Value::Object(item)
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "children": children }))
}

/// children/state 응답이 같은 state/evidence/confidence 필드를 사용한다.
/// 값의 조합은 docs/features/child-terminal/index.md의 판정 표를 따른다.
fn liveness_fields(liveness: ChildLiveness) -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("state".into(), json!(liveness.state.as_str()));
    fields.insert("evidence".into(), json!(liveness.evidence.as_str()));
    fields.insert("confidence".into(), json!(liveness.confidence.as_str()));
    fields
}

/// 명시한 자식 surface를 목록 조회와 같은 child_liveness 함수로 판정한다.
/// 관계가 사라져도 surface가 살아 있을 수 있어 실제 트리와 PTY 상태를 따로 확인한다.
pub(crate) fn handle_state(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut out = liveness_fields(engine.child_liveness(surface_id));
    out.insert("surface_id".into(), json!(surface_id));
    JsonRpcResponse::success(id, Value::Object(out))
}

pub(crate) fn handle_parent(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let child_surface = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match engine.child_terminals.parent_of_child(child_surface) {
        Some(parent_id) => JsonRpcResponse::success(
            id,
            json!({ "parent_surface_id": parent_id, "status": "active" }),
        ),
        None => JsonRpcResponse::success(
            id,
            json!({ "parent_surface_id": Value::Null, "status": "none" }),
        ),
    }
}

pub(crate) fn handle_kill(
    core: &mut Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(removed) = engine.child_terminals.remove_child(parent, child_index) else {
        return JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.child_terminals, parent, child_index),
        );
    };
    engine.child_terminals.save();

    engine.release_occupancy(removed.child_surface_id);

    let close_params = json!({ "surface_id": removed.child_surface_id });
    if let Err(e) = unwrap_ok(
        surface::handle_surface_close(core, window, engine, id.clone(), &close_params),
        &id,
    ) {
        return e;
    }
    JsonRpcResponse::success(
        id,
        json!({
            "killed_surface_id": removed.child_surface_id,
            "child_index": removed.index,
        }),
    )
}

/// surface를 닫지 않고 부모·자식 관계와 soft 점유를 해제한다. hard 점유는 건드리지 않는다.
/// soft 점유가 이미 없어도 관계 제거는 성공으로 처리한다.
pub(crate) fn handle_release(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(removed) = engine.child_terminals.remove_child(parent, child_index) else {
        return JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.child_terminals, parent, child_index),
        );
    };
    engine.child_terminals.save();

    if let Err(e) = engine.release_soft_occupancy(removed.child_surface_id, parent) {
        tracing::warn!(
            "terminal.release: soft occupancy release failed for surface {} \
             (parent {parent}): {e:?} — registry relationship removed anyway",
            removed.child_surface_id
        );
    }

    JsonRpcResponse::success(
        id,
        json!({
            "released_surface_id": removed.child_surface_id,
            "child_index": removed.index,
        }),
    )
}

/// 기존 surface를 자식으로 등록한다. 임의의 기존 대상이므로 soft 점유를 먼저 확보한다.
/// 실패하면 관계를 추가하지 않는다. 새 surface를 만드는 spawn은 관계 등록 후 점유하며
/// 실패 시 spawn_transaction이 이번에 만든 surface와 관계를 정리한다.
pub(crate) fn handle_adopt(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let target = match require_u32(params, "target", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };

    if engine.find_surface_by_id(target).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("surface {target} not found"));
    }
    if parent == target {
        return JsonRpcResponse::invalid_params(
            id,
            "cannot adopt a surface as its own child".to_string(),
        );
    }
    if let Some(existing_parent) = engine.child_terminals.parent_of_child(target) {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "surface {target} is already a child of {existing_parent} \
                 (release it first — see 'tasty terminal release')"
            ),
        );
    }
    // soft 점유 함수는 hard 점유를 검사하지 않으므로 여기서 먼저 거절한다.
    if engine.attach.is_hard_occupied(target) {
        return JsonRpcResponse::invalid_params(
            id,
            format!("surface {target} is hard-occupied (remote attach) — cannot adopt"),
        );
    }

    let role = optional_str(params, "role");
    let nickname = optional_str(params, "nickname");
    let cwd = optional_str(params, "cwd");
    let label = nickname.clone().or_else(|| role.clone());
    // Check the existing owner without changing its label or releasing its lock
    // if the following relationship commit fails.
    if let Some(occupancy) = engine.attach.occupancy_of(target)
        && occupancy.parent != Some(parent)
    {
        return JsonRpcResponse::error(id, -32020, "occupy_soft failed: another owner");
    }
    if let Err(error) = engine.occupy_soft(target, parent, label) {
        return JsonRpcResponse::error(id, -32020, format!("occupy_soft failed: {error:?}"));
    }
    let index = engine.child_terminals.next_index_for(parent);
    engine.child_terminals.register_child(
        parent,
        ChildEntry {
            child_surface_id: target,
            index,
            cwd,
            role,
            nickname,
        },
    );
    engine.child_terminals.save();

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": target,
            "child_index": index,
        }),
    )
}

pub(crate) fn handle_respawn(
    core: &mut Core,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(entry) = engine
        .child_terminals
        .find_child(parent, child_index)
        .cloned()
    else {
        return JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.child_terminals, parent, child_index),
        );
    };
    let new_cwd = optional_str(params, "cwd");
    let command = optional_str(params, "command");

    // cwd가 바뀌면 PTY를 교체하고, 아니면 Ctrl-C를 보낸다. Ctrl-C가 종료 완료를 보장하지는 않는다.
    if new_cwd.is_some() {
        let mut respawn_params = json!({ "surface_id": entry.child_surface_id });
        if let Some(c) = new_cwd.as_deref() {
            respawn_params["cwd"] = Value::String(c.to_string());
        }
        if let Err(e) = unwrap_ok(
            surface::handle_surface_respawn_terminal(core, engine, id.clone(), &respawn_params),
            &id,
        ) {
            return e;
        }
    } else {
        let combo = json!({
            "surface_id": entry.child_surface_id,
            "key": "c",
            "modifiers": ["ctrl"],
        });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send_combo(core, engine, id.clone(), &combo),
            &id,
        ) {
            return e;
        }
    }
    if let Some(cmd) = &command {
        let send_params = json!({ "surface_id": entry.child_surface_id, "text": cmd });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send(core, engine, id.clone(), &send_params),
            &id,
        ) {
            return e;
        }
    }

    let new_role = optional_str(params, "role");
    let new_nick = optional_str(params, "nickname");
    engine
        .child_terminals
        .update_child(parent, child_index, |e| {
            if let Some(r) = new_role {
                e.role = Some(r);
            }
            if let Some(n) = new_nick {
                e.nickname = Some(n);
            }
            if let Some(c) = new_cwd {
                e.cwd = Some(c);
            }
        });
    engine
        .child_terminals
        .set_idle(entry.child_surface_id, false);
    engine.child_terminals.save();
    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": entry.child_surface_id,
            "child_index": entry.index,
        }),
    )
}

/// 본문을 보내고 끝의 CR이 있으면 별도 write로 제출한다.
/// 본문 전송 실패는 해당 자식만 건너뛰며, 성공한 자식의 상태만 갱신한다.
#[allow(clippy::too_many_arguments)]
fn send_broadcast_to_child(
    core: &mut Core,
    engine: &mut CoreState,
    id: &Value,
    sid: u32,
    body: &str,
    submit: bool,
) -> bool {
    let body_params = json!({ "surface_id": sid, "text": body });
    if let Err(e) = unwrap_ok(
        surface::handle_surface_send(core, engine, id.clone(), &body_params),
        id,
    ) {
        tracing::warn!("terminal.broadcast surface.send (sid={sid}) failed: {e:?}");
        return false;
    }
    if submit {
        let cr_params = json!({ "surface_id": sid, "text": "\r" });
        if let Err(e) = unwrap_ok(
            surface::handle_surface_send(core, engine, id.clone(), &cr_params),
            id,
        ) {
            tracing::warn!("terminal.broadcast submit (sid={sid}) failed: {e:?}");
        }
    }
    true
}

pub(crate) fn handle_broadcast(
    core: &mut Core,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let parent = match resolve_parent(engine, params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let role_filter = optional_str(params, "role");

    let targets: Vec<u32> = engine
        .child_terminals
        .list_children(parent)
        .iter()
        .filter(|c| match &role_filter {
            Some(r) => c.role.as_deref() == Some(r.as_str()),
            None => true,
        })
        .map(|c| c.child_surface_id)
        .collect();

    let (body, submit) = build_broadcast_payload(&text);
    let mut sent_ids: Vec<u32> = Vec::new();
    let mut idle_cleared = false;
    for sid in targets {
        if send_broadcast_to_child(core, engine, &id, sid, &body, submit) {
            idle_cleared |= clear_idle_for_new_prompt(&mut engine.child_terminals, sid);
        }
        sent_ids.push(sid);
    }
    if idle_cleared {
        engine.child_terminals.save();
    }
    JsonRpcResponse::success(
        id,
        json!({ "sent_count": sent_ids.len(), "children": sent_ids }),
    )
}

/// 훅은 idle/needs_input/active만 보고할 수 있다.
/// exited/stale은 호스트가 트리·PTY에서 판정하는 출력 상태이므로 입력으로 받지 않는다.
pub(crate) fn handle_set_state(
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let new_state = match require_str(params, "state", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if !matches!(new_state.as_str(), "idle" | "active" | "needs_input") {
        return JsonRpcResponse::invalid_params(
            id,
            format!("unknown state '{new_state}' (supported: idle, needs_input, active)"),
        );
    }
    match new_state.as_str() {
        "idle" => engine.child_terminals.set_idle(surface_id, true),
        "active" => engine.child_terminals.set_idle(surface_id, false),
        "needs_input" => engine.child_terminals.set_needs_input(surface_id, true),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown state '{other}' (supported: idle, needs_input, active)"),
            );
        }
    }
    engine.child_terminals.save();
    JsonRpcResponse::success(
        id,
        json!({ "ok": true, "surface_id": surface_id, "state": new_state }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::attach::OccupancyTier;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn child(sid: u32, index: u32) -> ChildEntry {
        ChildEntry {
            child_surface_id: sid,
            index,
            cwd: None,
            role: None,
            nickname: None,
        }
    }

    fn ok(resp: JsonRpcResponse) -> Value {
        resp.result.expect("expected success result")
    }

    #[test]
    fn index_ranges_compress_contiguous_runs() {
        assert_eq!(format_index_ranges(&[0, 1, 2, 3]), "0-3");
        assert_eq!(format_index_ranges(&[7]), "7");
        assert_eq!(format_index_ranges(&[0, 1, 2, 5, 8, 9]), "0-2, 5, 8-9");
        assert_eq!(format_index_ranges(&[0, 57]), "0, 57");
    }

    #[test]
    fn workspace_not_found_message_hints_numeric_id() {
        let msg = workspace_not_found_message("1");
        assert!(msg.contains("workspace '1' not found"), "{msg}");
        assert!(msg.contains("numeric workspace id"), "{msg}");
        assert!(msg.contains("tasty list workspaces"), "{msg}");
    }

    #[test]
    fn child_not_found_points_at_index_when_given_a_surface_id() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(3157, child(3204, 31));
        let msg = child_not_found_message(&reg, 3157, 3204);
        assert!(msg.contains("child_surface_id, not a child index"), "{msg}");
        assert!(msg.contains("`--child 31`"), "{msg}");
    }

    #[test]
    fn child_not_found_points_at_other_parent() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(9000, child(3204, 4));
        let msg = child_not_found_message(&reg, 3157, 3204);
        assert!(msg.contains("under a different parent"), "{msg}");
        assert!(msg.contains("`--surface 9000 --child 4`"), "{msg}");
    }

    #[test]
    fn child_not_found_lists_valid_indices() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        for i in 0..3 {
            reg.register_child(3157, child(100 + i, i));
        }
        let msg = child_not_found_message(&reg, 3157, 999);
        assert!(msg.contains("valid child indices: 0-2"), "{msg}");
        assert!(msg.contains("3 children"), "{msg}");
    }

    #[test]
    fn child_not_found_reports_empty_parent() {
        let reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        let msg = child_not_found_message(&reg, 3157, 0);
        assert!(msg.contains("no children registered"), "{msg}");
    }

    #[test]
    fn tell_clears_idle_flag_on_target_child() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(10, child(50, 0));
        reg.set_idle(50, true);
        assert_eq!(reg.state_of(50), "idle");

        assert!(clear_idle_for_new_prompt(&mut reg, 50));
        assert_eq!(reg.state_of(50), "active");
    }

    #[test]
    fn tell_clears_needs_input_too() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(10, child(50, 0));
        reg.set_needs_input(50, true);
        assert_eq!(reg.state_of(50), "needs_input");

        assert!(clear_idle_for_new_prompt(&mut reg, 50));
        assert_eq!(reg.state_of(50), "active");
    }

    #[test]
    fn tell_does_not_touch_registry_for_non_child_surface() {
        let mut reg = crate::core::child_terminal::ChildTerminalRegistry::default();
        assert!(!clear_idle_for_new_prompt(&mut reg, 777));
        assert_eq!(reg.state_of(777), "active");
        assert_eq!(reg.last_state_report_at(777), None);
    }

    // adopt의 존재 확인은 surface 트리를 보므로 실제 PTY 대신 표시용 surface만 추가한다.
    fn add_extra_surface(e: &mut CoreState, surface_id: u32) {
        let pane_id = e.workspaces[0].pane_layout().all_pane_ids()[0];
        let tab = crate::model::Tab::new_with_surface(
            e.next_ids.next_tab(),
            "extra".to_string(),
            Box::new(crate::model::TerminalSurface { id: surface_id }),
        );
        e.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("default pane")
            .tabs
            .push(tab);
    }

    #[test]
    fn spawn_kill_occupancy_wiring() {
        // 점유와 관계 등록만 검사한다. 탭 생성·입력 핸들러 전체 시험은 아니다.
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5000u32;

        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();
        let occ = e.attach.occupancy_of(c).expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent));
        assert!(!e.attach.is_hard_occupied(c));

        assert!(e.child_terminals.remove_child(parent, idx).is_some());
        e.release_occupancy(c);
        assert!(e.attach.occupancy_of(c).is_none());
    }

    #[test]
    fn adopt_registers_existing_surface_without_new_tab() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6101u32; // 이미 존재하는(=spawn 아닌) surface
        add_extra_surface(&mut e, target);

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_none());

        let occ = e
            .attach
            .occupancy_of(target)
            .expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent));
        assert_eq!(e.child_terminals.parent_of_child(target), Some(parent));
    }

    #[test]
    fn adopt_rejects_already_registered_child() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6102u32;
        add_extra_surface(&mut e, target);

        let _ = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        let resp2 = handle_adopt(
            &mut e,
            json!(2),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp2.error.is_some()); // 중복 등록 거부
    }

    #[test]
    fn adopt_rejects_nonexistent_surface() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": 999999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_self_adoption() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": parent }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_hard_occupied_target_and_leaves_registry_unchanged() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 6103u32;
        add_extra_surface(&mut e, target);
        e.attach
            .acquire(target, /* hard occupancy client id */ 1)
            .unwrap();

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_some());
        assert!(e.child_terminals.parent_of_child(target).is_none());
        assert!(e.attach.is_hard_occupied(target)); // 기존 hard 점유는 건드리지 않음
    }

    #[test]
    fn release_clears_registry_and_occupancy_but_keeps_surface() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5701u32;
        add_extra_surface(&mut e, c);

        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());

        assert!(e.child_terminals.find_child(parent, idx).is_none());
        assert!(e.attach.occupancy_of(c).is_none());
    }

    #[test]
    fn release_rejects_unregistered_child_index() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": 999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn release_does_not_touch_unrelated_hard_occupancy() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let c = 5702u32;
        add_extra_surface(&mut e, c);
        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, None).unwrap();
        let other = 5703u32;
        e.attach.acquire(other, 1).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());
        assert!(e.attach.occupancy_of(c).is_none()); // soft 는 정상 해제
        assert!(e.attach.is_hard_occupied(other)); // 무관한 hard 점유는 그대로
    }

    // 이 시험은 단일 engine의 부모 선택만 확인한다. 창 간 모호성은 App::find_request_owner에서 거절한다.
    #[test]
    fn resolve_parent_omitted_surface_succeeds_with_single_parent() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        add_extra_surface(&mut e, 59010);
        let idx = e.child_terminals.next_index_for(parent);
        e.child_terminals.register_child(parent, child(59010, idx));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx }));
        assert!(resp.error.is_none(), "{:?}", resp.error);
    }

    #[test]
    fn resolve_parent_omitted_surface_errors_with_multiple_parents_in_one_engine() {
        let mut e = engine();
        let parent1 = e.workspaces[0].all_surface_ids()[0];
        let parent2 = 59020u32;
        add_extra_surface(&mut e, 59030);
        add_extra_surface(&mut e, 59040);
        let idx1 = e.child_terminals.next_index_for(parent1);
        e.child_terminals
            .register_child(parent1, child(59030, idx1));
        let idx2 = e.child_terminals.next_index_for(parent2);
        e.child_terminals
            .register_child(parent2, child(59040, idx2));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx1 }));
        assert!(resp.error.is_some());
    }

    #[test]
    fn send_text_with_ack_distinguishes_hard_occupied_from_not_found() {
        let mut e = engine();
        let target = 5801u32;
        add_extra_surface(&mut e, target);
        e.attach.acquire(target, 1).unwrap();

        let err = send_text_to_surface_with_ack(&mut e, target, "hi")
            .err()
            .expect("hard-occupied surface must fail");
        assert!(matches!(err, SendTextError::HardOccupied));
    }

    #[test]
    fn send_text_with_ack_reports_not_found_for_missing_surface() {
        let mut e = engine();
        let err = send_text_to_surface_with_ack(&mut e, 424_242, "hi")
            .err()
            .expect("missing surface must fail");
        assert!(matches!(err, SendTextError::NotFound));
    }

    // add_extra_surface는 PTY가 없으므로 여기서는 실제 터미널이 있는 기본 surface를 쓴다.
    #[test]
    fn send_text_with_ack_succeeds_for_free_terminal() {
        let mut e = engine();
        let target = e.workspaces[0].all_surface_ids()[0];

        assert!(send_text_to_surface_with_ack(&mut e, target, "hi").is_ok());
    }

    #[test]
    fn children_reconcile_prunes_dead_child() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        e.child_terminals.register_child(parent, child(99999, 0));
        let resp = handle_children(&mut e, json!(1), &json!({ "surface": parent }));
        let n = ok(resp)["children"].as_array().unwrap().len();
        assert_eq!(n, 0);
    }

    #[test]
    fn children_item_carries_evidence_and_confidence() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 5901u32;
        add_extra_surface(&mut e, target);
        e.child_terminals.register_child(parent, child(target, 0));

        let resp = ok(handle_children(
            &mut e,
            json!(1),
            &json!({ "surface": parent }),
        ));
        let item = &resp["children"].as_array().expect("children")[0];
        for key in ["state", "evidence", "confidence"] {
            assert!(
                item.get(key).and_then(|v| v.as_str()).is_some(),
                "children 항목에 '{key}' 가 문자열로 실려야 한다: {item}"
            );
        }
    }

    #[test]
    fn children_and_state_report_identical_liveness_fields() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0];
        let target = 5902u32;
        add_extra_surface(&mut e, target);
        e.child_terminals.register_child(parent, child(target, 0));

        let list = ok(handle_children(
            &mut e,
            json!(1),
            &json!({ "surface": parent }),
        ));
        let item = list["children"].as_array().expect("children")[0].clone();
        let single = ok(handle_state(
            &mut e,
            json!(2),
            &json!({ "surface": target }),
        ));

        for key in ["state", "evidence", "confidence"] {
            assert_eq!(
                item[key], single[key],
                "'{key}' 가 목록과 단건에서 갈렸다: list={item}, single={single}"
            );
        }
    }

    #[test]
    fn parent_lookup_unregistered_is_none() {
        let mut e = engine();
        let resp = handle_parent(&mut e, json!(1), &json!({ "surface": 424242 }));
        assert_eq!(ok(resp)["status"], "none");
    }

    #[test]
    fn set_state_rejects_unknown() {
        let mut e = engine();
        let resp = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "bogus" }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn set_state_updates_registry() {
        let mut e = engine();
        e.child_terminals.register_child(7, child(5000, 0));
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "idle" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "idle");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "needs_input" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "needs_input");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "active" }),
        );
        assert_eq!(e.child_terminals.state_of(5000), "active");
    }

    // needs_input 뒤 idle만 보고되어도 이전 입력 대기 상태를 지워야 한다.
    #[test]
    fn set_state_idle_clears_a_pending_needs_input() {
        let mut e = engine();
        e.child_terminals.register_child(7, child(5001, 0));
        fn push_child_state(e: &mut CoreState, state: &str) {
            let resp = handle_set_state(e, json!(1), &json!({ "surface": 5001, "state": state }));
            assert!(resp.error.is_none(), "{state} 주입이 거부됐다");
        }
        push_child_state(&mut e, "needs_input");
        assert_eq!(e.child_terminals.state_of(5001), "needs_input");
        push_child_state(&mut e, "idle");
        assert_eq!(e.child_terminals.state_of(5001), "idle");
    }

    #[test]
    fn broadcast_payload_splits_trailing_cr_for_submit() {
        let body63 = "a".repeat(63);
        let (payload, submit) = build_broadcast_payload(&format!("{body63}\r"));
        assert_eq!(payload, body63, "본문에 제출 \\r 이 섞이면 안 됨");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }

    #[test]
    fn broadcast_payload_no_trailing_cr_is_injection_only() {
        let (payload, submit) = build_broadcast_payload(&"b".repeat(80));
        assert_eq!(payload, "b".repeat(80));
        assert!(!submit);
    }

    #[test]
    fn broadcast_payload_multiline_wraps_bracketed_and_submits() {
        let (payload, submit) = build_broadcast_payload("line1\nline2\r");
        assert_eq!(payload, "\u{1b}[200~line1\nline2\u{1b}[201~");
        assert!(!payload.contains('\r'));
        assert!(submit);
    }
}
