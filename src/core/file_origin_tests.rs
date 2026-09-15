use super::tests::build_test_core;
use crate::core::intent::DomainIntent;
use crate::file::dispatch::{DispatchTarget, execute_handler_action, open_surface_tab};
use crate::file::format::{DetectorId, FileTarget};
use crate::file::handler::{FileHandler, HandlerAction, HandlerId, HandlerOwner};
use crate::state::FileHandlerPickerResult;

fn handler(action: HandlerAction) -> FileHandler {
    FileHandler {
        id: HandlerId::new("host/origin-test"),
        detector: DetectorId::new("origin-test"),
        priority: 0,
        owner: HandlerOwner::Host,
        action,
        display_name_i18n_key: None,
        disabled: false,
    }
}

#[test]
fn delayed_picker_selection_uses_origin_pane_after_active_workspace_changes() {
    use tasty_plugin_protocol::host_port::FileHandlerRegistryPort;
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let pane = engine.find_pane_for_surface(sid).unwrap();
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.file_handler.as_ref(),
        "com.example.origin",
        &[
            serde_json::json!({"id":"open", "detector":"origin-test", "priority":0,
            "action":{"kind":"open_surface", "surface_kind":"empty", "param_key":"file"}}),
        ],
    );
    core.apply_identify_result(
        &mut state,
        &mut engine,
        FileTarget::new("/unknown"),
        None,
        Some(sid),
        false,
    );
    let picker = state.dialogs.file_handler_picker.take().unwrap();
    core.apply(
        &mut engine,
        DomainIntent::CreateWorkspace {
            cwd: None,
            kind: "terminal".into(),
            surface_params: serde_json::json!({}),
            name: None,
            subtitle: None,
            description: None,
            category: None,
        },
    )
    .unwrap();
    state.active_workspace = 1;
    let before = engine.workspaces[0].all_surface_ids();
    let active_tab = engine.find_pane_by_id(pane).unwrap().active_tab;
    let focused_surface = state.focused_surface_id(&engine);
    core.apply_file_picker_result(
        &mut state,
        &mut engine,
        picker.target,
        FileHandlerPickerResult::Selected(HandlerId::new("com.example.origin/open")),
        picker.origin_surface_id,
        picker.ignore_size_limit,
    );
    let added = engine.workspaces[0]
        .all_surface_ids()
        .into_iter()
        .find(|id| !before.contains(id))
        .unwrap();
    assert_eq!(engine.find_pane_for_surface(added), Some(pane));
    assert_eq!(state.active_workspace, 1);
    assert_eq!(engine.find_pane_by_id(pane).unwrap().active_tab, active_tab);
    assert_eq!(state.focused_surface_id(&engine), focused_surface);
    assert!(state.pending_intents.is_empty());
}

#[test]
fn a_dead_origin_cannot_execute_any_action_or_enqueue_a_new_tab() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    for action in [
        HandlerAction::OpenSurface {
            surface_kind: "empty".into(),
            param_key: "file".into(),
        },
        HandlerAction::Ipc {
            method: "example.open".into(),
            owner_plugin_id: "example".into(),
        },
        HandlerAction::System,
    ] {
        assert!(!execute_handler_action(
            &mut core,
            &mut state,
            &mut engine,
            &handler(action),
            &DispatchTarget::File(FileTarget::new("/missing")),
            Some(u32::MAX),
            false
        ));
        assert!(state.pending_intents.is_empty());
        assert!(state.pending_handler_ipc.is_empty());
    }
    assert!(!open_surface_tab(
        &mut core,
        &mut state,
        &mut engine,
        "empty",
        serde_json::json!({}),
        Some(u32::MAX)
    ));
    assert!(state.pending_intents.is_empty());
}

#[test]
fn no_origin_retains_the_user_new_tab_path_and_failed_creation_is_not_success() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    assert!(!open_surface_tab(
        &mut core,
        &mut state,
        &mut engine,
        "missing-kind",
        serde_json::json!({}),
        Some(sid)
    ));
    assert!(state.pending_intents.is_empty());
    assert!(open_surface_tab(
        &mut core,
        &mut state,
        &mut engine,
        "empty",
        serde_json::json!({}),
        None
    ));
    assert_eq!(state.pending_intents.len(), 1);
}

#[test]
fn identify_and_picker_keep_origin_and_cancel_or_disappearance_do_not_dispatch() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let pane = engine.find_pane_for_surface(sid).unwrap();
    let target = FileTarget::new("/missing/unknown");
    core.apply_identify_result(
        &mut state,
        &mut engine,
        target.clone(),
        None,
        Some(sid),
        true,
    );
    let picker = state.dialogs.file_handler_picker.take().unwrap();
    assert_eq!(picker.origin_surface_id, Some(sid));
    assert!(picker.ignore_size_limit);
    let recent_before = engine.file_handler_recent.list().len();
    core.apply_file_picker_result(
        &mut state,
        &mut engine,
        picker.target,
        FileHandlerPickerResult::Cancelled,
        picker.origin_surface_id,
        picker.ignore_size_limit,
    );
    assert!(state.pending_intents.is_empty());
    assert_eq!(engine.file_handler_recent.list().len(), recent_before);

    // Preserve the pane while removing the explicitly named origin.
    core.apply(
        &mut engine,
        DomainIntent::CreateTab {
            pane_id: pane,
            cwd: None,
            kind: "empty".into(),
            name: None,
            surface_params: serde_json::json!({}),
        },
    )
    .unwrap();
    core.apply(
        &mut engine,
        DomainIntent::CloseSurface {
            surface_id: sid,
            save_snapshot: false,
        },
    )
    .unwrap();
    assert!(!engine.has_surface(sid));
    core.apply_identify_result(
        &mut state,
        &mut engine,
        target.clone(),
        None,
        Some(sid),
        false,
    );
    assert!(state.dialogs.file_handler_picker.is_none());
    let h = engine
        .file_handler
        .all_handlers()
        .into_iter()
        .next()
        .unwrap();
    core.apply_file_picker_result(
        &mut state,
        &mut engine,
        DispatchTarget::File(target),
        FileHandlerPickerResult::Selected(h.id),
        Some(sid),
        false,
    );
    assert!(state.pending_intents.is_empty());
    assert!(state.pending_handler_ipc.is_empty());
    assert_eq!(engine.file_handler_recent.list().len(), recent_before);
}

#[test]
fn explicit_origin_preserves_the_selected_tab_even_when_origin_is_inactive() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let origin = engine.workspaces[0].all_surface_ids()[0];
    let pane_id = engine.find_pane_for_surface(origin).unwrap();
    // The ordinary CreateTab contract is unchanged: non-terminal creation selects
    // its result. This also makes the saved selection different from the origin.
    core.apply(
        &mut engine,
        DomainIntent::CreateTab {
            pane_id,
            cwd: None,
            kind: "empty".into(),
            name: None,
            surface_params: serde_json::json!({}),
        },
    )
    .unwrap();
    assert_eq!(engine.find_pane_by_id(pane_id).unwrap().active_tab, 1);
    let focused_surface = state.focused_surface_id(&engine);
    assert_ne!(focused_surface, Some(origin));
    for kind in ["empty", "terminal", "missing-kind"] {
        let before = engine.find_pane_by_id(pane_id).unwrap();
        let selected_id = before.tabs[before.active_tab].id;
        let count = before.tabs.len();
        let succeeded = open_surface_tab(
            &mut core,
            &mut state,
            &mut engine,
            kind,
            serde_json::json!({}),
            Some(origin),
        );
        assert_eq!(succeeded, kind != "missing-kind");
        let after = engine.find_pane_by_id(pane_id).unwrap();
        assert_eq!(after.tabs.len(), count + usize::from(succeeded));
        assert_eq!(after.tabs[after.active_tab].id, selected_id, "{kind}");
        assert_eq!(state.focused_surface_id(&engine), focused_surface, "{kind}");
        assert!(state.pending_intents.is_empty());
    }
}
