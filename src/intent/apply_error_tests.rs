//! 사용자 요청 실패만 토스트로 표시하는지 검사한다.
//! 토스트와 attach 클라이언트가 필요한 GUI 전용 시험이다.

use super::*;
use crate::core::engine_access::EngineMut;

fn fixture() -> (
    crate::core::Core,
    crate::state::RequestContext,
    crate::core::CoreState,
) {
    let (state, engine) = crate::state::tests::test_state();
    (
        crate::ipc::handler::cli_entry_tests::test_core(),
        state,
        engine,
    )
}

fn blocked(forwarded: bool) -> anyhow::Error {
    anyhow::Error::new(crate::core::MirrorStructuralBlocked {
        workspace_index: 0,
        forwarded,
    })
}

fn agent() -> IntentOrigin {
    IntentOrigin::Agent {
        source: AgentSource::Ipc,
    }
}

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::Shortcut("test"),
    }
}

#[test]
fn an_unforwardable_block_toasts_only_for_the_user() {
    let (_core, mut state, mut engine) = fixture();
    report_apply_error(&mut state, &mut engine, &agent(), "t", &blocked(false));
    assert_eq!(
        state.toasts.len(),
        0,
        "에이전트 요청 차단은 토스트로 표시하지 않는다"
    );

    report_apply_error(&mut state, &mut engine, &user(), "t", &blocked(false));
    assert_eq!(
        state.toasts.len(),
        1,
        "사용자 요청 차단은 토스트로 표시한다"
    );
}

#[test]
fn a_withdrawn_kind_refusal_toasts_only_for_the_user() {
    let (_core, mut state, mut engine) = fixture();
    let err = anyhow::Error::new(crate::core::surface_registry::SurfaceKindWithdrawn {
        kind: "markdown".to_string(),
        plugin_id: "com.tasty.markdown".to_string(),
    });
    report_apply_error(&mut state, &mut engine, &agent(), "t", &err);
    assert_eq!(
        state.toasts.len(),
        0,
        "에이전트 요청 거절은 토스트로 표시하지 않는다"
    );

    report_apply_error(&mut state, &mut engine, &user(), "t", &err);
    assert_eq!(
        state.toasts.len(),
        1,
        "사용자 요청 거절은 사유를 토스트로 표시한다"
    );
}

/// 원격 실패 표시를 생략하는 표지가 에이전트 요청에만 붙는지 검사한다.
#[test]
fn an_agent_forward_is_marked_for_a_silent_failure() {
    let (mut core, mut state, mut engine) = fixture();
    let surface_id = *state
        .active_workspace(&engine)
        .all_surface_ids()
        .first()
        .expect("fixture surface");
    engine.workspaces[0].mirror = true;
    let file = tempfile::NamedTempFile::new().expect("tmp file");

    let req = crate::ipc::protocol::JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        jsonrpc: "2.0".to_string(),
        method: "markdown.navigate".to_string(),
        params: serde_json::json!({
            "surface_id": surface_id,
            "path": file.path().to_string_lossy(),
        }),
        id: Some(serde_json::json!(1)),
        session_token: None,
    };
    let resp = crate::ipc::handler::handle_with_caller(
        &mut core,
        &mut state,
        &mut engine,
        &req,
        &crate::ipc::caller::CallerContext::Local,
    );
    assert!(resp.error.is_none(), "navigate: {:?}", resp.error);
    crate::intent::headless::drain_pending_intents(&mut core, &mut state, &mut engine);

    assert_eq!(
        engine.pending_structural_forward.len(),
        1,
        "변환 요청을 원격으로 전달한다"
    );
    assert!(
        engine.pending_structural_forward[0].silent_failure,
        "에이전트 요청의 원격 실패는 로그로 남긴다"
    );
    assert_eq!(state.toasts.len(), 0);

    engine.pending_structural_forward.clear();
    crate::intent::surface::handle(
        &mut core,
        &mut state,
        &mut engine,
        &Intent::ConvertSurface {
            surface_id,
            target: ConvertTarget::Terminal,
        }
        .from_user_menu("test"),
    );
    assert_eq!(engine.pending_structural_forward.len(), 1);
    assert!(!engine.pending_structural_forward[0].silent_failure);
}

/// mirror workspace의 MoveSurface를 전달한 뒤 events_or_report처럼 apply 오류를 그대로 넘긴다.
/// 공용 보고 함수만 거쳐도 사용자 요청이면 user_triggered가 켜지고 에이전트 요청이면 꺼진 채 남아야 한다.
#[test]
fn a_forwarded_block_is_user_triggered_only_for_the_user() {
    for (origin, expect_user) in [(user(), true), (agent(), false)] {
        let (mut core, mut state, mut engine) = fixture();
        let source = state.focused_surface_id(&engine).expect("fixture surface");
        state.add_tab(&mut engine).expect("second tab");
        let target = state.focused_surface_id(&engine).expect("second surface");
        assert_ne!(source, target);
        engine.workspaces[0].mirror = true;

        let err = core
            .apply(
                &mut engine,
                crate::core::intent::DomainIntent::MoveSurface {
                    source_surface_id: source,
                    target_surface_id: target,
                },
            )
            .expect_err("mirror workspace는 로컬에서 옮기지 않는다");
        assert!(
            err.downcast_ref::<crate::core::MirrorStructuralBlocked>()
                .is_some_and(|b| b.forwarded),
            "원격으로 전달한 차단이어야 한다: {err}"
        );
        assert_eq!(engine.pending_structural_forward.len(), 1);
        assert!(!engine.pending_structural_forward[0].user_triggered);

        report_apply_error(&mut state, &mut engine, &origin, "t", &err);
        let queued = &engine.pending_structural_forward[0];
        assert_eq!(
            queued.user_triggered, expect_user,
            "{origin:?}: 사용자 요청만 사용자 조작으로 전달한다"
        );
        assert_eq!(
            queued.silent_failure, !expect_user,
            "{origin:?}: 에이전트 요청만 원격 실패를 로그로 남긴다"
        );
        assert_eq!(
            state.toasts.len(),
            0,
            "전달한 요청은 여기서 토스트를 띄우지 않는다"
        );
    }
}

#[test]
fn a_preset_apply_failure_toasts_only_for_the_user() {
    let (core, mut state, mut engine) = fixture();
    let apply = || Intent::ApplyPreset {
        kind: tasty_presets::PresetKind::Tab,
        name: "no-such-preset-apply-error-tests".to_string(),
        category: None,
    };
    preset::handle(&core, &mut state, &mut engine, &apply().from_agent_ipc());
    assert_eq!(
        state.toasts.len(),
        0,
        "에이전트의 프리셋 적용 실패는 토스트로 표시하지 않는다"
    );

    preset::handle(
        &core,
        &mut state,
        &mut engine,
        &apply().from_user_menu("test"),
    );
    assert_eq!(
        state.toasts.len(),
        1,
        "사용자의 프리셋 적용 실패는 토스트로 표시한다"
    );
}

// 이름 검사에서 거절되므로 디스크에 쓰지 않고 저장 실패를 시험한다.
#[test]
fn a_preset_save_failure_toasts_only_for_the_user() {
    let (core, mut state, mut engine) = fixture();
    let ws = &engine.workspaces[0];
    let captured = crate::intent::preset_capture::capture_workspace_preset(
        &crate::model::StructurePresentationSnapshot::default(),
        &engine,
        ws,
        None,
        &engine.surface_registry,
    )
    .expect("capture");
    let save = || Intent::SavePreset {
        base_name: "unused".to_string(),
        explicit_name: Some("bad/name".to_string()),
        overwrite: true,
        preset: ClonedPreset::Workspace(captured.clone()),
    };
    preset::handle(&core, &mut state, &mut engine, &save().from_agent_ipc());
    assert_eq!(
        state.toasts.len(),
        0,
        "에이전트의 프리셋 저장 실패는 토스트로 표시하지 않는다"
    );

    preset::handle(
        &core,
        &mut state,
        &mut engine,
        &save().from_user_menu("test"),
    );
    assert_eq!(
        state.toasts.len(),
        1,
        "사용자의 프리셋 저장 실패는 토스트로 표시한다"
    );
}

/// 최근 목록 기록은 변환 전에 하므로 get_live로 생성 가능한 kind인지 확인해야 한다.
/// 플러그인을 다시 등록한 경우도 함께 검사한다.
#[test]
fn a_convert_to_a_withdrawn_kind_leaves_no_recent_entry() {
    let (mut core, mut state, mut engine) = fixture();
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": "probe_recent",
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "webview",
        "records_recent": true,
    }))
    .expect("probe decl");
    let register = |engine: &crate::core::CoreState| {
        let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
        crate::plugin_bridge::remote_kind::register_remote_kind(
            &engine.surface_registry,
            "com.x.probe",
            &decl,
            host_cmd_tx,
        );
    };
    register(&engine);
    assert_eq!(
        engine.surface_registry.withdraw_plugin("com.x.probe"),
        vec!["probe_recent"]
    );
    let surface_id = *state
        .active_workspace(&engine)
        .all_surface_ids()
        .first()
        .expect("fixture surface");
    let convert = |file: &str| {
        Intent::ConvertSurface {
            surface_id,
            target: ConvertTarget::Kind {
                cwd: None,
                kind: "probe_recent".to_string(),
                params: serde_json::json!({ "file": file }),
            },
        }
        .from_user_menu("test")
    };

    crate::intent::surface::handle(
        &mut core,
        &mut state,
        &mut engine,
        &convert("/notes/withdrawn.md"),
    );
    assert_eq!(
        state.recent_files.get("probe_recent"),
        Vec::<String>::new(),
        "거절된 변환은 최근 목록에 남지 않는다"
    );

    register(&engine);
    crate::intent::surface::handle(
        &mut core,
        &mut state,
        &mut engine,
        &convert("/notes/live.md"),
    );
    assert_eq!(
        state.recent_files.get("probe_recent"),
        vec!["/notes/live.md".to_string()],
        "철회가 풀린 kind 로의 변환은 기록된다"
    );
}

/// mirror surface의 경로는 원격 파일이라 로컬 최근 목록에 남기면 로컬 경로로 다시 열린다.
#[test]
fn a_convert_on_a_mirror_surface_leaves_no_local_recent_entry() {
    let (mut core, mut state, mut engine) = fixture();
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": "probe_recent_mirror",
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "webview",
        "records_recent": true,
    }))
    .expect("probe decl");
    let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.surface_registry,
        "com.x.probe_mirror",
        &decl,
        host_cmd_tx,
    );
    let surface_id = *state
        .active_workspace(&engine)
        .all_surface_ids()
        .first()
        .expect("fixture surface");
    engine.workspaces[0].mirror = true;

    crate::intent::surface::handle(
        &mut core,
        &mut state,
        &mut engine,
        &Intent::ConvertSurface {
            surface_id,
            target: ConvertTarget::Kind {
                cwd: None,
                kind: "probe_recent_mirror".to_string(),
                params: serde_json::json!({ "file": "/remote/only.md" }),
            },
        }
        .from_user_menu("test"),
    );
    assert_eq!(
        engine.pending_structural_forward.len(),
        1,
        "변환 요청을 원격으로 전달한다"
    );
    assert_eq!(
        state.recent_files.get("probe_recent_mirror"),
        Vec::<String>::new(),
        "원격 경로는 로컬 최근 목록에 기록하지 않는다"
    );
}

fn register_recent_probe(engine: &crate::core::CoreState, kind: &str, plugin_id: &str) {
    let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(serde_json::json!({
        "kind": kind,
        "display_name_i18n_key": "surface.kind.markdown",
        "rendering": "webview",
        "records_recent": true,
    }))
    .expect("probe decl");
    let (host_cmd_tx, _host_cmd_rx) = std::sync::mpsc::channel();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &engine.surface_registry,
        plugin_id,
        &decl,
        host_cmd_tx,
    );
}

fn new_tab_with_file(
    core: &mut crate::core::Core,
    state: &mut crate::state::RequestContext,
    engine: &mut EngineMut<'_>,
    kind: &str,
    file: &str,
) {
    crate::intent::tab::handle(
        core,
        state,
        engine,
        &Intent::NewTab {
            kind: Some(kind.to_string()),
            params: serde_json::json!({ "file": file }),
        }
        .from_user_menu("test"),
    );
}

/// mirror pane에 새 탭을 열면 경로가 원격 파일이라 로컬 최근 목록에 기록하지 않는다.
#[test]
fn a_new_tab_in_a_mirror_pane_leaves_no_local_recent_entry() {
    let (mut core, mut state, mut engine) = fixture();
    register_recent_probe(&engine, "probe_recent_newtab", "com.x.probe_newtab");
    engine.workspaces[0].mirror = true;

    new_tab_with_file(
        &mut core,
        &mut state,
        &mut engine,
        "probe_recent_newtab",
        "/remote/only.md",
    );
    assert_eq!(
        engine.pending_structural_forward.len(),
        1,
        "새 탭 요청을 원격으로 전달한다"
    );
    assert_eq!(
        state.recent_files.get("probe_recent_newtab"),
        Vec::<String>::new(),
        "원격 경로는 로컬 최근 목록에 기록하지 않는다"
    );
}

/// 로컬 pane의 새 탭은 계속 최근 목록에 기록한다.
#[test]
fn a_new_tab_in_a_local_pane_records_the_recent_entry() {
    let (mut core, mut state, mut engine) = fixture();
    register_recent_probe(
        &engine,
        "probe_recent_newtab_local",
        "com.x.probe_newtab_local",
    );

    new_tab_with_file(
        &mut core,
        &mut state,
        &mut engine,
        "probe_recent_newtab_local",
        "/local/only.md",
    );
    assert!(
        engine.pending_structural_forward.is_empty(),
        "로컬 pane은 원격으로 전달하지 않는다"
    );
    assert_eq!(
        state.recent_files.get("probe_recent_newtab_local"),
        vec!["/local/only.md".to_string()],
        "로컬 경로는 최근 목록에 기록한다"
    );
}

/// Intent 큐를 거치지 않는 IPC 구조 요청에도 silent_failure가 붙어야 한다.
#[test]
fn an_ipc_direct_structural_forward_is_marked_for_a_silent_failure() {
    let (mut core, mut state, mut engine) = fixture();
    let ws = state.active_workspace(&engine);
    let surface_id = *ws.all_surface_ids().first().expect("fixture surface");
    let pane_id = engine.find_pane_for_surface(surface_id).expect("pane");
    let tab_id = engine
        .find_pane_by_id(pane_id)
        .and_then(|p| p.tabs.first())
        .map(|t| t.id)
        .expect("tab");
    engine.workspaces[0].mirror = true;

    let requests = [
        (
            "split",
            serde_json::json!({ "level": "surface", "target_surface": surface_id }),
        ),
        (
            "split",
            serde_json::json!({ "level": "pane", "target_pane": pane_id }),
        ),
        ("tab.create", serde_json::json!({ "pane_id": pane_id })),
        ("tab.close", serde_json::json!({ "tab_id": tab_id })),
        (
            "tab.move",
            serde_json::json!({ "pane_id": pane_id, "from_index": 0, "to_index": 0 }),
        ),
        ("pane.close", serde_json::json!({ "pane_id": pane_id })),
        (
            "surface.close",
            serde_json::json!({ "surface_id": surface_id }),
        ),
        (
            "image.open",
            serde_json::json!({ "surface_id": surface_id, "path": "/tmp/x.png" }),
        ),
    ];
    for (method, params) in requests {
        engine.pending_structural_forward.clear();
        let req = crate::ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params: params.clone(),
            id: Some(serde_json::json!(1)),
            session_token: None,
        };
        let resp = crate::ipc::handler::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &crate::ipc::caller::CallerContext::Local,
        );
        assert!(resp.error.is_none(), "{method} {params}: {:?}", resp.error);
        assert_eq!(
            resp.result.as_ref().and_then(|r| r.get("forwarded")),
            Some(&serde_json::json!(true)),
            "{method} {params}: 원격으로 전달한다"
        );
        assert_eq!(
            engine.pending_structural_forward.len(),
            1,
            "{method} {params}"
        );
        assert!(
            engine.pending_structural_forward[0].silent_failure,
            "{method} {params}: 에이전트 요청의 원격 실패는 로그로 간다"
        );
    }
    assert_eq!(state.toasts.len(), 0);
}
