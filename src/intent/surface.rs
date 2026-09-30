//! Surface 분할·변환 Intent를 Core에 전달한다.

use super::{ConvertTarget, DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::core::CoreState;
use crate::core::engine_access::EngineMut;
use crate::model::SplitDirection;
use crate::state::RequestContext;

pub fn handle(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    intent: &DispatchedIntent,
) {
    match &intent.body {
        Intent::SplitSurface { direction } => {
            split(core, state, engine, *direction, &intent.origin)
        }
        Intent::ConvertSurface { surface_id, target } => {
            convert(core, state, engine, *surface_id, target, &intent.origin)
        }
        _ => {}
    }
}

fn split(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    direction: SplitDirection,
    origin: &IntentOrigin,
) {
    let Some(sid) = state.focused_surface_id(engine) else {
        tracing::warn!("SplitSurface: no focused surface");
        return;
    };
    let cwd = state.resolve_inherit_cwd_from_surface(&engine.as_ref(), sid);
    let intent = crate::core::intent::DomainIntent::SplitSurface {
        target_surface_id: sid,
        direction,
        cwd,
        kind: "terminal".to_string(),
        surface_params: serde_json::json!({}),
    };
    let events = match crate::app::structural_exec::execute(core, state, engine, intent) {
        Ok(e) => e,
        Err(e) => {
            crate::core::mark_last_forward_user_triggered(engine, &e, origin);
            super::report_apply_error(state, engine, origin, "SplitSurface", &e);
            return;
        }
    };
    for ev in events {
        if let crate::core::intent::CoreEvent::SurfaceSplit {
            workspace_index,
            pane_id,
            new_surface_id,
            ..
        } = ev
        {
            crate::app::structural_cascade::cascade_surface_split(
                state,
                engine,
                origin,
                workspace_index,
                pane_id,
                new_surface_id,
            );
        }
    }
}

fn convert(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut EngineMut<'_>,
    surface_id: u32,
    target: &ConvertTarget,
    origin: &IntentOrigin,
) {
    use crate::core::intent::ConvertSurfaceTarget;

    let domain_target = match target {
        ConvertTarget::Terminal => {
            let cwd = state.resolve_inherit_cwd(&engine.as_ref());
            ConvertSurfaceTarget::Terminal { cwd }
        }
        ConvertTarget::Kind { cwd, kind, params } => {
            // file_path 같은 별칭을 등록된 kind가 사용하는 키로 정규화한다.
            let mut params = params.clone();
            if let Some(def) = engine.surface_registry.get(kind) {
                def.normalize_param_aliases(&mut params);
            }
            // cwd가 생략되고 상속 설정이 켜져 있으면 변환할 surface의 로컬 cwd를 사용한다.
            let resolved_cwd = cwd
                .clone()
                .or_else(|| state.resolve_inherit_cwd_from_surface(&engine.as_ref(), surface_id));
            // 적용 전에 기록하므로 철회된 kind는 get_live로 제외한다.
            // 별칭 정규화는 저장을 하지 않아 위에서는 get을 사용해도 된다.
            // mirror surface의 경로는 원격 파일이라 로컬 최근 목록에서 다시 열 수 없다.
            if !engine.is_mirror_surface(surface_id)
                && engine
                    .surface_registry
                    .get_live(kind)
                    .is_some_and(|d| d.records_recent)
            {
                state.record_recent(kind, &params);
            }
            ConvertSurfaceTarget::Kind {
                cwd: resolved_cwd,
                kind: kind.clone(),
                params,
            }
        }
    };

    let intent = crate::core::intent::DomainIntent::ConvertSurface {
        surface_id,
        target: domain_target,
    };
    if let Err(e) = crate::app::structural_exec::execute(core, state, engine, intent) {
        super::report_apply_error(state, engine, origin, "ConvertSurface", &e);
    }
}
