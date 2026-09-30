//! 포커스된 pane에 탭을 추가한다. 사용자 요청일 때만 새 탭을 선택한다.
//! IPC tab.create는 이 큐를 거치지 않고 별도로 처리한다.

use super::{DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::core::CoreState;
use crate::state::RequestContext;

pub fn handle(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if let Intent::NewTab { kind, params } = &intent.body {
        new_tab(core, state, engine, kind.as_deref(), params, &intent.origin)
    }
}

fn new_tab(
    core: &mut Core,
    state: &mut RequestContext,
    engine: &mut CoreState,
    kind: Option<&str>,
    params: &serde_json::Value,
    origin: &IntentOrigin,
) {
    let kind = kind.unwrap_or("terminal");
    let pane_id = state.focused_pane_id(engine);
    let cwd = if kind == "terminal" {
        state.resolve_inherit_cwd(engine)
    } else {
        None
    };
    let surface_params = if params.is_null() {
        serde_json::json!({})
    } else {
        params.clone()
    };

    let records_recent = engine
        .surface_registry
        .get(kind)
        .is_some_and(|d| d.records_recent);
    let recent_params = records_recent.then(|| surface_params.clone());

    let intent = crate::core::intent::DomainIntent::CreateTab {
        pane_id,
        cwd,
        kind: kind.to_string(),
        name: None,
        surface_params,
        // 에이전트 요청으로 사용자가 보던 탭을 바꾸지 않는다.
        activate: origin.is_user(),
    };
    // mirror pane의 탭은 원격에 만들어지므로 경로도 원격 파일이다. 로컬 최근 목록에서 다시 열 수 없다.
    // Core::apply가 forward를 정하는 판정과 같은 기준을 쓴다.
    if let Some(params) = recent_params
        && engine
            .mirror_workspace_index_for_structural(&intent)
            .is_none()
    {
        state.record_recent(kind, &params);
    }
    match crate::app::structural_exec::execute(core, state, engine, intent) {
        Ok(events) => {
            crate::app::structural_exec::select_created_tabs(
                state,
                engine,
                &events,
                origin.is_user(),
            );

            #[cfg(feature = "gui")]
            if origin.is_user() {
                for event in &events {
                    if let crate::core::intent::CoreEvent::TabCreated {
                        pane_id, tab_id, ..
                    } = event
                    {
                        state.observe_tutorial_tab_created(engine, *pane_id, *tab_id);
                    }
                }
            }
            // Headless builds have no tutorial observer; Core already applied the mutation.
            let _ = events;
        }
        Err(e) => {
            crate::core::mark_last_forward_user_triggered(engine, &e, origin);
            super::report_apply_error(state, engine, origin, &format!("NewTab kind={kind}"), &e);
        }
    }
}
