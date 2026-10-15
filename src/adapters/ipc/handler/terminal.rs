//! 자식 터미널의 생성·입력·조회·종료를 관리한다. 에이전트별 명령 구성과 훅은 플러그인이 맡는다.
//! 구조 생성·닫기는 App의 journal admission에서 처리하며 여기에는 입력·관계 서비스가 남는다.
//! 부모·자식 관계와 soft 점유를 함께 관리한다(ADR-0021).

mod fixed_input;
use crate::runtime::engine_access::{EngineMut, EngineRef};
use fixed_input::respawn_options;
pub(crate) use fixed_input::{input_response, prepare_input};
use serde_json::{Value, json};

use crate::core::state::child_liveness::ChildLiveness;
use crate::runtime::child_terminal::{ChildEntry, ParentKey};
use tasty_ipc::protocol::JsonRpcResponse;

use super::params::{optional_parent, require_u32};

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

/// 부모를 사람이 읽는 형태로 쓴다. Surface 부모 문구는 이전 u32 부모와 같다.
fn parent_label(parent: &ParentKey) -> String {
    match parent {
        ParentKey::Surface(id) => format!("surface {id}"),
        ParentKey::External(key) => format!("parent_external {key}"),
    }
}

/// child는 surface ID가 아니라 부모별 index다. 잘못 지정했으면 올바른 인자를 안내한다.
/// 두 번호가 겹칠 수 있으므로 surface ID로 자동 해석하지 않는다.
fn child_not_found_message(
    reg: &crate::runtime::child_terminal::ChildTerminalRegistry,
    parent: &ParentKey,
    given: u32,
) -> String {
    let base = format!("child {given} not found under {}", parent_label(parent));

    if let Some(owner) = reg.parent_of_child(given)
        && let Some(index) = reg
            .list_children(&owner)
            .iter()
            .find(|c| c.child_surface_id == given)
            .map(|c| c.index)
    {
        return if &owner == parent {
            format!(
                "{base}. {given} is a child_surface_id, not a child index — use `--child {index}`"
            )
        } else {
            let target = match &owner {
                ParentKey::Surface(owner) => format!("--surface {owner}"),
                ParentKey::External(owner) => format!("--parent-external {owner}"),
            };
            format!(
                "{base}. {given} is a child_surface_id under a different parent \
                 — use `{target} --child {index}`"
            )
        };
    }

    let mut indices: Vec<u32> = reg.list_children(parent).iter().map(|c| c.index).collect();
    if indices.is_empty() {
        return format!(
            "{base} (no children registered under {})",
            parent_label(parent)
        );
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

/// `--surface`(parent) 또는 `parent_external` 명시가 없으면 유일 Surface parent 로 폴백한다.
/// External 부모는 폴백 후보가 아니다. 0 또는 2+ 면 에러(호출자 명시 요구).
fn resolve_parent(
    engine: &EngineRef<'_>,
    params: &Value,
    id: &Value,
) -> Result<ParentKey, JsonRpcResponse> {
    if let Some(p) = optional_parent(params, "surface", id)? {
        return Ok(p);
    }
    engine
        .runtime
        .child_terminals
        .single_parent(&engine.core.live_surface_ids())
        .map(ParentKey::Surface)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(
                id.clone(),
                "missing 'surface' parameter (0 or >1 parents — specify --surface)".to_string(),
            )
        })
}

/// 본문 제출 거절을 기존 오류 코드와 문구로 바꾼다.
fn submit_rejected_response(
    id: Value,
    surface_id: u32,
    rejected: crate::runtime::pending_submit::SubmitRejected,
) -> JsonRpcResponse {
    use crate::runtime::pending_submit::SubmitRejected;
    match rejected {
        SubmitRejected::QueueFull => JsonRpcResponse::error(
            id,
            tasty_ipc::protocol::ERR_COMMAND_QUEUE_FULL,
            "terminal submission queue is full",
        ),
        SubmitRejected::NotFound => {
            JsonRpcResponse::invalid_params(id, format!("Surface {surface_id} not found"))
        }
        SubmitRejected::HardOccupied => JsonRpcResponse::invalid_params(
            id,
            format!(
                "Surface {surface_id} is attached elsewhere (hard-occupied) — release the \
                 attach or target a different surface"
            ),
        ),
    }
}

fn decode_tell<'a>(params: &'a Value, id: &Value) -> Result<(u32, &'a str), JsonRpcResponse> {
    let surface = require_u32(params, "surface", id)?;
    let text = params
        .get("text")
        .and_then(|value| value.as_str())
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), "missing 'text'"))?;
    Ok((surface, text))
}

pub(crate) fn handle_tell(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let (surface_id, text) = match decode_tell(params, &id) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let result = crate::app::terminal_input::tell(&mut engine.terminal_input(), surface_id, text);
    input_response(&engine.as_ref(), id, result)
}

pub(crate) fn handle_children(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(&engine.as_ref(), params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let live = engine.live_surface_ids();
    let children: Vec<Value> = engine
        .runtime
        .child_terminals
        .list_children(&parent)
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
pub(crate) fn handle_state(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let surface_id = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut out = liveness_fields(engine.child_liveness(surface_id));
    out.insert("surface_id".into(), json!(surface_id));
    JsonRpcResponse::success(id, Value::Object(out))
}

pub(crate) fn handle_parent(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let child_surface = match require_u32(params, "surface", &id) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match engine
        .runtime
        .child_terminals
        .parent_of_child(child_surface)
    {
        Some(parent) => JsonRpcResponse::success(
            id,
            json!({
                "parent_surface_id": parent.surface(),
                "parent_external": parent.external(),
                "status": "active",
            }),
        ),
        None => JsonRpcResponse::success(
            id,
            json!({
                "parent_surface_id": Value::Null,
                "parent_external": Value::Null,
                "status": "none",
            }),
        ),
    }
}

pub(crate) fn handle_release(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(&engine.as_ref(), params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let Some(removed) = engine
        .runtime
        .child_terminals
        .remove_child(&parent, child_index)
    else {
        return JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.runtime.child_terminals, &parent, child_index),
        );
    };
    engine.runtime.child_terminals.save();

    if let Err(e) = engine.release_soft_occupancy(removed.child_surface_id, parent.clone()) {
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
/// 기존 surface에 자식 관계와 soft 점유를 연결한다. 구조 생성은 수행하지 않는다.
pub(crate) fn handle_adopt(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(&engine.as_ref(), params, &id) {
        Ok(ParentKey::Surface(p)) => p,
        Ok(ParentKey::External(_)) => {
            return JsonRpcResponse::invalid_params(
                id,
                "terminal.adopt does not accept 'parent_external'",
            );
        }
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
    if let Some(existing_parent) = engine.runtime.child_terminals.parent_of_child(target) {
        return JsonRpcResponse::invalid_params(
            id,
            format!(
                "surface {target} is already a child of {existing_parent} \
                 (release it first — see 'tasty terminal release')"
            ),
        );
    }
    // soft 점유 함수는 hard 점유를 검사하지 않으므로 여기서 먼저 거절한다.
    if engine.live.occupancy.is_hard_occupied(target) {
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
    if let Some(occupancy) = engine.live.occupancy.occupancy_of(target)
        && occupancy.parent != Some(ParentKey::Surface(parent))
    {
        return JsonRpcResponse::error(id, -32020, "occupy_soft failed: another owner");
    }
    if let Err(error) = engine.occupy_soft(target, parent, label) {
        return JsonRpcResponse::error(id, -32020, format!("occupy_soft failed: {error:?}"));
    }
    let index = engine.runtime.child_terminals.next_index_for(parent);
    engine.runtime.child_terminals.register_child(
        parent,
        ChildEntry {
            child_surface_id: target,
            index,
            cwd,
            role,
            nickname,
        },
    );
    engine.runtime.child_terminals.save();

    JsonRpcResponse::success(
        id,
        json!({
            "child_surface_id": target,
            "child_index": index,
        }),
    )
}

pub(crate) fn handle_respawn(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    engine.reconcile_child_terminals();
    let parent = match resolve_parent(&engine.as_ref(), params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let child_index = match require_u32(params, "child", &id) {
        Ok(c) => c,
        Err(e) => return e,
    };
    if engine
        .runtime
        .child_terminals
        .find_child(&parent, child_index)
        .is_none()
    {
        return JsonRpcResponse::invalid_params(
            id,
            child_not_found_message(&engine.runtime.child_terminals, &parent, child_index),
        );
    }
    // cwd가 바뀌면 PTY를 교체해야 하므로 구조 admission이 맡는다.
    if optional_str(params, "cwd").is_some() {
        return JsonRpcResponse::internal_error(
            id,
            "child resource respawn requires committed structure admission",
        );
    }
    let options = respawn_options(params);
    let result = crate::app::terminal_input::respawn(
        &mut engine.terminal_input(),
        &parent,
        child_index,
        &options,
    );
    input_response(&engine.as_ref(), id, result)
}

pub(crate) fn handle_broadcast(
    engine: &mut EngineMut<'_>,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let parent = match resolve_parent(&engine.as_ref(), params, &id) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let role_filter = optional_str(params, "role");

    let targets: Vec<u32> = engine
        .runtime
        .child_terminals
        .list_children(&parent)
        .iter()
        .filter(|c| match &role_filter {
            Some(r) => c.role.as_deref() == Some(r.as_str()),
            None => true,
        })
        .map(|c| c.child_surface_id)
        .collect();
    let result =
        crate::app::terminal_input::broadcast(&mut engine.terminal_input(), targets, &text);
    input_response(&engine.as_ref(), id, result)
}

/// 훅은 idle/needs_input/active만 보고할 수 있다.
/// exited/stale은 호스트가 트리·PTY에서 판정하는 출력 상태이므로 입력으로 받지 않는다.
pub(crate) fn handle_set_state(
    engine: &mut EngineMut<'_>,
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
        "idle" => engine.runtime.child_terminals.set_idle(surface_id, true),
        "active" => engine.runtime.child_terminals.set_idle(surface_id, false),
        "needs_input" => engine
            .runtime
            .child_terminals
            .set_needs_input(surface_id, true),
        other => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("unknown state '{other}' (supported: idle, needs_input, active)"),
            );
        }
    }
    engine.runtime.child_terminals.save();
    JsonRpcResponse::success(
        id,
        json!({ "ok": true, "surface_id": surface_id, "state": new_state }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::attach::OccupancyTier;

    fn engine() -> crate::runtime::engine_session::EngineSession {
        engine_with_extra(&[])
    }
    fn engine_with_extra(ids: &[u32]) -> crate::runtime::engine_session::EngineSession {
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        let mut events = vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "workspace".into(),
                category: 0,
                index: 0,
                pane: 1,
            },
        ];
        for (index, id) in std::iter::once(1).chain(ids.iter().copied()).enumerate() {
            events.push(E::TabCreated {
                id,
                pane: 1,
                index,
                name: "terminal".into(),
                surface: SurfaceSpec {
                    id,
                    kind: "terminal".into(),
                    data: None,
                },
            });
        }
        let mut owner =
            crate::state::tests::test_state_from_model(crate::state::tests::test_model(events)).1;
        for id in ids {
            drop(owner.runtime.terminals.remove(*id));
        }
        owner
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
    fn child_not_found_points_at_index_when_given_a_surface_id() {
        let mut reg = crate::runtime::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(3157, child(3204, 31));
        let msg = child_not_found_message(&reg, &3157.into(), 3204);
        assert!(msg.contains("child_surface_id, not a child index"), "{msg}");
        assert!(msg.contains("`--child 31`"), "{msg}");
    }

    #[test]
    fn child_not_found_points_at_other_parent() {
        let mut reg = crate::runtime::child_terminal::ChildTerminalRegistry::default();
        reg.register_child(9000, child(3204, 4));
        let msg = child_not_found_message(&reg, &3157.into(), 3204);
        assert!(msg.contains("under a different parent"), "{msg}");
        assert!(msg.contains("`--surface 9000 --child 4`"), "{msg}");
    }

    #[test]
    fn child_not_found_lists_valid_indices() {
        let mut reg = crate::runtime::child_terminal::ChildTerminalRegistry::default();
        for i in 0..3 {
            reg.register_child(3157, child(100 + i, i));
        }
        let msg = child_not_found_message(&reg, &3157.into(), 999);
        assert!(msg.contains("valid child indices: 0-2"), "{msg}");
        assert!(msg.contains("3 children"), "{msg}");
    }

    #[test]
    fn child_not_found_reports_empty_parent() {
        let reg = crate::runtime::child_terminal::ChildTerminalRegistry::default();
        let msg = child_not_found_message(&reg, &3157.into(), 0);
        assert!(msg.contains("no children registered"), "{msg}");
    }

    #[test]
    fn spawn_kill_occupancy_wiring() {
        // 점유와 관계 등록만 검사한다. 탭 생성·입력 핸들러 전체 시험은 아니다.
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let c = 5000u32;

        let idx = e.runtime.child_terminals.next_index_for(parent);
        e.runtime
            .child_terminals
            .register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();
        let occ = e
            .live
            .occupancy
            .occupancy_of(c)
            .expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent.into()));
        assert!(!e.live.occupancy.is_hard_occupied(c));

        assert!(
            e.runtime
                .child_terminals
                .remove_child(&parent.into(), idx)
                .is_some()
        );
        e.release_soft_occupancy(c, parent).unwrap();
        assert!(e.live.occupancy.occupancy_of(c).is_none());
    }

    #[test]
    fn adopt_registers_existing_surface_without_new_tab() {
        let mut e_session = engine_with_extra(&[6101]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = 6101u32; // 이미 존재하는(=spawn 아닌) surface

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_none());

        let occ = e
            .live
            .occupancy
            .occupancy_of(target)
            .expect("soft occupancy present");
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(parent.into()));
        assert_eq!(
            e.runtime.child_terminals.parent_of_child(target),
            Some(parent.into())
        );
    }

    #[test]
    fn adopt_rejects_already_registered_child() {
        let mut e_session = engine_with_extra(&[6102]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = 6102u32;

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
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": 999999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_self_adoption() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": parent }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn adopt_rejects_hard_occupied_target_and_leaves_registry_unchanged() {
        let mut e_session = engine_with_extra(&[6103]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = 6103u32;
        e.live
            .occupancy
            .acquire(target, /* hard occupancy client id */ 1)
            .unwrap();

        let resp = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "target": target }),
        );
        assert!(resp.error.is_some());
        assert!(e.runtime.child_terminals.parent_of_child(target).is_none());
        assert!(e.live.occupancy.is_hard_occupied(target)); // 기존 hard 점유는 건드리지 않음
    }

    #[test]
    fn release_clears_registry_and_occupancy_but_keeps_surface() {
        let mut e_session = engine_with_extra(&[5701]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let c = 5701u32;

        let idx = e.runtime.child_terminals.next_index_for(parent);
        e.runtime
            .child_terminals
            .register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, Some("worker".into())).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());

        assert!(
            e.runtime
                .child_terminals
                .find_child(&parent.into(), idx)
                .is_none()
        );
        assert!(e.live.occupancy.occupancy_of(c).is_none());
    }

    #[test]
    fn release_rejects_unregistered_child_index() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": 999u32 }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn release_does_not_touch_unrelated_hard_occupancy() {
        let mut e_session = engine_with_extra(&[5702]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let c = 5702u32;
        let idx = e.runtime.child_terminals.next_index_for(parent);
        e.runtime
            .child_terminals
            .register_child(parent, child(c, idx));
        e.occupy_soft(c, parent, None).unwrap();
        let other = 5703u32;
        e.live.occupancy.acquire(other, 1).unwrap();

        let resp = handle_release(
            &mut e,
            json!(1),
            &json!({ "surface": parent, "child": idx }),
        );
        assert!(resp.error.is_none());
        assert!(e.live.occupancy.occupancy_of(c).is_none()); // soft 는 정상 해제
        assert!(e.live.occupancy.is_hard_occupied(other)); // 무관한 hard 점유는 그대로
    }

    // 이 시험은 단일 engine의 부모 선택만 확인한다. 창 간 모호성은 App::find_request_owner에서 거절한다.
    #[test]
    fn resolve_parent_omitted_surface_succeeds_with_single_parent() {
        let mut e_session = engine_with_extra(&[59010]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let idx = e.runtime.child_terminals.next_index_for(parent);
        e.runtime
            .child_terminals
            .register_child(parent, child(59010, idx));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx }));
        assert!(resp.error.is_none(), "{:?}", resp.error);
    }

    #[test]
    fn resolve_parent_omitted_surface_errors_with_multiple_parents_in_one_engine() {
        let mut e_session = engine_with_extra(&[59030, 59040]);
        let mut e = e_session.borrow_mut();
        let parent1 = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let parent2 = 59020u32;
        let idx1 = e.runtime.child_terminals.next_index_for(parent1);
        e.runtime
            .child_terminals
            .register_child(parent1, child(59030, idx1));
        let idx2 = e.runtime.child_terminals.next_index_for(parent2);
        e.runtime
            .child_terminals
            .register_child(parent2, child(59040, idx2));

        let resp = handle_release(&mut e, json!(1), &json!({ "child": idx1 }));
        assert!(resp.error.is_some());
    }

    #[test]
    fn children_reconcile_prunes_dead_child() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        e.runtime
            .child_terminals
            .register_child(parent, child(99999, 0));
        let resp = handle_children(&mut e, json!(1), &json!({ "surface": parent }));
        let n = ok(resp)["children"].as_array().unwrap().len();
        assert_eq!(n, 0);
    }

    #[test]
    fn children_item_carries_evidence_and_confidence() {
        let mut e_session = engine_with_extra(&[5901]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = 5901u32;
        e.runtime
            .child_terminals
            .register_child(parent, child(target, 0));

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
        let mut e_session = engine_with_extra(&[5902]);
        let mut e = e_session.borrow_mut();
        let parent = e
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = 5902u32;
        e.runtime
            .child_terminals
            .register_child(parent, child(target, 0));

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
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let resp = handle_parent(&mut e, json!(1), &json!({ "surface": 424242 }));
        assert_eq!(ok(resp)["status"], "none");
    }

    #[test]
    fn set_state_rejects_unknown() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        let resp = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "bogus" }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn set_state_updates_registry() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        e.runtime.child_terminals.register_child(7, child(5000, 0));
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "idle" }),
        );
        assert_eq!(e.runtime.child_terminals.state_of(5000), "idle");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "needs_input" }),
        );
        assert_eq!(e.runtime.child_terminals.state_of(5000), "needs_input");
        let _ = handle_set_state(
            &mut e,
            json!(1),
            &json!({ "surface": 5000, "state": "active" }),
        );
        assert_eq!(e.runtime.child_terminals.state_of(5000), "active");
    }

    // needs_input 뒤 idle만 보고되어도 이전 입력 대기 상태를 지워야 한다.
    #[test]
    fn set_state_idle_clears_a_pending_needs_input() {
        let mut e_session = engine();
        let mut e = e_session.borrow_mut();
        e.runtime.child_terminals.register_child(7, child(5001, 0));
        fn push_child_state(e: &mut EngineMut<'_>, state: &str) {
            let resp = handle_set_state(e, json!(1), &json!({ "surface": 5001, "state": state }));
            assert!(resp.error.is_none(), "{state} 주입이 거부됐다");
        }
        push_child_state(&mut e, "needs_input");
        assert_eq!(e.runtime.child_terminals.state_of(5001), "needs_input");
        push_child_state(&mut e, "idle");
        assert_eq!(e.runtime.child_terminals.state_of(5001), "idle");
    }

    /// tell의 거절 순서와 wake 횟수를 관측한다. 본문은 기본 surface의 detached 터미널 sink로 본다.
    fn tell_fixture() -> (
        crate::runtime::engine_session::EngineSession,
        std::sync::Arc<std::sync::atomic::AtomicUsize>,
        std::sync::mpsc::Receiver<Vec<u8>>,
    ) {
        let mut session = engine();
        let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let e = session.borrow_mut();
            let counter = wakes.clone();
            e.runtime.waker = std::sync::Arc::new(move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            });
            e.runtime
                .terminals
                .get_mut(1)
                .expect("default terminal")
                .set_input_sink(tx);
        }
        (session, wakes, rx)
    }

    fn tell(e: &mut EngineMut<'_>, surface: u32) -> JsonRpcResponse {
        handle_tell(e, json!(1), &json!({ "surface": surface, "text": "hi" }))
    }

    #[test]
    fn tell_refuses_a_full_queue_before_writing_the_body() {
        let (mut session, wakes, rx) = tell_fixture();
        let mut e = session.borrow_mut();
        let generation = e.runtime.terminals.generation(1).expect("generation");
        for _ in 0..crate::runtime::pending_submit::MAX_PENDING {
            let ack = tasty_terminal::Terminal::new_detached(4, 2).send_key_with_ack("x");
            e.runtime
                .pending_submits
                .push(crate::runtime::pending_submit::PendingSubmit::new(
                    1, generation, ack,
                ));
        }

        let err = tell(&mut e, 1).error.expect("full queue must refuse");
        assert_eq!(err.code, tasty_ipc::protocol::ERR_COMMAND_QUEUE_FULL);
        assert_eq!(err.message, "terminal submission queue is full");
        assert_eq!(
            e.runtime.pending_submits.len(),
            crate::runtime::pending_submit::MAX_PENDING
        );
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn tell_reports_a_missing_surface_without_queueing() {
        let (mut session, wakes, _rx) = tell_fixture();
        let mut e = session.borrow_mut();

        let err = tell(&mut e, 424_242).error.expect("missing surface");
        assert_eq!(err.code, -32602);
        assert_eq!(err.message, "Surface 424242 not found");
        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn tell_refuses_a_hard_occupied_surface_without_writing_or_queueing() {
        let (mut session, wakes, rx) = tell_fixture();
        let mut e = session.borrow_mut();
        e.live.occupancy.acquire(1, 1).expect("hard occupy");

        let err = tell(&mut e, 1).error.expect("hard-occupied surface");
        assert_eq!(err.code, -32602);
        assert_eq!(
            err.message,
            "Surface 1 is attached elsewhere (hard-occupied) — release the attach or target a \
             different surface"
        );
        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(rx.try_iter().next().is_none());
    }

    #[test]
    fn tell_writes_the_body_queues_one_submit_and_wakes_once() {
        let (mut session, wakes, rx) = tell_fixture();
        let mut e = session.borrow_mut();
        let generation = e.runtime.terminals.generation(1).expect("generation");

        let ok = tell(&mut e, 1).result.expect("tell succeeds");
        assert_eq!(ok, json!({ "sent": true, "surface_id": 1 }));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), vec![b"hi".to_vec()]);
        assert_eq!(e.runtime.pending_submits.len(), 1);
        assert_eq!(e.runtime.pending_submits[0].surface(), 1);
        assert!(e.runtime.terminals.matches_generation(1, generation));
        assert_eq!(wakes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    fn external(key: &str) -> ParentKey {
        ParentKey::External(key.into())
    }

    /// External 부모 둘에 자식을 하나씩 단다. spawn 설치처럼 관계와 soft 점유를 함께 기록한다.
    fn two_external_parents(e: &mut EngineMut<'_>) {
        for (key, sid) in [("codex:a", 6101), ("codex:b", 6102)] {
            let parent = external(key);
            let index = e.runtime.child_terminals.reserve_index(&parent).unwrap();
            e.occupy_soft(sid, parent.clone(), Some("worker".into()))
                .unwrap();
            e.runtime
                .child_terminals
                .register_child(parent, child(sid, index));
        }
    }

    #[test]
    fn external_parents_list_report_and_release_only_their_own_children() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);

        let listed = ok(handle_children(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:a" }),
        ));
        let ids: Vec<u64> = listed["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["surface_id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, [6101]);

        assert_eq!(
            ok(handle_parent(&mut e, json!(1), &json!({ "surface": 6101 }))),
            json!({ "parent_surface_id": null, "parent_external": "codex:a", "status": "active" })
        );

        let wrong = handle_release(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:b", "child": 1 }),
        );
        let message = wrong.error.expect("b has no child 1").message;
        assert!(
            message.starts_with("child 1 not found under parent_external codex:b"),
            "{message}"
        );

        let released = ok(handle_release(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:a", "child": 0 }),
        ));
        assert_eq!(released["released_surface_id"], json!(6101));
        assert!(e.live.occupancy.occupancy_of(6101).is_none());
        assert!(
            e.runtime
                .child_terminals
                .list_children(&external("codex:a"))
                .is_empty()
        );
        assert_eq!(
            e.runtime
                .child_terminals
                .reserve_index(&external("codex:a")),
            Ok(0),
            "last child released restarts the numbering"
        );
        assert_eq!(
            e.live.occupancy.occupancy_of(6102).unwrap().parent,
            Some(external("codex:b"))
        );
        assert_eq!(
            e.runtime
                .child_terminals
                .list_children(&external("codex:b"))
                .len(),
            1
        );
    }

    /// 다른 External 부모의 child_surface_id를 넘기면 CLI 옵션 표기로 올바른 인자를 안내한다.
    #[test]
    fn a_child_of_another_external_parent_points_at_the_cli_option() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);
        let msg = child_not_found_message(&e.runtime.child_terminals, &external("codex:a"), 6102);
        assert!(
            msg.ends_with("use `--parent-external codex:b --child 0`"),
            "{msg}"
        );
    }

    #[test]
    fn a_surface_and_an_external_parent_together_are_refused() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);
        for resp in [
            handle_children(
                &mut e,
                json!(1),
                &json!({ "surface": 1, "parent_external": "codex:a" }),
            ),
            handle_release(
                &mut e,
                json!(1),
                &json!({ "surface": 1, "parent_external": "codex:a", "child": 0 }),
            ),
        ] {
            let err = resp.error.expect("both parents");
            assert_eq!(err.code, -32602);
            assert_eq!(
                err.message,
                "pass either 'surface' or 'parent_external', not both"
            );
        }
        let err = handle_children(&mut e, json!(1), &json!({ "parent_external": "" }))
            .error
            .expect("empty key");
        assert_eq!(err.message, "'parent_external' must not be empty");
        assert_eq!(
            e.runtime
                .child_terminals
                .list_children(&external("codex:a"))
                .len(),
            1,
            "a refused request changes nothing"
        );
    }

    #[test]
    fn the_omitted_parent_fallback_skips_external_parents() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);
        let err = handle_release(&mut e, json!(1), &json!({ "child": 0 }))
            .error
            .expect("no surface parent to fall back to");
        assert_eq!(
            err.message,
            "missing 'surface' parameter (0 or >1 parents — specify --surface)"
        );
    }

    #[test]
    fn adopt_refuses_an_external_parent() {
        let mut e_session = engine_with_extra(&[6101]);
        let mut e = e_session.borrow_mut();
        let err = handle_adopt(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:a", "target": 6101 }),
        )
        .error
        .expect("adopt is surface only");
        assert_eq!(
            err.message,
            "terminal.adopt does not accept 'parent_external'"
        );
        assert!(e.runtime.child_terminals.parent_of_child(6101).is_none());
        assert!(e.live.occupancy.occupancy_of(6101).is_none());
    }

    #[test]
    fn a_broadcast_fixes_only_the_external_parents_children() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);
        let request: crate::ipc::protocol::JsonRpcRequest = serde_json::from_value(json!({
            "jsonrpc": "2.0",
            "method": "terminal.broadcast",
            "params": { "parent_external": "codex:b", "text": "hi\r" },
            "id": 1,
        }))
        .unwrap();
        let fixed = prepare_input(&e.as_ref(), &request)
            .ok()
            .flatten()
            .expect("broadcast is fixed");
        assert_eq!(fixed.targets(), [6102]);
    }

    #[test]
    fn a_new_prompt_clears_idle_on_an_external_child() {
        let mut e_session = engine_with_extra(&[6101, 6102]);
        let mut e = e_session.borrow_mut();
        two_external_parents(&mut e);
        e.runtime.child_terminals.set_idle(6101, true);
        assert!(e.runtime.child_terminals.clear_idle_for_new_prompt(6101));
        assert_eq!(e.runtime.child_terminals.state_of(6101), "active");
    }

    /// 직접 respawn의 전송 실패는 단계마다 기존 코드와 문구를 그대로 낸다.
    #[test]
    fn a_direct_respawn_maps_each_send_failure_to_its_own_code() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut e_session = engine_with_extra(&[6101]);
        let mut e = e_session.borrow_mut();
        let parent = external("codex:r");
        for (sid, index) in [(1, 0), (6101, 1)] {
            e.runtime
                .child_terminals
                .register_child(parent.clone(), child(sid, index));
        }

        let missing = handle_respawn(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:r", "child": 1 }),
        )
        .error
        .expect("no terminal");
        assert_eq!(
            (missing.code, missing.message.as_str()),
            (-32603, "No terminal found")
        );

        e.live.occupancy.acquire(1, 1).expect("hard occupy");
        let held = handle_respawn(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:r", "child": 0, "command": "make" }),
        )
        .error
        .expect("hard occupied");
        assert_eq!(held.code, -32602);
        assert_eq!(
            held.message,
            "Surface 1 is hard-occupied (remote attach holds it) — server-local input is \
             blocked while attached"
        );

        let cwd = handle_respawn(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:r", "child": 0, "cwd": "/tmp" }),
        )
        .error
        .expect("cwd needs admission");
        assert_eq!(cwd.code, -32603);
        assert_eq!(
            cwd.message,
            "child resource respawn requires committed structure admission"
        );
    }

    /// 직접 broadcast는 본문과 끝 CR을 따로 쓰고, 보낼 수 없는 child도 대상 수에 센다.
    #[test]
    fn a_direct_broadcast_writes_body_then_cr_and_counts_every_target() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut e_session = engine_with_extra(&[6101]);
        let mut e = e_session.borrow_mut();
        let parent = external("codex:b");
        for (sid, index) in [(1, 0), (6101, 1)] {
            e.runtime
                .child_terminals
                .register_child(parent.clone(), child(sid, index));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        e.runtime
            .terminals
            .get_mut(1)
            .expect("terminal")
            .set_input_sink(tx);
        e.runtime.child_terminals.set_idle(1, true);
        e.runtime.child_terminals.set_idle(6101, true);
        let response = ok(handle_broadcast(
            &mut e,
            json!(1),
            &json!({ "parent_external": "codex:b", "text": "go\r" }),
        ));
        assert_eq!(response, json!({ "sent_count": 2, "children": [1, 6101] }));
        let writes: Vec<Vec<u8>> = rx.try_iter().collect();
        assert_eq!(writes, [b"go".to_vec(), b"\r".to_vec()]);
        assert_eq!(e.runtime.child_terminals.state_of(1), "active");
        assert_eq!(e.runtime.child_terminals.state_of(6101), "idle");
    }
}
