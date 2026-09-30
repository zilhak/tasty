//! Remote structural requests are resolved and applied at the shared application boundary.

use crate::core::CoreState;
use crate::core::engine_access::EngineMut;
use crate::model::SurfaceId;
use tasty_ipc::stream::StructuralOp;

/// 서버에서 실행한 구조 변경을 client에 반영하기 위한 결과.
#[derive(Debug)]
pub(crate) struct ForwardedDelta {
    pub delta: tasty_ipc::stream::StreamControl,
    /// 호출자는 delta 직후 새 터미널을 tap해야 client의 ID 매핑보다 snapshot이 먼저 도착하지 않는다.
    pub added_terminals: Vec<SurfaceId>,
    /// 실제로 종류가 바뀐 surface ID. PluginManager를 가진 호출자가 오래된 mesh frame을 지운다.
    pub converted_surface: Option<SurfaceId>,
}

/// forward된 요청은 원격 쪽 사용자 조작이어도 이 호스트의 사용자 포커스를 옮기지 않는다.
/// 복원 기록과 새 탭 활성화는 ForwardOrigin에서 따로 정한다.
pub(crate) fn forward_intent_origin(
    _origin: tasty_ipc::stream::ForwardOrigin,
) -> crate::core::origin::IntentOrigin {
    crate::core::origin::IntentOrigin::Agent {
        source: crate::core::origin::AgentSource::Remote,
    }
}

/// mirror의 구조 변경을 서버에서 실행한다. 호출자가 holder를 검증해야 한다.
/// IPC의 권한·자기 대상·hard 점유 검사는 여기서 실행하지 않는다. split·tab·close는
/// structural_exec를 공유하고 convert·restore·move-surface는 Core::apply를 직접 호출한다.
///
/// anchor workspace의 실행 전후 차이로 새 터미널을 찾고 현재 트리를 반환한다.
/// workspace가 사라지면 점유를 해제하고 Ok(None), 실행 실패는 Err(reason)이다.
/// 호출자는 result → delta → 새 터미널 tap 순서로 처리한다. workspace가 사라진 경우에는
/// 이 함수의 강제 분리 통지가 result보다 먼저 나간다.
/// origin이 User인 close만 복원 스택에 남기고 Agent의 close는 남기지 않는다.
pub(crate) fn execute_forwarded_structural_op(
    core: &mut crate::core::Core,
    state: &mut dyn crate::app::structure_context::CascadeWindow,
    engine: &mut EngineMut<'_>,
    op: &StructuralOp,
    origin: tasty_ipc::stream::ForwardOrigin,
) -> Result<Option<ForwardedDelta>, String> {
    use crate::app::structural_exec::{self as exec, SplitLevel, SplitRequest};
    use serde_json::json;
    use std::collections::HashSet;

    engine.refresh_attach_presentation(state.presentation());

    // close가 anchor를 지워도 변경 후 workspace를 찾을 수 있도록 ID를 먼저 보관한다.
    let ws_id = engine
        .find_workspace_index_for_surface(op.anchor_surface_id())
        .map(|(idx, _)| engine.workspaces[idx].id);
    let before: HashSet<SurfaceId> = ws_id
        .and_then(|id| engine.find_workspace_index_for_id(id))
        .map(|idx| {
            engine.workspaces[idx]
                .all_surface_ids()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

    let mut converted_surface: Option<SurfaceId> = None;
    let restorable = origin == tasty_ipc::stream::ForwardOrigin::User;
    let intent_origin = forward_intent_origin(origin);

    let outcome: Result<(), String> = match op {
        StructuralOp::SplitSurface {
            surface_id,
            direction,
            surface_kind,
            params,
        } => {
            let p = structural_params(
                params,
                json!({
                    "level": "surface",
                    "direction": direction.as_ipc_str(),
                    "target_surface": surface_id,
                    "type": surface_kind,
                }),
            );
            let target_pane = crate::core::param_bag::read_u32(&p, "target_pane")?;
            let req = SplitRequest {
                level: SplitLevel::Surface,
                direction: split_direction(*direction),
                target_surface: Some(*surface_id),
                target_pane,
                params: &p,
            };
            // 결과를 판정하기 전에 자동 tap 억제를 해제해야 오류가 나도 다음 생성에 영향을 주지 않는다.
            engine.attach.set_auto_tap_suppressed(true);
            let result = exec::split(core, state, engine, req, &intent_origin);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::SplitPane {
            anchor_surface_id,
            direction,
            surface_kind,
            params,
        } => {
            let p = structural_params(
                params,
                json!({
                    "level": "pane",
                    "direction": direction.as_ipc_str(),
                    "target_surface": anchor_surface_id,
                    "type": surface_kind,
                }),
            );
            let target_pane = crate::core::param_bag::read_u32(&p, "target_pane")?;
            let req = SplitRequest {
                level: SplitLevel::Pane,
                direction: split_direction(*direction),
                target_surface: Some(*anchor_surface_id),
                target_pane,
                params: &p,
            };
            engine.attach.set_auto_tap_suppressed(true);
            let result = exec::split(core, state, engine, req, &intent_origin);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::NewTab {
            anchor_surface_id,
            surface_kind,
            params,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} not found"))?;
            let p = structural_params(params, json!({ "pane_id": pane_id, "type": surface_kind }));
            engine.attach.set_auto_tap_suppressed(true);
            let activate = origin == tasty_ipc::stream::ForwardOrigin::User;
            let result =
                exec::create_tab(core, state, engine, pane_id, &p, activate, &intent_origin);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::CloseSurface { surface_id } => {
            // holder의 요청은 도메인 실행을 직접 부른다. params로 점유 검사 면제를 허용하지 않는다.
            forward_result(exec::close_surface(
                core,
                state,
                engine,
                *surface_id,
                restorable,
                &intent_origin,
            ))
        }
        StructuralOp::CloseTab { anchor_surface_id } => {
            let tab_id = engine
                .find_tab_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} tab not found"))?;
            // 기록은 출처 workspace를 트리에서 찾으므로 닫기 전에 한다.
            // pane의 마지막 탭은 닫히지 않으므로 기록하지 않는다. Core 탭 닫기와 같은 규칙이다.
            if let Some(item) = restorable
                .then(|| engine.find_pane_for_tab(tab_id))
                .flatten()
                .and_then(|pane_id| {
                    let tabs = &engine.find_pane_by_id(pane_id)?.tabs;
                    if tabs.len() <= 1 {
                        return None;
                    }
                    let idx = tabs.iter().position(|t| t.id == tab_id)?;
                    Some((pane_id, idx))
                })
                .and_then(|(pane_id, idx)| {
                    engine.capture_closed_tab(pane_id, idx, state.presentation())
                })
            {
                engine.push_closed_item(item);
            }
            forward_result(exec::close_tab(core, state, engine, tab_id, &intent_origin))
        }
        StructuralOp::ClosePane { anchor_surface_id } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            // 닫기 전에 캡처해야 pane의 split context가 남는다.
            // 유일한 pane은 닫히지 않으며 capture_closed_pane도 None을 돌려준다.
            if let Some(item) = restorable
                .then(|| engine.capture_closed_pane(pane_id, state.presentation()))
                .flatten()
            {
                engine.push_closed_item(item);
            }
            forward_result(exec::close_pane(
                core,
                state,
                engine,
                pane_id,
                &intent_origin,
            ))
        }
        StructuralOp::MoveTab {
            anchor_surface_id,
            from_index,
            to_index,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            forward_result(exec::move_tab(
                core,
                state,
                engine,
                pane_id,
                *from_index,
                *to_index,
                &intent_origin,
            ))
        }
        StructuralOp::ConvertSurface {
            surface_id,
            surface_kind,
            params,
            cwd,
        } => {
            // 요청 cwd가 우선이다. 없으면 서버의 inherit_cwd 설정에 따라 실제 터미널 cwd를 조회한다.
            use crate::core::intent::ConvertSurfaceTarget;
            let carried_cwd = cwd
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(std::path::PathBuf::from)
                .or_else(|| state.resolve_inherit_cwd_from_surface(engine, *surface_id));
            let target = if surface_kind == "terminal" {
                ConvertSurfaceTarget::Terminal { cwd: carried_cwd }
            } else {
                ConvertSurfaceTarget::Kind {
                    cwd: carried_cwd,
                    kind: surface_kind.clone(),
                    params: params.clone(),
                }
            };
            let intent = crate::core::intent::DomainIntent::ConvertSurface {
                surface_id: *surface_id,
                target,
            };
            match crate::app::structural_exec::execute(core, state, engine, intent) {
                Ok(events) => match events.into_iter().next() {
                    Some(crate::core::intent::CoreEvent::SurfaceConverted {
                        replaced: true,
                        ..
                    }) => {
                        converted_surface = Some(*surface_id);
                        Ok(())
                    }
                    // 도메인이 낸 실패 이유를 그대로 보내 원인을 다른 오류로 바꾸지 않는다.
                    Some(crate::core::intent::CoreEvent::SurfaceConverted {
                        failure: Some(reason),
                        ..
                    }) => Err(reason),
                    _ => Err(convert_failure_fallback(*surface_id)),
                },
                Err(e) => Err(e.to_string()),
            }
        }
        StructuralOp::RestoreClosedItem { anchor_surface_id } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            let ws_id = engine
                .find_workspace_index_for_pane(pane_id)
                .map(|idx| engine.workspaces[idx].id)
                .ok_or_else(|| format!("pane {pane_id} workspace not found"))?;
            let intent = crate::core::intent::DomainIntent::RestoreClosedItem {
                target_pane_id: Some(pane_id),
                // 다른 workspace에서 닫힌 항목은 복원하지 않는다.
                scope: crate::core::intent::RestoreScope::Workspace(ws_id),
            };
            match crate::app::structural_exec::execute(core, state, engine, intent) {
                Ok(events) => {
                    let restored = matches!(
                        events.into_iter().next(),
                        Some(crate::core::intent::CoreEvent::ClosedItemRestored {
                            restored: true,
                            ..
                        })
                    );
                    if restored {
                        // 로컬 복원 후속 처리는 서버 사용자의 workspace·pane 선택을 바꾸므로 호출하지 않는다.
                        // mirror의 선택은 client가 delta를 적용할 때 처리한다.
                        Ok(())
                    } else {
                        // client가 빈 복원 목록 안내를 구별하도록 전용 sentinel을 반환한다.
                        Err(tasty_ipc::stream::STRUCTURAL_REASON_RESTORE_EMPTY.to_string())
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
        StructuralOp::MoveSurface {
            source_surface_id,
            target_surface_id,
        } => {
            // 호출자는 anchor(source)의 holder만 검증한다. target이 점유한 workspace 밖이면
            // 다른 workspace의 surface를 덮어쓰게 되므로 없는 surface와 같은 문구로 거절한다.
            let target_ws = engine
                .find_workspace_index_for_surface(*target_surface_id)
                .map(|(idx, _)| engine.workspaces[idx].id);
            if ws_id.is_none() || target_ws != ws_id {
                return Err(crate::core::request_target::unowned_target_message(
                    crate::core::request_target::ResourceId {
                        kind: crate::core::request_target::Kind::Surface,
                        id: u64::from(*target_surface_id),
                    },
                    &format!("structural_op.{}", op.wire_kind()),
                ));
            }
            let intent = crate::core::intent::DomainIntent::MoveSurface {
                source_surface_id: *source_surface_id,
                target_surface_id: *target_surface_id,
            };
            match crate::app::structural_exec::execute(core, state, engine, intent) {
                Ok(events) => {
                    let ev = events.into_iter().next();
                    if !matches!(
                        ev,
                        Some(crate::core::intent::CoreEvent::MoveSurfaceApplied { .. })
                    ) {
                        return Err("Core::apply returned no MoveSurfaceApplied event".to_string());
                    }
                    match ev.and_then(|ev| {
                        crate::app::structural_cascade::SurfaceCloseCascade::from_move_surface_applied(
                            ev, false,
                        )
                    }) {
                        Some(c) => {
                            crate::app::structural_cascade::cascade_surface_closed(
                                core, state, engine, c,
                            );
                            Ok(())
                        }
                        None => Err(format!(
                            "move failed: source={source_surface_id} target={target_surface_id}"
                        )),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
    };

    outcome?;

    // 이 경로가 delta를 반환하므로 일반 구조 변경 통지가 같은 트리를 다시 보내지 않게 한다.
    if let Some(ws_id) = ws_id {
        engine.attach.clear_structure_changed(ws_id);
    }

    let Some(ws_id) = ws_id else {
        return Ok(None);
    };
    let Some(idx_after) = engine.find_workspace_index_for_id(ws_id) else {
        // 보낼 트리가 없으므로 점유를 지우고 강제 분리한다. 호출자의 StructuralResult보다
        // 이 통지가 먼저 나가며, client는 분리 뒤의 결과 프레임을 읽지 않을 수 있다.
        engine.attach.force_detach_workspace(ws_id);
        return Ok(None);
    };
    let class = engine.workspaces[idx_after].classify_attach_surfaces();
    let added_terminals: Vec<SurfaceId> = class
        .terminals
        .iter()
        .copied()
        .filter(|sid| !before.contains(sid))
        .collect();
    // 새 터미널도 workspace 점유에 넣어 입력·resize의 holder 검사와 서버 읽기 전용 표시를 유지한다.
    for sid in &added_terminals {
        engine.attach.add_workspace_member(ws_id, *sid, true);
    }
    let (tree, surfaces) = engine.build_workspace_tree_surfaces(idx_after, &class);
    let delta = tasty_ipc::stream::StreamControl::StructuralDelta {
        workspace_id: ws_id,
        tree,
        surfaces,
    };
    Ok(Some(ForwardedDelta {
        delta,
        added_terminals,
        converted_surface,
    }))
}

/// 성공 값은 버리고 실패 문구는 그대로 전달한다. 다른 mirror로 다시 forward한 결과도 성공으로 취급한다.
fn forward_result<T>(
    result: Result<T, crate::app::structural_exec::StructuralFailure>,
) -> Result<(), String> {
    use crate::app::structural_exec::StructuralFailure;
    match result {
        Ok(_) => Ok(()),
        Err(StructuralFailure::Rejected(msg)) => Err(msg),
        Err(StructuralFailure::MissingEvent(msg)) => Err(msg.to_string()),
        Err(StructuralFailure::Apply(e)) => {
            if e.downcast_ref::<crate::core::MirrorStructuralBlocked>()
                .is_some_and(|blocked| blocked.forwarded)
            {
                Ok(())
            } else {
                Err(e.to_string())
            }
        }
    }
}

/// 도메인이 실패 이유를 빠뜨렸을 때 쓴다. 원인을 추측해 not found 등으로 바꾸지 않는다.
fn convert_failure_fallback(surface_id: SurfaceId) -> String {
    format!("surface {surface_id} was not converted")
}

fn split_direction(axis: tasty_ipc::stream::SplitAxis) -> crate::model::SplitDirection {
    match axis {
        tasty_ipc::stream::SplitAxis::Horizontal => crate::model::SplitDirection::Horizontal,
        tasty_ipc::stream::SplitAxis::Vertical => crate::model::SplitDirection::Vertical,
    }
}

/// base 객체에 제어 키를 덮어쓴다. base가 객체가 아니면 빈 객체에서 시작한다.
/// 이 묶음은 IPC params와 같은 경로로 읽히고 새 surface의 params에도 남는다.
fn structural_params(base: &serde_json::Value, control: serde_json::Value) -> serde_json::Value {
    let mut obj = base.as_object().cloned().unwrap_or_default();
    if let Some(ctrl) = control.as_object() {
        for (k, v) in ctrl {
            obj.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(obj)
}
