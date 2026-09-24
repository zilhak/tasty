//! 새 워크스페이스를 만들고 사용자 요청일 때만 활성화한다.

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::core::CoreState;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if let Intent::NewWorkspace {
        kind,
        params,
        category,
    } = &intent.body
    {
        new_workspace(
            core,
            state,
            engine,
            kind.as_deref(),
            params,
            *category,
            &intent.origin,
        );
    }
}

fn new_workspace(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    kind: Option<&str>,
    params: &serde_json::Value,
    category: Option<crate::model::WorkspaceCategoryId>,
    origin: &super::IntentOrigin,
) {
    #[cfg(feature = "gui")]
    let tutorial_setup = matches!(
        origin,
        super::IntentOrigin::User {
            source: super::UserSource::Menu("tutorial.prepare")
        }
    );
    #[cfg(feature = "gui")]
    if tutorial_setup && !state.tutorial.preparing {
        return;
    }
    let kind = kind.unwrap_or("terminal");
    // 새 PTY는 로컬에서 실행하므로 mirror의 원격 cwd는 상속하지 않는다.
    let cwd = if kind == "terminal" && params.is_null() {
        state.resolve_inherit_cwd(engine)
    } else {
        None
    };
    let surface_params = if params.is_null() {
        serde_json::json!({})
    } else {
        params.clone()
    };

    let intent = crate::core::intent::DomainIntent::CreateWorkspace {
        cwd,
        kind: kind.to_string(),
        surface_params,
        name: None,
        subtitle: None,
        description: None,
        category,
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => {
            #[cfg(feature = "gui")]
            if tutorial_setup {
                state.tutorial.preparing = false;
                state.tutorial.setup_error = true;
            }
            tracing::warn!("NewWorkspace kind={kind} failed: {e}");
            return;
        }
    };

    for event in events {
        if let crate::core::intent::CoreEvent::WorkspaceCreated {
            id: workspace_id,
            index,
            surface_id,
            renamed_name,
            renamed_subtitle,
            renamed_description,
        } = event
        {
            #[cfg(feature = "gui")]
            if tutorial_setup {
                if let Some(ws) = engine.workspaces.get(index) {
                    if let Some(pane) = ws.pane_layout().find_pane(ws.focused_pane) {
                        if let Some(tab) = pane.tabs.get(pane.active_tab) {
                            state.tutorial.prepared(
                                crate::adapters::ui::tutorial::PracticeContext {
                                    workspace: workspace_id,
                                    pane: pane.id,
                                    tab: tab.id,
                                },
                            );
                        }
                    }
                }
            }
            crate::app::dispatch_domain::cascade_workspace_created(
                state,
                engine,
                origin,
                0,
                crate::app::dispatch_domain::WorkspaceCreatedCascade {
                    workspace_id,
                    index,
                    surface_id,
                    renamed_name,
                    renamed_subtitle,
                    renamed_description,
                },
            );
        }
    }
}
