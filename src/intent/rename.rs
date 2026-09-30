//! 이름 변경 팝업에 사용자가 직접 입력한 이름을 Core::apply로 적용한다.
//! Domain 큐의 후속 처리는 host 이벤트에 user_direct를 싣지 않으므로 이 핸들러가 직접 낸다.

use super::{DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::core::CoreState;
use crate::core::intent::DomainIntent;
use crate::state::AppState;

/// 대상은 ID로 지정한다. 팝업이 열린 동안 순서가 바뀌어도 같은 대상을 바꾼다.
#[cfg_attr(
    not(feature = "gui"),
    expect(dead_code, reason = "only the gui rename popup raises a direct rename")
)]
#[derive(Debug, Clone)]
pub enum DirectRename {
    WorkspaceName { workspace_id: u32, name: String },
    WorkspaceSubtitle { workspace_id: u32, subtitle: String },
}

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    let Intent::DirectRename(rename) = &intent.body else {
        return;
    };
    match rename.clone() {
        DirectRename::WorkspaceName { workspace_id, name } => rename_workspace(
            core,
            state,
            engine,
            workspace_id,
            Some(name),
            None,
            &intent.origin,
        ),
        DirectRename::WorkspaceSubtitle {
            workspace_id,
            subtitle,
        } => rename_workspace(
            core,
            state,
            engine,
            workspace_id,
            None,
            Some(subtitle),
            &intent.origin,
        ),
    }
}

fn rename_workspace(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    workspace_id: u32,
    name: Option<String>,
    subtitle: Option<String>,
    origin: &IntentOrigin,
) {
    let intent = DomainIntent::UpdateWorkspaceMeta {
        workspace_id,
        name: name.clone(),
        subtitle: subtitle.clone(),
        description: None,
    };
    if let Err(e) = core.apply(engine, intent) {
        super::report_apply_error(state, engine, origin, "DirectRename workspace", &e);
        return;
    }
    state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
        workspace_id,
        name,
        subtitle,
        description: None,
        user_direct: origin.is_user(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PendingHostEvent;

    fn run(make: impl FnOnce(u32) -> DirectRename, mirror: bool) -> (AppState, CoreState) {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        engine.workspaces[0].mirror = mirror;
        engine.layout_dirty.clear();
        let rename = make(engine.workspaces[0].id);
        let intent = Intent::DirectRename(rename).from_user_menu("test");
        handle(&mut core, &mut state, &mut engine, &intent);
        (state, engine)
    }

    fn renamed_events(state: &AppState) -> Vec<PendingHostEvent> {
        state
            .pending_host_events
            .iter()
            .filter(|e| matches!(e, PendingHostEvent::WorkspaceRenamed { .. }))
            .cloned()
            .collect()
    }

    #[test]
    fn a_workspace_name_is_applied_with_a_user_direct_event() {
        let (state, engine) = run(
            |workspace_id| DirectRename::WorkspaceName {
                workspace_id,
                name: "N".into(),
            },
            false,
        );
        assert_eq!(engine.workspaces[0].name, "N");
        assert!(engine.layout_dirty.is_dirty());
        assert!(matches!(
            renamed_events(&state).as_slice(),
            [PendingHostEvent::WorkspaceRenamed {
                name: Some(n),
                subtitle: None,
                description: None,
                user_direct: true,
                ..
            }] if n == "N"
        ));
    }

    #[test]
    fn a_workspace_subtitle_is_applied_with_a_user_direct_event() {
        let (state, engine) = run(
            |workspace_id| DirectRename::WorkspaceSubtitle {
                workspace_id,
                subtitle: "S".into(),
            },
            false,
        );
        assert_eq!(engine.workspaces[0].subtitle, "S");
        assert!(matches!(
            renamed_events(&state).as_slice(),
            [PendingHostEvent::WorkspaceRenamed {
                name: None,
                subtitle: Some(s),
                user_direct: true,
                ..
            }] if s == "S"
        ));
    }

    #[test]
    fn a_mirror_workspace_is_still_renamed_locally() {
        let (state, engine) = run(
            |workspace_id| DirectRename::WorkspaceName {
                workspace_id,
                name: "M".into(),
            },
            true,
        );
        assert_eq!(engine.workspaces[0].name, "M");
        assert_eq!(renamed_events(&state).len(), 1);
    }
}
