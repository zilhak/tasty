//! 포커스된 pane의 분할을 Core에 요청하고 origin에 따라 후속 포커스를 처리한다.

use super::{DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::core::CoreState;
use crate::model::SplitDirection;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if let Intent::SplitPane { direction } = &intent.body {
        split(core, state, engine, *direction, &intent.origin);
    }
}

fn split(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    direction: SplitDirection,
    origin: &IntentOrigin,
) {
    let pane_id = state.active_workspace(engine).focused_pane;
    let cwd = state.resolve_inherit_cwd(engine);
    let intent = crate::core::intent::DomainIntent::SplitPane {
        target_pane_id: pane_id,
        direction,
        cwd,
        kind: "terminal".to_string(),
        surface_params: serde_json::json!({}),
    };
    let events = match core.apply(engine, intent) {
        Ok(e) => e,
        Err(e) => {
            crate::core::mark_last_forward_user_triggered(engine, &e, origin);
            super::report_apply_error(state, engine, origin, "SplitPane", &e);
            return;
        }
    };
    for ev in events {
        if let crate::core::intent::CoreEvent::PaneSplit {
            workspace_index,
            original_pane_id,
            new_pane_id,
            new_surface_id,
            direction,
        } = ev
        {
            crate::core::structural_cascade::cascade_pane_split(
                state,
                engine,
                origin,
                crate::core::structural_cascade::PaneSplitCascade {
                    workspace_index,
                    original_pane_id,
                    new_pane_id,
                    new_surface_id,
                    direction,
                },
            );
        }
    }
}
