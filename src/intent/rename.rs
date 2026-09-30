//! 이름 변경 팝업에 사용자가 직접 입력한 이름을 Core::apply로 적용한다.
//! Domain 큐의 후속 처리는 host 이벤트에 user_direct를 싣지 않으므로 이 핸들러가 직접 낸다.

use super::{DispatchedIntent, Intent, IntentOrigin};
use crate::core::Core;
use crate::core::CoreState;
use crate::core::intent::DomainIntent;
use crate::state::AppState;

/// 대상은 ID로 지정한다. 팝업이 열린 동안 순서가 바뀌어도 같은 대상을 바꾼다.
#[cfg_attr(
    all(not(feature = "gui"), not(test)),
    expect(dead_code, reason = "only the gui rename popup raises a direct rename")
)]
#[derive(Debug, Clone)]
pub enum DirectRename {
    WorkspaceName {
        workspace_id: u32,
        name: String,
    },
    WorkspaceSubtitle {
        workspace_id: u32,
        subtitle: String,
    },
    /// None이면 사용자 이름을 지운다.
    TabName {
        tab_id: u32,
        name: Option<String>,
    },
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
        DirectRename::TabName { tab_id, name } => {
            rename_tab(core, state, engine, tab_id, name, &intent.origin)
        }
    }
}

fn rename_tab(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    tab_id: u32,
    name: Option<String>,
    origin: &IntentOrigin,
) {
    if let Err(e) = core.apply(engine, DomainIntent::RenameTab { tab_id, name }) {
        super::report_apply_error(state, engine, origin, "DirectRename tab", &e);
        return;
    }
    let title = engine
        .find_pane_for_tab(tab_id)
        .and_then(|pane_id| engine.find_pane_by_id(pane_id))
        .and_then(|pane| pane.tabs.iter().find(|t| t.id == tab_id))
        .map(|tab| {
            tab.display_name(state.navigation.surface_id(tab))
                .to_string()
        })
        .unwrap_or_default();
    state.enqueue_host_event(crate::state::PendingHostEvent::TabRenamed {
        tab_id,
        title,
        user_direct: origin.is_user(),
    });
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

    fn first_tab(engine: &CoreState) -> (u32, u32) {
        let sid = engine.workspaces[0].all_surface_ids()[0];
        let pane_id = engine.find_pane_for_surface(sid).expect("pane");
        let tab = &engine.find_pane_by_id(pane_id).expect("pane").tabs[0];
        (pane_id, tab.id)
    }

    fn tab_renamed(state: &AppState) -> Vec<(u32, String, bool)> {
        state
            .pending_host_events
            .iter()
            .filter_map(|e| match e {
                PendingHostEvent::TabRenamed {
                    tab_id,
                    title,
                    user_direct,
                } => Some((*tab_id, title.clone(), *user_direct)),
                _ => None,
            })
            .collect()
    }

    fn run_tab(name: Option<&str>, mirror: bool) -> (AppState, CoreState, u32, u32) {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        engine.workspaces[0].mirror = mirror;
        let (pane_id, tab_id) = first_tab(&engine);
        let intent = Intent::DirectRename(DirectRename::TabName {
            tab_id,
            name: name.map(str::to_string),
        })
        .from_user_menu("test");
        handle(&mut core, &mut state, &mut engine, &intent);
        (state, engine, pane_id, tab_id)
    }

    #[test]
    fn a_tab_name_is_applied_with_a_user_direct_event() {
        let (state, engine, pane_id, tab_id) = run_tab(Some("T"), false);
        let tab = &engine.find_pane_by_id(pane_id).unwrap().tabs[0];
        assert_eq!(tab.explicit_name.as_deref(), Some("T"));
        assert!(engine.layout_dirty.is_dirty());
        assert_eq!(tab_renamed(&state), [(tab_id, "T".to_string(), true)]);
    }

    #[test]
    fn clearing_a_tab_name_returns_to_the_surface_title() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (pane_id, tab_id) = first_tab(&engine);
        engine.find_pane_by_id_mut(pane_id).unwrap().tabs[0].explicit_name = Some("OLD".into());
        let intent = Intent::DirectRename(DirectRename::TabName { tab_id, name: None })
            .from_user_menu("test");
        handle(&mut core, &mut state, &mut engine, &intent);
        let tab = &engine.find_pane_by_id(pane_id).unwrap().tabs[0];
        assert_eq!(tab.explicit_name, None);
        let title = tab
            .display_name(state.navigation.surface_id(tab))
            .to_string();
        assert_eq!(tab_renamed(&state), [(tab_id, title, true)]);
    }

    #[test]
    fn a_mirror_tab_is_still_renamed_locally() {
        let (state, engine, pane_id, _) = run_tab(Some("M"), true);
        let tab = &engine.find_pane_by_id(pane_id).unwrap().tabs[0];
        assert_eq!(tab.explicit_name.as_deref(), Some("M"));
        assert_eq!(tab_renamed(&state).len(), 1);
    }

    #[test]
    fn a_missing_tab_emits_no_event() {
        let (mut state, mut engine) = crate::state::tests::test_state();
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let intent = Intent::DirectRename(DirectRename::TabName {
            tab_id: 999_999,
            name: Some("X".into()),
        })
        .from_user_menu("test");
        handle(&mut core, &mut state, &mut engine, &intent);
        assert!(tab_renamed(&state).is_empty());
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
