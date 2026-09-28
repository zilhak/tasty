use super::remote_openable_handler;
use crate::core::identify_port::IdentifySpawner;
use crate::core::intent::DomainIntent;
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
        &engine.surface_registry,
        plugin_id,
        &decl,
        host_cmd_tx,
    );
}

fn install_handler(engine: &crate::core::CoreState, plugin_id: &str, action: serde_json::Value) {
    FileHandlerRegistryPort::install_plugin_handlers(
        engine.file_handler.as_ref(),
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
) -> (crate::state::AppState, crate::core::CoreState, usize) {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    register_kind(&engine, "com.tasty.markdown", "markdown");
    install_handler(
        &engine,
        "com.tasty.markdown",
        serde_json::json!({"kind": "open_surface", "surface_kind": "markdown", "param_key": "file"}),
    );
    engine.workspaces[0].mirror = true;
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let surfaces = engine.workspaces[0].all_surface_ids().len();
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
    (state, engine, surfaces)
}

#[test]
fn a_mirror_open_forwards_one_user_new_tab_without_local_effects() {
    let (state, engine, surfaces) = apply_on_mirror(FileDispatchOrigin::User);
    assert_eq!(engine.pending_structural_forward.len(), 1);
    let forward = &engine.pending_structural_forward[0];
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
    assert_eq!(engine.workspaces[0].all_surface_ids().len(), surfaces);
    assert_eq!(state.recent_files.get("markdown"), Vec::<String>::new());
    assert!(state.pending_intents.is_empty());
    assert!(state.dialogs.file_handler_picker.is_none());
    assert_eq!(state.toasts.len(), 0);
}

#[test]
fn an_agent_mirror_open_is_not_marked_as_a_user_forward() {
    let (_state, engine, _) = apply_on_mirror(FileDispatchOrigin::Agent);
    assert_eq!(engine.pending_structural_forward.len(), 1);
    let forward = &engine.pending_structural_forward[0];
    assert!(!forward.user_triggered);
    assert!(
        forward.silent_failure,
        "에이전트 요청의 실패는 로그로만 남긴다"
    );
}

#[test]
fn a_mirror_open_with_only_an_ipc_handler_runs_nothing_and_toasts() {
    let (mut core, _) = build_test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    install_handler(
        &engine,
        "com.example.ipc",
        serde_json::json!({"kind": "ipc", "method": "example.open"}),
    );
    engine.workspaces[0].mirror = true;
    let sid = engine.workspaces[0].all_surface_ids()[0];
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
    assert!(engine.pending_structural_forward.is_empty());
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
    let (mut state, mut engine) = crate::state::tests::test_state();
    engine.workspaces[0].mirror = true;
    let sid = engine.workspaces[0].all_surface_ids()[0];
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
    assert!(engine.pending_structural_forward.is_empty());
    assert!(state.dialogs.file_handler_picker.is_none());
    assert_eq!(state.toasts.len(), 1);
}

#[test]
fn only_open_surface_kinds_that_mirror_content_are_remote_openable() {
    let (_state, engine) = crate::state::tests::test_state();
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
            remote_openable_handler(&engine, vec![handler(action.clone())]).is_none(),
            "{action:?} must not open on the remote",
        );
    }
    let picked = remote_openable_handler(
        &engine,
        vec![
            handler(HandlerAction::System),
            handler(open_surface("html")),
            handler(open_surface("markdown")),
        ],
    )
    .expect("markdown is remote-openable");
    assert!(matches!(
        &picked.action,
        HandlerAction::OpenSurface { surface_kind, .. } if surface_kind == "markdown"
    ));
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
    let (mut core, mut engine) = build_test_core();
    let spawner = std::sync::Arc::new(RecordingSpawner::default());
    engine.identify_worker = Some(spawner.clone());
    let sid = engine.workspaces[0].all_surface_ids()[0];
    let dispatch = |depth| DomainIntent::DispatchFile {
        target: FileTarget::new("/remote/doc.md"),
        depth,
        origin_surface_id: Some(sid),
        dispatch_origin: FileDispatchOrigin::User,
        ignore_size_limit: false,
    };

    core.apply(&mut engine, dispatch(DetectDepth::Deep))
        .unwrap();
    engine.workspaces[0].mirror = true;
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
