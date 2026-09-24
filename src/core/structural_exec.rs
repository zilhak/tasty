//! IPC와 원격 forward의 구조 변경 검증·실행·후속 처리를 공유한다.
//! 권한·요청자·점유 검사는 각 진입점이 먼저 수행해야 한다. wire 응답 조립도 진입점 몫이다.
//! forward의 대상 해석·복원 snapshot·출력 tap 제어는 attach_runtime이 맡는다.

use std::path::PathBuf;

use serde_json::Value;

use crate::core::cascade_window::CascadeWindow;
use crate::core::intent::{CoreEvent, DomainIntent};
use crate::core::origin::{AgentSource, IntentOrigin};
use crate::core::structural_cascade::{
    PaneSplitCascade, SurfaceCloseCascade, cascade_pane_closed_full, cascade_pane_split,
    cascade_surface_closed, cascade_surface_split, cascade_tab_closed_full, cascade_tab_created,
};
use crate::core::{Core, CoreState};
use crate::model::SplitDirection;

/// IPC 오류 코드에 대응하는 실패 종류. 두 진입점은 같은 실행 오류 문구를 받는다.
#[derive(Debug)]
pub(crate) enum StructuralFailure {
    Rejected(String),
    MissingEvent(&'static str),
    Apply(anyhow::Error),
}

impl From<anyhow::Error> for StructuralFailure {
    fn from(e: anyhow::Error) -> Self {
        Self::Apply(e)
    }
}

/// 원격 조작이 이 호스트의 사용자 포커스를 옮기지 않도록 Agent origin을 사용한다.
fn agent_origin() -> IntentOrigin {
    IntentOrigin::Agent {
        source: AgentSource::Ipc,
    }
}

/// 다른 mirror로 다시 전달한 요청도 agent 표시를 붙여 실패를 사용자 toast 대신 로그로 보낸다.
fn apply_as_agent(
    core: &mut Core,
    engine: &mut CoreState,
    intent: DomainIntent,
) -> Result<Vec<CoreEvent>, StructuralFailure> {
    core.apply(engine, intent).map_err(|e| {
        crate::core::mark_last_forward_agent_origin(engine, &e, &agent_origin());
        StructuralFailure::Apply(e)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitLevel {
    Pane,
    Surface,
}

pub(crate) struct SplitRequest<'a> {
    pub(crate) level: SplitLevel,
    pub(crate) direction: SplitDirection,
    pub(crate) target_surface: Option<u32>,
    pub(crate) target_pane: Option<u32>,
    /// 새 surface에 넘길 파라미터. 종류별 필수 값도 여기에 담는다.
    pub(crate) params: &'a Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitOutcome {
    Pane {
        new_pane_id: u32,
        new_surface_id: u32,
    },
    Surface {
        new_surface_id: u32,
    },
}

/// 대상을 검증하고 구조 변경·후속 처리 뒤 문자열 meta를 적용한다. meta 저장 실패는 로그로 남긴다.
pub(crate) fn split(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    req: SplitRequest<'_>,
) -> Result<SplitOutcome, StructuralFailure> {
    let SplitRequest {
        level,
        direction,
        target_surface,
        target_pane,
        params,
    } = req;

    if target_surface.is_none() && target_pane.is_none() {
        return Err(StructuralFailure::Rejected(
            "Missing target. Use 'target_surface' (surface ID or nickname) and/or 'target_pane' (pane ID)".to_string(),
        ));
    }
    if target_surface.is_some() && target_pane.is_some() {
        return Err(StructuralFailure::Rejected(
            "Cannot specify both 'target_surface' and 'target_pane'. Use one.".to_string(),
        ));
    }

    let meta = params.get("meta").and_then(|v| v.as_object());
    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);
    // 디렉터리 여부를 확인한다. 상대 경로 거부나 경로 정규화는 이 검사에 포함되지 않는다.
    if let Some(p) = &cwd
        && !p.is_dir()
    {
        return Err(StructuralFailure::Rejected(format!(
            "cwd does not exist: {}",
            p.display()
        )));
    }
    let kind = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    if let Some(def) = engine.surface_registry.get_live(kind)
        && let Some(missing) = def.first_missing_required_param(params)
    {
        return Err(StructuralFailure::Rejected(format!(
            "Missing '{missing}' parameter for {kind} type"
        )));
    }

    let outcome = match level {
        SplitLevel::Pane => {
            let resolved_pane_id = if let Some(pid) = target_pane {
                pid
            } else if let Some(sid) = target_surface {
                match engine.find_pane_for_surface(sid) {
                    Some(pid) => pid,
                    None => {
                        return Err(StructuralFailure::Rejected(format!(
                            "Surface {sid} not found"
                        )));
                    }
                }
            } else {
                return Err(StructuralFailure::Rejected(
                    "pane-level split requires 'target_pane' or 'target_surface'".to_string(),
                ));
            };

            let resolved_cwd = if kind == "terminal" {
                cwd.or_else(|| {
                    let sid = engine
                        .find_pane_by_id(resolved_pane_id)
                        .and_then(|p| p.tabs.get(p.active_tab))
                        .and_then(|t| t.focused_surface_id())?;
                    state.resolve_inherit_cwd_from_surface(engine, sid)
                })
            } else {
                None
            };

            let intent = DomainIntent::SplitPane {
                target_pane_id: resolved_pane_id,
                direction,
                cwd: resolved_cwd,
                kind: kind.to_string(),
                surface_params: params.clone(),
            };
            let events = apply_as_agent(core, engine, intent)?;
            let Some(CoreEvent::PaneSplit {
                workspace_index,
                original_pane_id,
                new_pane_id,
                new_surface_id,
                direction,
            }) = events.into_iter().next()
            else {
                return Err(StructuralFailure::MissingEvent(
                    "Core::apply returned no PaneSplit event",
                ));
            };

            cascade_pane_split(
                state,
                engine,
                &agent_origin(),
                PaneSplitCascade {
                    workspace_index,
                    original_pane_id,
                    new_pane_id,
                    new_surface_id,
                    direction,
                },
            );
            SplitOutcome::Pane {
                new_pane_id,
                new_surface_id,
            }
        }
        SplitLevel::Surface => {
            let Some(sid) = target_surface else {
                return Err(StructuralFailure::Rejected(
                    "Surface-level split requires 'target_surface', not 'target_pane'".to_string(),
                ));
            };

            let resolved_cwd = if kind == "terminal" {
                cwd.or_else(|| state.resolve_inherit_cwd_from_surface(engine, sid))
            } else {
                None
            };

            let intent = DomainIntent::SplitSurface {
                target_surface_id: sid,
                direction,
                cwd: resolved_cwd,
                kind: kind.to_string(),
                surface_params: params.clone(),
            };
            let events = apply_as_agent(core, engine, intent)?;
            let Some(CoreEvent::SurfaceSplit {
                workspace_index,
                pane_id,
                new_surface_id,
            }) = events.into_iter().next()
            else {
                return Err(StructuralFailure::MissingEvent(
                    "Core::apply returned no SurfaceSplit event",
                ));
            };

            cascade_surface_split(
                state,
                engine,
                &agent_origin(),
                workspace_index,
                pane_id,
                new_surface_id,
            );
            SplitOutcome::Surface { new_surface_id }
        }
    };

    let new_surface_id = match outcome {
        SplitOutcome::Pane { new_surface_id, .. } | SplitOutcome::Surface { new_surface_id } => {
            new_surface_id
        }
    };
    apply_meta(state, new_surface_id, meta);
    Ok(outcome)
}

fn apply_meta(
    state: &dyn CascadeWindow,
    surface_id: u32,
    meta: Option<&serde_json::Map<String, Value>>,
) {
    if let Some(map) = meta {
        for (key, value) in map {
            if let Some(v) = value.as_str() {
                let result = state.set_surface_meta(surface_id, key, v);
                if let Err(e) = result {
                    tracing::warn!(
                        "surface_meta set failed for surface {surface_id} key '{key}': {e}"
                    );
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabCreated {
    pub(crate) pane_id: u32,
    pub(crate) surface_id: u32,
    pub(crate) tab_count: usize,
    pub(crate) active_tab: usize,
}

/// 종류별 기본 파라미터를 보충해 탭을 만든다. 에이전트 진입점은 activate=false를 넘긴다.
pub(crate) fn create_tab(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    pane_id: u32,
    params: &Value,
    activate: bool,
) -> Result<TabCreated, StructuralFailure> {
    if engine.find_pane_by_id(pane_id).is_none() {
        return Err(StructuralFailure::Rejected(format!(
            "Pane {} not found",
            pane_id
        )));
    }

    let surface_type = params
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("terminal");

    // 새 탭은 상속된 params가 없으므로 @home 등 종류별 기본값을 여기서 보충한다.
    let mut params = params.clone();
    if let Some(def) = engine.surface_registry.get(surface_type) {
        let home = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf());
        engine.apply_kind_default_params(&def, &mut params, home.as_deref());
    }

    let cwd = if surface_type == "terminal" {
        let explicit = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(PathBuf::from);
        if let Some(p) = &explicit
            && !p.is_dir()
        {
            return Err(StructuralFailure::Rejected(format!(
                "cwd does not exist: {}",
                p.display()
            )));
        }
        explicit.or_else(|| {
            let sid = engine
                .find_pane_by_id(pane_id)
                .and_then(|p| p.tabs.get(p.active_tab))
                .and_then(|t| t.focused_surface_id())?;
            state.resolve_inherit_cwd_from_surface(engine, sid)
        })
    } else {
        None
    };

    let tab_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let intent = DomainIntent::CreateTab {
        pane_id,
        cwd,
        kind: surface_type.to_string(),
        name: tab_name,
        surface_params: params,
        activate,
    };
    let events = apply_as_agent(core, engine, intent)?;

    let Some(CoreEvent::TabCreated {
        pane_id,
        tab_id,
        surface_id,
        tab_count,
        active_tab,
    }) = events.into_iter().next()
    else {
        return Err(StructuralFailure::MissingEvent(
            "Core::apply returned no TabCreated event",
        ));
    };

    cascade_tab_created(state, engine, pane_id, tab_id, surface_id);

    Ok(TabCreated {
        pane_id,
        surface_id,
        tab_count,
        active_tab,
    })
}

/// closed=false도 성공 응답이다. 대상 부재나 닫기 제한 때문에 실제로 닫지 못했음을 나타낸다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Closed {
    pub(crate) id: u32,
    pub(crate) closed: bool,
}

pub(crate) fn close_tab(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    tab_id: u32,
) -> Result<Closed, StructuralFailure> {
    let events = apply_as_agent(core, engine, DomainIntent::CloseTab { tab_id })?;

    let Some(CoreEvent::TabClosed {
        tab_id,
        pane_id,
        closed,
        cleanup_targets,
    }) = events.into_iter().next()
    else {
        return Err(StructuralFailure::MissingEvent(
            "Core::apply returned no TabClosed event",
        ));
    };

    if closed {
        cascade_tab_closed_full(state, engine, tab_id, pane_id, cleanup_targets, false);
    }
    Ok(Closed { id: tab_id, closed })
}

/// 없는 pane은 Core 호출 전에 오류로 돌려준다. 없는 tab의 처리와 다르다.
pub(crate) fn close_pane(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    pane_id: u32,
) -> Result<Closed, StructuralFailure> {
    if engine.find_pane_by_id(pane_id).is_none() {
        return Err(StructuralFailure::Rejected(format!(
            "Pane {} not found",
            pane_id
        )));
    }

    let events = apply_as_agent(core, engine, DomainIntent::ClosePane { pane_id })?;
    let Some(CoreEvent::PaneClosed {
        pane_id,
        closed,
        cleanup_targets,
    }) = events.into_iter().next()
    else {
        return Err(StructuralFailure::MissingEvent(
            "Core::apply returned no PaneClosed event",
        ));
    };

    if closed {
        cascade_pane_closed_full(state, engine, pane_id, cleanup_targets, false);
    }
    Ok(Closed {
        id: pane_id,
        closed,
    })
}

pub(crate) fn move_tab(
    core: &mut Core,
    engine: &mut CoreState,
    pane_id: u32,
    from_index: usize,
    to_index: usize,
) -> Result<bool, StructuralFailure> {
    let events = apply_as_agent(
        core,
        engine,
        DomainIntent::MoveTab {
            pane_id,
            from_index,
            to_index,
        },
    )?;
    Ok(matches!(
        events.into_iter().next(),
        Some(CoreEvent::TabMoved { moved: true, .. })
    ))
}

/// surface를 닫고 빈 workspace의 재생성을 시도한다.
/// save_snapshot은 검증된 진입점이 정한다. IPC는 false, holder의 사용자 조작은 true를 줄 수 있다.
/// 요청자가 만든 params에서 직접 읽으면 에이전트가 사용자 복원 목록을 채울 수 있다.
/// lifecycle의 is_user_close는 별도이며 이 함수에서는 항상 false다.
pub(crate) fn close_surface(
    core: &mut Core,
    state: &mut dyn CascadeWindow,
    engine: &mut CoreState,
    surface_id: u32,
    save_snapshot: bool,
) -> Result<Closed, StructuralFailure> {
    let intent = DomainIntent::CloseSurface {
        surface_id,
        save_snapshot,
    };
    let events = apply_as_agent(core, engine, intent)?;
    let Some(event @ CoreEvent::SurfaceClosed { surface_id, .. }) = events.into_iter().next()
    else {
        return Err(StructuralFailure::MissingEvent(
            "Core::apply returned no SurfaceClosed event",
        ));
    };

    let Some(c) = SurfaceCloseCascade::from_surface_closed(event, false) else {
        return Ok(Closed {
            id: surface_id,
            closed: false,
        });
    };
    cascade_surface_closed(core, state, engine, c);
    Ok(Closed {
        id: surface_id,
        closed: true,
    })
}
