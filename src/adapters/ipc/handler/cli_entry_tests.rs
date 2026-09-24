//! CLI가 만든 요청을 실제 핸들러에 전달해 파라미터 이름이 맞는지 확인한다.
//! 터미널 재시작은 없는 대상을 지정해 대상 조회까지 도달했는지만 확인한다.
//! `window.close`는 App 상태가 필요하므로 메서드 이름과 파라미터 형식만 검사한다.

use serde_json::json;
use tasty_cli::request::command_to_request;
use tasty_cli::{
    CloseCommands, Commands, HookHandlerCommands, SendCommands, SessionCommands, SurfaceCommands,
};

use crate::ipc::caller::CallerContext;

/// 누락된 파라미터나 잘못된 surface_id 오류가 없는지 확인한다.
/// 대상 없음 등 이후 단계의 오류는 허용한다.
fn assert_params_were_understood(method: &str, resp: &tasty_ipc::protocol::JsonRpcResponse) {
    if let Some(err) = &resp.error {
        let m = &err.message;
        assert!(
            !(m.contains("Missing") || m.contains("missing") || m.contains("Invalid 'surface_id'")),
            "{method}: CLI 가 보낸 params 를 핸들러가 못 읽었다 — 이름이 어긋났을 \
             가능성이 크다: {m}"
        );
    }
}

/// 다른 핸들러 시험에서도 재사용하는 테스트 어댑터 구성.
pub(crate) fn test_core() -> crate::core::Core {
    test_core_builder().build().expect("test Core")
}

/// 일부 어댑터만 바꿔야 하는 시험을 위해 build 전의 구성을 반환한다.
pub(crate) fn test_core_builder() -> crate::core::builder::CoreBuilder {
    use std::sync::{Arc, Mutex};
    crate::core::builder::CoreBuilder::new()
        .with_fs(Arc::new(crate::adapters::test::mem_fs::MemFileSystem::new()))
        .with_clock(Arc::new(
            crate::adapters::test::fake_clock::FakeClock::default(),
        ))
        .with_clipboard(Arc::new(
            crate::adapters::test::mock_clipboard::MockClipboard::default(),
        ))
        .with_process(Arc::new(
            crate::adapters::test::mock_process::MockProcessSpawner,
        ))
        .with_home(Arc::new(crate::adapters::test::tmp_home::TmpHome::new(
            tempfile::tempdir().expect("tmp").keep(),
        )))
        .with_sound_player(Arc::new(crate::ports::notification_sound::NoopPlayer))
        .with_memory(Arc::new(Mutex::new(
            tasty_memory::testing::InMemoryStorage::new(),
        )))
        .with_themes(Arc::new(tasty_themes::ThemeStore::new()))
        .with_preset_store(Arc::new(Mutex::new(
            tasty_presets::PresetStore::load_default(),
        )))
        .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
}

#[test]
fn surface_query_cli_entry_points_reach_their_handlers() {
    let (_state, engine) = crate::state::tests::test_state();
    let surface = engine.workspaces[0]
        .all_surface_ids()
        .first()
        .copied()
        .expect("워크스페이스에 surface 가 있어야 한다");

    let cases: Vec<(SurfaceCommands, &str)> = vec![
        (
            SurfaceCommands::CursorPosition { surface },
            "surface.cursor_position",
        ),
        (
            SurfaceCommands::ForegroundProcess { surface },
            "surface.foreground_process",
        ),
        (SurfaceCommands::Locate { surface }, "surface.locate"),
    ];

    for (command, expected_method) in cases {
        let req = command_to_request(&Commands::Surface { command });
        assert_eq!(req.method, expected_method);

        let resp = match expected_method {
            "surface.cursor_position" => {
                super::surface::handle_cursor_position(&engine, json!(1), &req.params)
            }
            "surface.foreground_process" => {
                super::surface::handle_foreground_process(&engine, json!(1), &req.params)
            }
            _ => super::surface::handle_surface_locate(&engine, json!(1), &req.params),
        };
        assert_params_were_understood(expected_method, &resp);
        assert!(
            resp.result.is_some(),
            "{expected_method}: 살아 있는 surface 에 대한 조회는 성공해야 한다: {:?}",
            resp.error
        );
    }
}

/// `tasty surface respawn-terminal` — 파괴적이라 **없는 surface** 로 부른다.
/// 파라미터를 못 읽으면 `Missing`, 읽었으면 대상 조회 실패다.
#[test]
fn respawn_terminal_cli_entry_point_reaches_target_lookup() {
    let mut core = test_core();
    let (_state, mut engine) = crate::state::tests::test_state();
    let missing = 999_999u32;

    let req = command_to_request(&Commands::Surface {
        command: SurfaceCommands::RespawnTerminal { surface: missing },
    });
    assert_eq!(req.method, "surface.respawn_terminal");

    let resp = super::surface::handle_surface_respawn_terminal(
        &mut core,
        &mut engine,
        json!(1),
        &req.params,
    );
    assert_params_were_understood("surface.respawn_terminal", &resp);
    assert!(
        resp.error.is_some(),
        "없는 surface 는 에러여야 한다(파라미터는 읽혔다)"
    );
}

/// `tasty surface fire-hook` — `surface_id` 와 `event` 두 키를 함께 확인한다.
/// 훅이 하나도 등록돼 있지 않아도 파라미터 단계는 지나야 한다.
#[test]
fn fire_hook_cli_entry_point_reaches_its_handler() {
    let mut core = test_core();
    let (mut state, mut engine) = crate::state::tests::test_state();
    let surface = engine.workspaces[0].all_surface_ids()[0];

    let req = command_to_request(&Commands::Surface {
        command: SurfaceCommands::FireHook {
            surface,
            event: "process-exit".to_string(),
        },
    });
    assert_eq!(req.method, "surface.fire_hook");
    assert_eq!(req.params["event"], json!("process-exit"));

    let resp = super::hooks::handle_surface_fire_hook(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &req.params,
    );
    assert_params_were_understood("surface.fire_hook", &resp);
}

/// `tasty send text --wait-idle` — 플래그가 메서드를 가르고, params 는 그대로다.
#[test]
fn send_text_wait_idle_cli_entry_point_switches_method_and_reaches_its_handler() {
    let (_state, mut engine) = crate::state::tests::test_state();
    let surface = engine.workspaces[0].all_surface_ids()[0];

    let plain = command_to_request(&Commands::Send {
        command: SendCommands::Text {
            text: "echo hi".to_string(),
            surface: Some(surface),
            wait_idle: false,
        },
    });
    assert_eq!(plain.method, "surface.send");

    let req = command_to_request(&Commands::Send {
        command: SendCommands::Text {
            text: "echo hi".to_string(),
            surface: Some(surface),
            wait_idle: true,
        },
    });
    assert_eq!(req.method, "surface.send_wait_idle");

    let resp = super::handle_send_wait_idle(&mut engine, json!(1), &req.params);
    assert_params_were_understood("surface.send_wait_idle", &resp);
    assert!(
        resp.result.is_some(),
        "유휴 상태의 살아 있는 surface 로 보내면 성공해야 한다: {:?}",
        resp.error
    );
}

#[test]
fn session_cli_entry_points_reach_their_handlers() {
    let core = test_core();
    let caller = CallerContext::Local;

    let issue = command_to_request(&Commands::Session {
        command: SessionCommands::Issue {
            agent_id: "probe-agent".to_string(),
            permissions: vec!["surface.read".to_string()],
            ttl_ms: Some(60_000),
        },
    });
    assert_eq!(issue.method, "session.issue");
    let resp = super::session::handle_issue(&core, &caller, json!(1), &issue.params);
    assert_params_were_understood("session.issue", &resp);
    assert!(
        resp.result.is_some(),
        "토큰 발급은 성공해야 한다: {:?}",
        resp.error
    );

    let revoke = command_to_request(&Commands::Session {
        command: SessionCommands::Revoke {
            token: "not-a-real-token".to_string(),
        },
    });
    assert_eq!(revoke.method, "session.revoke");
    let resp = super::session::handle_revoke(&core, json!(1), &revoke.params);
    assert_params_were_understood("session.revoke", &resp);

    let list = command_to_request(&Commands::Session {
        command: SessionCommands::List,
    });
    assert_eq!(list.method, "session.list");
    let resp = super::session::handle_list(&core, json!(1));
    assert!(resp.result.is_some(), "목록 조회는 성공해야 한다");
}

/// App 상태가 필요한 핸들러는 호출하지 않고 요청의 메서드와 `id` 형식을 확인한다.
#[test]
fn close_window_cli_entry_point_matches_what_the_app_handler_reads() {
    let req = command_to_request(&Commands::Close {
        command: CloseCommands::Window { id: 3 },
    });
    assert_eq!(req.method, "window.close");
    assert_eq!(
        req.params.get("id").and_then(|v| v.as_u64()),
        Some(3),
        "App 핸들러는 `id` 를 u64 로 읽는다"
    );
}

/// 실제 사용자 설정을 바꾸지 않도록 레지스트리를 수정하지 않는 요청만 보낸다.
/// 각 파라미터를 읽었을 때 나오는 오류로 키 이름이 맞는지 확인한다.
#[test]
fn hook_handler_edit_cli_entry_points_reach_their_handlers() {
    let absent = "user/definitely-not-registered-in-this-test";

    let req = command_to_request(&Commands::HookHandler {
        command: HookHandlerCommands::Get {
            id: absent.to_string(),
        },
    });
    assert_eq!(req.method, "hook_handler.get");
    let resp = super::hook_handler::handle_get(json!(1), &req.params);
    let msg = resp.error.expect("없는 id 는 거절된다").message;
    assert!(msg.contains("not found"), "id 키가 안 읽혔다: {msg}");

    let req = command_to_request(&Commands::HookHandler {
        command: HookHandlerCommands::Upsert {
            id: "user/x".into(),
            source: Some("bogus".into()),
            priority: None,
            display_name_key: None,
            disabled: None,
            action: None,
            calls: None,
        },
    });
    assert_eq!(req.method, "hook_handler.upsert");
    let resp = super::hook_handler::handle_upsert(json!(1), &req.params);
    let msg = resp.error.expect("잘못된 source 는 거절된다").message;
    // 다른 오류에도 source가 나오므로, 이 오류에만 있는 선택지 목록을 확인한다.
    assert!(
        msg.contains("hook|webhook|any"),
        "source 키가 안 읽혔다: {msg}"
    );

    // --calls 는 action 자리로 들어간다. 스키마에 안 맞는 것을 보내 그 자리를 고정한다.
    let req = command_to_request(&Commands::HookHandler {
        command: HookHandlerCommands::Upsert {
            id: "user/x".into(),
            source: None,
            priority: None,
            display_name_key: None,
            disabled: None,
            action: None,
            calls: Some(r#"[{"no_method_key":1}]"#.into()),
        },
    });
    let resp = super::hook_handler::handle_upsert(json!(1), &req.params);
    let msg = resp
        .error
        .expect("스키마에 안 맞는 calls 는 거절된다")
        .message;
    // 위와 같은 이유로 `'action'` 이 아니라 이 갈래에만 있는 스키마 힌트로 잰다.
    assert!(msg.contains("ipc_sequence"), "action 키가 안 읽혔다: {msg}");

    let req = command_to_request(&Commands::HookHandler {
        command: HookHandlerCommands::Remove {
            id: absent.to_string(),
        },
    });
    assert_eq!(req.method, "hook_handler.remove");
    let resp = super::hook_handler::handle_remove(json!(1), &req.params);
    let result = resp.result.expect("없는 id 제거는 오류가 아니다");
    assert_eq!(result["id"], absent, "id 키가 안 읽혔다");
    assert_eq!(result["existed"], false);
}
