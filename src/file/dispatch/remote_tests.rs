use super::is_remote_openable;
use crate::core::identify_port::IdentifySpawner;
use crate::app::command::DomainIntent;
use crate::file::dispatch::picker_apply::tests::build_test_core;
use crate::file::dispatch::{FileDispatchOrigin, apply_identify_result};
use crate::file::format::{DetectDepth, DetectorId, FileTarget};
use crate::file::handler::{FileHandler, HandlerAction, HandlerId, HandlerOwner};
use tasty_ipc::stream::StructuralOp;
use tasty_plugin_protocol::host_port::FileHandlerRegistryPort;

fn register_kind(engine: &crate::core::CoreState, plugin_id: &str, kind: &str) {
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": kind,
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "webview",
        "records_recent": true,
    }))
    .expect("kind decl");
    let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.runtime.surface_registry,
        plugin_id,
        &decl,
        host_cmd_tx,
    );
}

fn install_handler(engine: &crate::core::CoreState, plugin_id: &str, action: serde_json::Value) {
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.runtime.file_handler.as_ref(),
        plugin_id,
        &[
            serde_json::json!({"id": "open", "detector": "remote-test", "priority": 0,
            "action": action}),
        ],
    );
}

fn handler(action: HandlerAction) -> FileHandler {
    FileHandler {
        id: HandlerId::new("host/remote-test"),
        detector: DetectorId::new("remote-test"),
        priority: 0,
        owner: HandlerOwner::Host,
        action,
        display_name_i18n_key: None,
        disabled: false,
    }
}

fn open_surface(kind: &str) -> HandlerAction {
    HandlerAction::OpenSurface {
        surface_kind: kind.into(),
        param_key: "file".into(),
    }
}

/// mirror workspace의 첫 surface를 origin으로 식별 결과를 적용한다.
fn apply_on_mirror(
    origin: FileDispatchOrigin,
) -> (
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
    usize,
) {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    register_kind(&engine, "com.tasty.markdown", "markdown");
    install_handler(
        &engine,
        "com.tasty.markdown",
        serde_json::json!({"kind": "open_surface", "surface_kind": "markdown", "param_key": "file"}),
    );
    engine.make_mirror_fixture(0);
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    let surfaces = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()
        .len();
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/doc.md"),
        Some(DetectorId::new("remote-test")),
        Some(sid),
        origin,
        false,
    );
    (state, engine_session, surfaces)
}

#[test]
fn a_mirror_open_forwards_one_user_new_tab_without_local_effects() {
    let (state, mut engine_session, surfaces) = apply_on_mirror(FileDispatchOrigin::User);
    let engine = engine_session.borrow_mut();
    assert_eq!(engine.remote.pending_structural_forward.len(), 1);
    let forward = &engine.remote.pending_structural_forward[0];
    let StructuralOp::NewTab {
        surface_kind,
        params,
        ..
    } = &forward.op
    else {
        panic!("expected a NewTab forward, got {:?}", forward.op);
    };
    assert_eq!(surface_kind, "markdown");
    assert_eq!(params, &serde_json::json!({"file": "/remote/doc.md"}));
    assert!(
        forward.user_triggered,
        "사용자 더블클릭은 원격 새 탭을 선택한다"
    );
    assert!(!forward.silent_failure);
    assert_eq!(
        engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()
            .len(),
        surfaces
    );
    assert_eq!(state.recent_files.get("markdown"), Vec::<String>::new());
    assert!(state.pending_intents.is_empty());
    assert!(state.dialogs.file_handler_picker.is_none());
    assert_eq!(state.toasts.len(), 0);
}

#[test]
fn an_agent_mirror_open_is_not_marked_as_a_user_forward() {
    let (_state, mut engine_session, _) = apply_on_mirror(FileDispatchOrigin::Agent);
    let engine = engine_session.borrow_mut();
    assert_eq!(engine.remote.pending_structural_forward.len(), 1);
    let forward = &engine.remote.pending_structural_forward[0];
    assert!(!forward.user_triggered);
    assert!(
        forward.silent_failure,
        "에이전트 요청의 실패는 로그로만 남긴다"
    );
}

#[test]
fn a_mirror_open_with_only_an_ipc_handler_runs_nothing_and_toasts() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    install_handler(
        &engine,
        "com.example.ipc",
        serde_json::json!({"kind": "ipc", "method": "example.open"}),
    );
    engine.make_mirror_fixture(0);
    let sid = engine
        .workspace_at_mut(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/doc.bin"),
        Some(DetectorId::new("remote-test")),
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert!(engine.remote.pending_structural_forward.is_empty());
    assert!(state.pending_handler_ipc.is_empty());
    assert!(state.pending_intents.is_empty());
    assert!(
        state.dialogs.file_handler_picker.is_none(),
        "로컬 핸들러 전체 picker를 띄우지 않는다"
    );
    assert_eq!(state.toasts.len(), 1);
}

#[test]
fn a_mirror_open_without_a_detector_opens_no_picker() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    engine.make_mirror_fixture(0);
    let sid = engine
        .workspace_at_mut(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/unknown"),
        None,
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert!(engine.remote.pending_structural_forward.is_empty());
    assert!(state.dialogs.file_handler_picker.is_none());
    assert_eq!(state.toasts.len(), 1);
}

#[test]
fn only_open_surface_kinds_that_mirror_content_are_remote_openable() {
    let (_state, mut engine_session) = crate::state::tests::test_state();
    let engine = engine_session.borrow_mut();
    register_kind(&engine, "com.tasty.markdown", "markdown");
    register_kind(&engine, "com.tasty.html", "html");
    // 같은 kind라도 허용 목록과 다른 plugin이 등록했으면 client가 원문을 받지 않는다.
    register_kind(&engine, "com.example.other", "probe_other_markdown");

    for action in [
        HandlerAction::System,
        HandlerAction::Ipc {
            method: "example.open".into(),
            owner_plugin_id: "example".into(),
        },
        open_surface("html"),
        open_surface("probe_other_markdown"),
        open_surface("not_registered"),
    ] {
        assert!(
            !is_remote_openable(&engine, &handler(action.clone())),
            "{action:?} must not open on the remote",
        );
    }
    assert!(is_remote_openable(
        &engine,
        &handler(open_surface("markdown"))
    ));
}

/// 1순위가 원격에 열 수 없는 Ipc이고 2순위가 markdown인 mirror 환경을 만든다.
fn mirror_with_ipc_first() -> (
    crate::app::services::AppServices,
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
    u32,
) {
    let (core, _) = build_test_core();
    let (state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    register_kind(&engine, "com.tasty.markdown", "markdown");
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.runtime.file_handler.as_ref(),
        "com.example.ipc",
        &[
            serde_json::json!({"id": "open", "detector": "remote-test", "priority": 0,
            "action": {"kind": "ipc", "method": "example.open"}}),
        ],
    );
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.runtime.file_handler.as_ref(),
        "com.tasty.markdown",
        &[
            serde_json::json!({"id": "open", "detector": "remote-test", "priority": 10,
            "action": {"kind": "open_surface", "surface_kind": "markdown", "param_key": "file"}}),
        ],
    );
    engine.make_mirror_fixture(0);
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    (core, state, engine_session, sid)
}

#[test]
fn a_user_open_with_an_unopenable_first_handler_shows_only_remote_candidates() {
    let (mut core, mut state, mut engine_session, sid) = mirror_with_ipc_first();
    let mut engine = engine_session.borrow_mut();
    // 최근 목록에 원격에 열 수 없는 핸들러가 있어도 picker에 보이면 안 된다.
    state
        .file_handler_recent
        .record(&HandlerId::new("com.example.ipc/open"));
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/doc.md"),
        Some(DetectorId::new("remote-test")),
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert!(engine.remote.pending_structural_forward.is_empty());
    assert!(state.pending_handler_ipc.is_empty());
    let picker = state
        .dialogs
        .file_handler_picker
        .as_ref()
        .expect("user open shows the remote picker");
    let ids: Vec<&str> = picker
        .candidates
        .iter()
        .chain(picker.recent.iter())
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids, vec!["com.tasty.markdown/open"]);
    assert_eq!(picker.origin_surface_id, Some(sid));
    assert!(picker.default_handler.is_none());
    assert!(!picker.candidates_are_fallback);
    assert_eq!(state.toasts.len(), 0);
}

#[test]
fn an_agent_open_with_an_unopenable_first_handler_runs_the_first_remote_one() {
    let (mut core, mut state, mut engine_session, sid) = mirror_with_ipc_first();
    let mut engine = engine_session.borrow_mut();
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/doc.md"),
        Some(DetectorId::new("remote-test")),
        Some(sid),
        FileDispatchOrigin::Agent,
        false,
    );
    assert!(
        state.dialogs.file_handler_picker.is_none(),
        "에이전트 요청은 사용자 화면에 picker를 띄우지 않는다"
    );
    assert!(state.pending_handler_ipc.is_empty());
    assert_eq!(engine.remote.pending_structural_forward.len(), 1);
    assert!(!engine.remote.pending_structural_forward[0].user_triggered);
}

/// 사용자 입력을 증명하지 못한 plugin 중계 요청도 사용자 클릭일 수 있어 원격 picker를 연다.
/// 확정 뒤에도 같은 출처로 실행하도록 picker에 전달된 출처를 싣는다.
#[test]
fn an_unverified_plugin_open_with_an_unopenable_first_handler_shows_the_remote_picker() {
    let (mut core, mut state, mut engine_session, sid) = mirror_with_ipc_first();
    let mut engine = engine_session.borrow_mut();
    apply_identify_result(
        &mut core,
        &mut state,
        &mut engine,
        FileTarget::new("/remote/doc.md"),
        Some(DetectorId::new("remote-test")),
        Some(sid),
        FileDispatchOrigin::PluginUnverified,
        false,
    );
    assert!(engine.remote.pending_structural_forward.is_empty());
    assert!(state.pending_handler_ipc.is_empty());
    let picker = state
        .dialogs
        .file_handler_picker
        .as_ref()
        .expect("unverified plugin open shows the remote picker");
    assert_eq!(picker.dispatch_origin, FileDispatchOrigin::PluginUnverified);
    assert_eq!(picker.origin_surface_id, Some(sid));
    assert!(picker.default_handler.is_none());
    assert_eq!(state.toasts.len(), 0);
}

/// 원격에 열 수 없으면 사용자와 증명하지 못한 plugin 중계 요청은 toast로, 외부 IPC 요청은 로그로 알린다.
#[test]
fn an_unopenable_remote_file_toasts_unless_an_external_ipc_asked() {
    for (origin, toasts) in [
        (FileDispatchOrigin::User, 1),
        (FileDispatchOrigin::PluginUnverified, 1),
        (FileDispatchOrigin::Agent, 0),
    ] {
        let (mut core, _) = build_test_core();
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        engine.make_mirror_fixture(0);
        let sid = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        apply_identify_result(
            &mut core,
            &mut state,
            &mut engine,
            FileTarget::new("/remote/unknown"),
            None,
            Some(sid),
            origin,
            false,
        );
        assert!(state.dialogs.file_handler_picker.is_none(), "{origin:?}");
        assert!(engine.remote.pending_structural_forward.is_empty(), "{origin:?}");
        assert_eq!(state.toasts.len(), toasts, "{origin:?}");
    }
}

#[test]
fn a_remote_picker_selection_forwards_a_user_new_tab_and_rejects_local_handlers() {
    let (mut core, mut state, mut engine_session, sid) = mirror_with_ipc_first();
    let mut engine = engine_session.borrow_mut();
    let target = crate::file::dispatch::DispatchTarget::File(FileTarget::new("/remote/doc.md"));

    crate::file::dispatch::apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        target.clone(),
        crate::state::FileHandlerPickerResult::Selected(HandlerId::new("com.example.ipc/open")),
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert!(state.pending_handler_ipc.is_empty());
    assert!(engine.remote.pending_structural_forward.is_empty());

    crate::file::dispatch::apply_file_picker_result(
        &mut core,
        &mut state,
        &mut engine,
        target,
        crate::state::FileHandlerPickerResult::Selected(HandlerId::new("com.tasty.markdown/open")),
        Some(sid),
        FileDispatchOrigin::User,
        false,
    );
    assert_eq!(engine.remote.pending_structural_forward.len(), 1);
    let forward = &engine.remote.pending_structural_forward[0];
    let StructuralOp::NewTab {
        surface_kind,
        params,
        ..
    } = &forward.op
    else {
        panic!("expected a NewTab forward, got {:?}", forward.op);
    };
    assert_eq!(surface_kind, "markdown");
    assert_eq!(params, &serde_json::json!({"file": "/remote/doc.md"}));
    assert!(forward.user_triggered);
    assert_eq!(state.recent_files.get("markdown"), Vec::<String>::new());
}

#[derive(Default)]
struct RecordingSpawner(std::sync::Mutex<Vec<(DetectDepth, Option<u32>)>>);

impl IdentifySpawner for RecordingSpawner {
    fn spawn_identify(
        &self,
        _target: FileTarget,
        depth: DetectDepth,
        origin_surface_id: Option<u32>,
        _dispatch_origin: FileDispatchOrigin,
        _ignore_size_limit: bool,
    ) {
        self.0.lock().unwrap().push((depth, origin_surface_id));
    }
}

#[test]
fn a_mirror_origin_identifies_by_name_only() {
    let (mut core, mut engine_session) = build_test_core();
    let mut engine = engine_session.borrow_mut();
    let spawner = std::sync::Arc::new(RecordingSpawner::default());
    engine.runtime.identify_worker = Some(spawner.clone());
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    let dispatch = |depth| DomainIntent::DispatchFile {
        target: FileTarget::new("/remote/doc.md"),
        depth,
        origin_surface_id: Some(sid),
        dispatch_origin: FileDispatchOrigin::User,
        ignore_size_limit: false,
    };

    core.apply(&mut engine, dispatch(DetectDepth::Deep))
        .unwrap();
    engine.make_mirror_fixture(0);
    core.apply(&mut engine, dispatch(DetectDepth::Deep))
        .unwrap();

    assert_eq!(
        *spawner.0.lock().unwrap(),
        vec![
            (DetectDepth::Deep, Some(sid)),
            (DetectDepth::Name, Some(sid))
        ],
        "로컬 origin은 요청한 깊이를, mirror origin은 파일 이름만으로 식별한다"
    );
}
