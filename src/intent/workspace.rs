//! Workspace 도메인 Intent 핸들러.
//!
//! 정책:
//! - **NewWorkspace**: `DomainIntent::CreateWorkspace` 로 forward. cascade
//!   (`cascade_workspace_created`) 가 origin 보고 active 전환 분기 — User
//!   origin 이면 active 전환, Agent/System 이면 background 유지.
//! - **CloseWorkspace**: 미마이그레이션. 사용자 단축키의 cascade 가 `request_close` 결과
//!   에 의존하므로 직접 호출 유지 (intent-exempt).
//! - **ActivateWorkspace**: W1=B per `docs/design/action-dispatch.md` — focus 독립성
//!   원칙. 사용자 단축키/클릭으로만 가능.

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::engine_state::CoreState;
use crate::state::AppState;

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if let Intent::NewWorkspace { kind, params } = &intent.body {
        new_workspace(core, state, engine, kind.as_deref(), params, &intent.origin);
    }
}

fn new_workspace(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    kind: Option<&str>,
    params: &serde_json::Value,
    origin: &super::IntentOrigin,
) {
    let kind = kind.unwrap_or("terminal");
    // 호출자가 cwd 결정 (terminal kind + null params 면 inherit, 그 외 None).
    let cwd = if kind == "terminal" && params.is_null() {
        state.resolve_inherit_cwd(engine)
    } else {
        None
    };
    // Intent 경로는 IPC 와 달리 surface params 가 보통 null — `params.is_null()`
    // 이면 빈 객체로 정규화해 Core 가 일관된 payload 를 받게 한다.
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
    };

    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => {
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
            crate::app::dispatch_domain::cascade_workspace_created(
                state,
                engine,
                origin,
                workspace_id,
                index,
                0,
                surface_id,
                renamed_name,
                renamed_subtitle,
                renamed_description,
            );
        }
    }
}
