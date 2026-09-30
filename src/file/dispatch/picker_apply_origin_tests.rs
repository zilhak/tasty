use super::tests::build_test_core;
use super::{apply_file_picker_result, apply_identify_result};
use crate::core::intent::DomainIntent;
use crate::file::dispatch::{
    DispatchTarget, FileDispatchOrigin, execute_handler_action, open_surface_tab,
};
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

/// 식별 결과가 picker를 열면 요청 출처를 그대로 실은 OpenPopup 하나만 큐에 남는다.
fn take_picker_open_request(state: &mut crate::state::RequestContext, agent: bool) {
    let pending = state.take_pending_intents();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        &pending[0].body,
        crate::intent::Intent::Ui(crate::intent::UiIntent::OpenPopup { id, .. })
            if *id == crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID
    ));
    assert_eq!(pending[0].origin.is_agent(), agent);
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
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/unknown"),
        None,
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    let picker = state.dialogs.file_handler_picker.take().unwrap();
    take_picker_open_request(&mut state, false);
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
    state.set_active_workspace_index(&engine, 1);
    let before = engine.workspaces[0].all_surface_ids();
    let active_tab = state
        .navigation
        .tab_index(engine.find_pane_by_id(pane).unwrap());
    let focused_surface = state.focused_surface_id(&engine);
    apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        picker.target,
        FileHandlerPickerResult::Selected(HandlerId::new("com.example.origin/open")),
        picker.origin_surface_id,
        picker.dispatch_origin,
        picker.ignore_size_limit,
    );
    let added = engine.workspaces[0]
        .all_surface_ids()
        .into_iter()
        .find(|id| !before.contains(id))
        .unwrap();
    assert_eq!(engine.find_pane_for_surface(added), Some(pane));
    // 사용자 선택은 origin pane의 결과 탭을 고르지만 활성 workspace와 포커스는 옮기지 않는다.
    let after = engine.find_pane_by_id(pane).unwrap();
    assert_ne!(state.navigation.tab_index(after), active_tab);
    assert_eq!(state.navigation.tab_index(after), after.tabs.len() - 1);
    assert_eq!(state.active_workspace_index(&engine), 1);
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
            FileDispatchOrigin::Agent,
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
        Some(u32::MAX),
        FileDispatchOrigin::Agent
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
        Some(sid),
        FileDispatchOrigin::User
    ));
    assert!(state.pending_intents.is_empty());
    assert!(open_surface_tab(
        &mut core,
        &mut state,
        &mut engine,
        "empty",
        serde_json::json!({}),
        None,
        FileDispatchOrigin::User
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
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        target.clone(),
        None,
        Some(sid),
        FileDispatchOrigin::User,
        true,
    );
    let picker = state.dialogs.file_handler_picker.take().unwrap();
    take_picker_open_request(&mut state, false);
    assert_eq!(picker.origin_surface_id, Some(sid));
    assert!(picker.ignore_size_limit);
    let recent_before = state.file_handler_recent.list().len();
    apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        picker.target,
        FileHandlerPickerResult::Cancelled,
        picker.origin_surface_id,
        picker.dispatch_origin,
        picker.ignore_size_limit,
    );
    assert!(state.pending_intents.is_empty());
    assert_eq!(state.file_handler_recent.list().len(), recent_before);

    // pane은 남기고 명시 origin만 지워 포커스 대체 실행 여부를 확인한다.
    core.apply(
        &mut engine,
        DomainIntent::CreateTab {
            pane_id: pane,
            cwd: None,
            kind: "empty".into(),
            name: None,
            surface_params: serde_json::json!({}),
            activate: true,
        },
    )
    .unwrap();
    core.apply(
        &mut engine,
        DomainIntent::CloseSurface {
            surface_id: sid,
            presentation: None,
        },
    )
    .unwrap();
    assert!(!engine.has_surface(sid));
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        target.clone(),
        None,
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert!(state.dialogs.file_handler_picker.is_none());
    let h = engine
        .file_handler
        .all_handlers()
        .into_iter()
        .next()
        .unwrap();
    apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        DispatchTarget::File(target),
        FileHandlerPickerResult::Selected(h.id),
        Some(sid),
        FileDispatchOrigin::Agent,
        false,
    );
    assert!(state.pending_intents.is_empty());
    assert!(state.pending_handler_ipc.is_empty());
    assert_eq!(state.file_handler_recent.list().len(), recent_before);
}

/// 에이전트가 연 결과는 원래 사용자 선택을 유지해야 한다.
#[test]
fn agent_origin_preserves_the_selected_tab_even_when_origin_is_inactive() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let origin = engine.workspaces[0].all_surface_ids()[0];
    let pane_id = engine.find_pane_for_surface(origin).unwrap();
    // 원래 origin과 선택된 탭을 다르게 둔다.
    core.apply(
        &mut engine,
        DomainIntent::CreateTab {
            pane_id,
            cwd: None,
            kind: "empty".into(),
            name: None,
            surface_params: serde_json::json!({}),
            activate: true,
        },
    )
    .unwrap();
    assert_eq!(
        state
            .navigation
            .tab_index(engine.find_pane_by_id(pane_id).unwrap()),
        1
    );
    let focused_surface = state.focused_surface_id(&engine);
    assert_ne!(focused_surface, Some(origin));
    for kind in ["empty", "terminal", "missing-kind"] {
        let before = engine.find_pane_by_id(pane_id).unwrap();
        let selected_id = before.tabs[state.navigation.tab_index(before)].id;
        let count = before.tabs.len();
        let succeeded = open_surface_tab(
            &mut core,
            &mut state,
            &mut engine,
            kind,
            serde_json::json!({}),
            Some(origin),
            FileDispatchOrigin::Agent,
        );
        assert_eq!(succeeded, kind != "missing-kind");
        let after = engine.find_pane_by_id(pane_id).unwrap();
        assert_eq!(after.tabs.len(), count + usize::from(succeeded));
        assert_eq!(
            after.tabs[state.navigation.tab_index(after)].id,
            selected_id,
            "{kind}"
        );
        assert_eq!(state.focused_surface_id(&engine), focused_surface, "{kind}");
        assert!(state.pending_intents.is_empty());
    }
}

/// 사용자 결과는 선택돼야 하며 origin의 pane에 추가돼야 한다. 선택만 보면 잘못된 pane을 놓친다.
#[test]
fn a_user_origin_selects_its_result_tab() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let origin = engine.workspaces[0].all_surface_ids()[0];
    let pane_id = engine.find_pane_for_surface(origin).unwrap();
    let before = engine.find_pane_by_id(pane_id).unwrap();
    let selected_before = before.tabs[state.navigation.tab_index(before)].id;
    let count = before.tabs.len();

    assert!(open_surface_tab(
        &mut core,
        &mut state,
        &mut engine,
        "empty",
        serde_json::json!({}),
        Some(origin),
        FileDispatchOrigin::User,
    ));

    let after = engine.find_pane_by_id(pane_id).unwrap();
    assert_eq!(
        after.tabs.len(),
        count + 1,
        "origin 의 pane 에 하나 늘어야 한다"
    );
    assert_ne!(
        after.tabs[state.navigation.tab_index(after)].id,
        selected_before,
        "사용자가 연 결과는 선택돼야 한다"
    );
    assert_eq!(
        after.tabs[state.navigation.tab_index(after)].id,
        after.tabs[after.tabs.len() - 1].id,
        "선택은 방금 append 된 탭이어야 한다"
    );
    assert!(state.pending_intents.is_empty());
}

#[test]
fn user_dispatch_and_remote_placeholder_open_the_picker_as_user_requests() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/unknown"),
        None,
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    take_picker_open_request(&mut state, false);

    crate::file::dispatch::open_remote_placeholder_picker(
        &mut state,
        FileTarget::new("/remote/a.md"),
    );
    take_picker_open_request(&mut state, false);
}

/// 매칭 핸들러가 없어도 에이전트 요청은 사용자 화면에 picker를 띄우지 않고 아무것도 실행하지 않는다.
#[test]
fn agent_dispatch_without_a_matching_handler_opens_no_picker() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let recent_before = state.file_handler_recent.list().len();
    for origin_surface_id in [Some(sid), None] {
        for detector in [None, Some(DetectorId::new("no-such-detector"))] {
            apply_identify_result(
                &mut core,
                &mut state,
                &mut engine,
                FileTarget::new("/unknown"),
                detector,
                origin_surface_id,
                FileDispatchOrigin::Agent,
                false,
            );
            assert!(state.dialogs.file_handler_picker.is_none());
            assert!(state.pending_intents.is_empty());
            assert!(state.pending_handler_ipc.is_empty());
        }
    }
    assert_eq!(state.file_handler_recent.list().len(), recent_before);
}

/// 사용자 입력을 증명하지 못한 plugin 중계 요청은 사용자 클릭일 수 있어 무매칭이면 fallback picker를 연다.
/// 확정해도 출처는 그대로라 결과 탭으로 선택을 옮기지 않는다.
#[test]
fn an_unverified_plugin_dispatch_without_a_matching_handler_opens_the_fallback_picker() {
    use tasty_plugin_protocol::host_port::FileHandlerRegistryPort;
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.file_handler.as_ref(),
        "com.example.picker",
        &[
            serde_json::json!({"id": "open", "detector": "picker-test", "priority": 0,
            "action": {"kind": "open_surface", "surface_kind": "empty", "param_key": "file"}}),
        ],
    );
    let sid = engine.workspaces[0].all_surface_ids()[0];
    for detector in [None, Some(DetectorId::new("no-such-detector"))] {
        apply_identify_result(
            &mut core,
            &mut state,
            &mut engine,
            FileTarget::new("/unknown"),
            detector,
            Some(sid),
            FileDispatchOrigin::PluginUnverified,
            false,
        );
        let picker = state
            .dialogs
            .file_handler_picker
            .take()
            .expect("unverified plugin dispatch opens the fallback picker");
        assert!(picker.candidates_are_fallback);
        assert_eq!(picker.dispatch_origin, FileDispatchOrigin::PluginUnverified);
        assert_eq!(picker.origin_surface_id, Some(sid));
        take_picker_open_request(&mut state, true);
    }

    let pane_id = engine.find_pane_for_surface(sid).unwrap();
    let before = engine.find_pane_by_id(pane_id).unwrap();
    let selected_id = before.tabs[state.navigation.tab_index(before)].id;
    let count = before.tabs.len();
    let picked = engine
        .file_handler
        .all_handlers()
        .into_iter()
        .find(|h| {
            matches!(&h.action, HandlerAction::OpenSurface { surface_kind, .. }
                if surface_kind == "empty")
        })
        .expect("installed open_surface handler");
    apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        DispatchTarget::File(FileTarget::new("/unknown")),
        FileHandlerPickerResult::Selected(picked.id),
        Some(sid),
        FileDispatchOrigin::PluginUnverified,
        false,
    );
    let after = engine.find_pane_by_id(pane_id).unwrap();
    assert_eq!(after.tabs.len(), count + 1, "선택한 핸들러가 탭을 연다");
    assert_eq!(
        after.tabs[state.navigation.tab_index(after)].id,
        selected_id,
        "증명하지 못한 요청의 결과는 사용자 선택을 옮기지 않는다"
    );
}
