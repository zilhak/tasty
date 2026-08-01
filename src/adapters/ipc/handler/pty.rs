//! `pty.*` IPC 핸들러 — headless PTY primitive 의 IPC/CLI 표면
//! (`docs/adr/0050-headless-pty-primitive.md`, `docs/features/headless-pty/index.md`).
//!
//! 에이전트가 **Surface(Tab) 없이** 백그라운드에서 굴리는 1 회성 PTY 를 spawn/write/
//! read/wait/kill/list 한다. 상위 `child_terminal`(`terminal.*`) 은 자식 터미널
//! *surface* 를 만들어 GUI 에 노출하지만, 여기 PTY 는 Surface 트리를 전혀 건드리지
//! 않는다 — 렌더되지 않고, 포커스/닫은-항목 히스토리/선택에 닿지 않는다(identity.md
//! 원칙 1). 좀비 누적은 [`PtyRegistry`](crate::core::pty_registry) 의 동시 개수 상한
//! + idle TTL 로 막는다.
//!
//! **두 store 정합**: 메타데이터·exit-code cell 은 `engine.pty_registry`, 실제 headless
//! `Terminal` 은 `engine.terminals`(`TerminalStore`)에 **같은 pty id** 로 보관한다(pty id
//! 는 [`PTY_ID_BASE`](crate::core::pty_registry::PTY_ID_BASE) 이상 disjoint 범위에서
//! 발급되어 surface id 와 충돌하지 않는다). 어느 한 쪽만 지우면 누수/좀비가
//! 되므로 kill/sweep 은 **항상 두 store 를 함께** 정리한다.

use std::time::Instant;

use serde_json::{Value, json};

use crate::core::CoreState;
use crate::core::pty_registry::{PtySpawnError, PtySpawnSpec};
use crate::ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

// ───── 파라미터 헬퍼 ─────

fn require_u32(params: &Value, key: &str, id: &Value) -> Result<u32, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), format!("missing '{key}'")))
}

fn require_str(params: &Value, key: &str, id: &Value) -> Result<String, JsonRpcResponse> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| JsonRpcResponse::invalid_params(id.clone(), format!("missing '{key}'")))
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// `command` 파라미터(문자열 배열)를 토큰 벡터로. 미지정/빈 배열이면 빈 vec — bare shell.
fn parse_command(params: &Value) -> Vec<String> {
    params
        .get("command")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// idle TTL 이 지난 headless PTY 를 두 store 에서 함께 회수한다. `pty.spawn`/`pty.list`
/// 접근 시점에 lazy 로 호출 — `reconcile_with_live_surfaces`(child_terminal) 와 동형의
/// "접근 시 self-heal" 패턴. `sweep_idle` 이 registry 에서 제거한 id 들을
/// `TerminalStore` 에서도 제거해 두 store 를 정합시킨다(Terminal drop → PTY SIGHUP).
fn lazy_sweep(engine: &mut CoreState) {
    let expired = engine.pty_registry.sweep_idle(Instant::now());
    for pty_id in expired {
        engine.terminals.remove(pty_id);
        // waker dedup 게이트 제거(pty_id 키) — 미제거 시 sweep 마다 게이트 영구 누적(누수).
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(pty_id);
        }
        tracing::debug!("headless pty {pty_id} swept (idle TTL exceeded)");
    }
}

// ───── 핸들러 ─────

/// `pty.spawn` — headless PTY 를 띄우고 pty id 를 반환한다. 상한 초과 시
/// [`PtySpawnError::LimitReached`] 를 IPC 에러로 변환한다(panic 하지 않음).
pub(crate) fn handle_spawn(
    core: &mut crate::core::Core,
    engine: &mut CoreState,
    caller: &CallerContext,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    lazy_sweep(engine);

    let command = parse_command(params);
    let cwd = optional_str(params, "cwd");
    let owner_agent_id = caller.agent_id().as_str().to_string();

    // 1) registry 등록(상한 게이트). 실패하면 아무 자원도 만들지 않고 즉시 반환.
    // Clock port 경유 — outbound port 실제 소비 경로.
    let now = core.now_instant();
    let pty_id = match engine.pty_registry.register(
        PtySpawnSpec {
            owner_agent_id: owner_agent_id.clone(),
            cwd: cwd.clone(),
            command: command.clone(),
        },
        now,
    ) {
        Ok(pid) => pid,
        Err(e @ PtySpawnError::LimitReached { .. }) => {
            return JsonRpcResponse::error(id, -32000, e.to_string());
        }
    };

    // 2) 실제 headless Terminal 생성(Surface/트리 삽입 없이 PTY 셸만). Surface 터미널
    //    spawn 경로(mod.rs apply_split_pane)와 동일한 ShellConfig/waker 배선을 쓰되,
    //    command 가 주어지면 initial_input 으로 즉시 실행한다(terminal.spawn 이 tab
    //    생성 후 command 를 send 하는 것과 동형 — 여기서는 첫 stdin 바이트로 주입).
    let cols = engine.default_cols;
    let rows = engine.default_rows;
    let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
    let waker = engine.make_waker(pty_id);
    let working_dir = cwd.as_deref().map(std::path::Path::new);
    let initial_input = if command.is_empty() {
        None
    } else {
        Some(format!("{}\n", command.join(" ")))
    };
    let terminal = match tasty_terminal::Terminal::new(
        tasty_terminal::TerminalConfig {
            cols,
            rows,
            shell: sh.shell_ref(),
            args: &sh.args_ref(),
            extra_env: &sh.envs_ref(),
            surface_id: pty_id,
            working_dir,
            initial_input: initial_input.as_deref(),
        },
        waker,
    ) {
        Ok(t) => t,
        Err(e) => {
            // 롤백: registry 항목을 되돌려 상한 슬롯을 회복한다.
            engine.pty_registry.remove(pty_id);
            return JsonRpcResponse::internal_error(id, format!("pty spawn failed: {e}"));
        }
    };
    engine.terminals.insert(pty_id, terminal);

    // 3) exit-code watcher 배선: waitable child 를 Terminal 에서 넘겨받아
    //    `child.wait()` 로 진짜 종료코드를 잡는다(18-a `attach_exit_watcher`).
    //    take_child 이후 Terminal 의 자체 exit 감지·Drop kill 은 이 child 에 적용되지
    //    않으므로, kill 은 Terminal drop 의 PTY master close(SIGHUP)로 처리한다.
    if let Some(mut child) = engine
        .terminals
        .get_mut(pty_id)
        .and_then(|t| t.take_child())
    {
        engine
            .pty_registry
            .attach_exit_watcher(pty_id, move || match child.wait() {
                Ok(status) => crate::core::pty_registry::PtyExit::from_status(
                    Some(status.exit_code() as i32),
                    status.success(),
                ),
                Err(e) => {
                    tracing::warn!("headless pty {pty_id} child.wait failed: {e}");
                    crate::core::pty_registry::PtyExit::from_status(None, false)
                }
            });
    }

    JsonRpcResponse::success(
        id,
        json!({
            "pty_id": pty_id,
            "owner_agent_id": owner_agent_id,
            "command": command,
            "cwd": cwd,
        }),
    )
}

/// `pty.write` — 실행 중 PTY 에 입력(stdin)을 그대로 보낸다(as-is, 자동 제출 없음 —
/// 호출자가 개행/`\r` 포함). idle 타이머 리셋.
pub(crate) fn handle_write(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let text = match require_str(params, "text", &id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if !engine.pty_registry.contains(pty_id) {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    let Some(terminal) = engine.terminals.get_mut(pty_id) else {
        return JsonRpcResponse::internal_error(
            id,
            format!("headless pty {pty_id} registry/store desync (terminal missing)"),
        );
    };
    terminal.send_bytes(text.as_bytes());
    engine.pty_registry.touch(pty_id, Instant::now());
    JsonRpcResponse::success(id, json!({ "id": pty_id, "written": text.len() }))
}

/// `pty.read` — PTY 의 현재 화면 텍스트를 읽는다(optional `lines`=하단 N 줄).
/// `surface.screen_text` 와 동일 추출 경로. idle 타이머 리셋.
/// `show_dim`(기본 false): dim(ghost-suggestion, 예: Claude Code 자동완성 제안) 셀을
/// 결과에 포함할지 — 기본은 제외해 실제 입력된 텍스트만 반환한다.
pub(crate) fn handle_read(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !engine.pty_registry.contains(pty_id) {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    let lines = params
        .get("lines")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let show_dim = params
        .get("show_dim")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let text = engine
        .find_terminal_by_id(pty_id)
        .map(|t| match lines {
            Some(n) => t.screen_text_lines(n, show_dim),
            None => t.screen_text(show_dim),
        })
        .unwrap_or_default();
    engine.pty_registry.touch(pty_id, Instant::now());
    JsonRpcResponse::success(id, json!({ "id": pty_id, "text": text }))
}

/// `pty.wait` — 폴링(즉시 반환, blocking 아님). exit-watcher 가 채운 exit cell 을
/// 조회해 종료 여부·exit code 를 반환한다. Surface 라이브 트리가 아니라
/// `PtyEntry::exit()` 로 판정한다(headless 라 Surface 자체가 없음). 활발히 폴링 중인
/// PTY 가 idle-sweep 에 회수되지 않도록 idle 타이머를 리셋한다.
pub(crate) fn handle_wait(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !engine.pty_registry.contains(pty_id) {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    engine.pty_registry.touch(pty_id, Instant::now());
    let entry = engine
        .pty_registry
        .get(pty_id)
        .expect("contains() checked above");
    match entry.exit() {
        Some(exit) => JsonRpcResponse::success(
            id,
            json!({
                "id": pty_id,
                "exited": true,
                "exit_code": exit.code,
                "success": exit.success,
            }),
        ),
        None => JsonRpcResponse::success(id, json!({ "id": pty_id, "exited": false })),
    }
}

/// `pty.kill` — 프로세스를 종료하고 두 store 에서 회수한다. Surface 를 닫는 게 아니라
/// (headless 라 Surface 가 없다) PTY 프로세스만 종료한다: Terminal 을 store 에서
/// 제거하면 drop 시 PTY master 가 닫히며 자식에 SIGHUP 이 가고, exit-watcher 스레드가
/// `child.wait()` 로 reap 한다.
pub(crate) fn handle_kill(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let had_entry = engine.pty_registry.remove(pty_id).is_some();
    // Terminal drop → PTY master close → 자식 SIGHUP. watcher 가 reap.
    let had_terminal = engine.terminals.remove(pty_id).is_some();
    if !had_entry && !had_terminal {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    // waker dedup 게이트 제거(pty_id 키) — 미제거 시 kill 마다 게이트 영구 누적(누수).
    if let Some(factory) = engine.waker_factory.as_ref() {
        factory.forget_surface(pty_id);
    }
    JsonRpcResponse::success(id, json!({ "id": pty_id, "killed": true }))
}

/// `pty.attach_surface` — headless PTY 를 실제 Surface(Tab) 로 **승격(adopt)** 한다.
/// `pane_id`(어느 Pane 에 붙일지) + `id`(pty id) 를 받아 `AdoptTerminal` intent 로
/// 실행한다. spawn/write/read/wait/kill/list 와 달리 이건 진짜 Surface 를 새로
/// 만들므로(권한 `[SurfaceWrite, TerminalSpawn]`) `tab.create` 와 동일한
/// `cascade_tab_created`(tab.created/surface.created host event)를 발화해야 GUI 가
/// 그 Tab 을 렌더한다. 승격 후 그 pty id 는 registry 에서 빠져 `pty.list` 에 더 이상
/// 나타나지 않는다(같은 Terminal 인스턴스가 surface_id 키로 옮겨짐 — 상태 보존).
pub(crate) fn handle_attach_surface(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut CoreState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let pane_id = match require_u32(params, "pane_id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    // 검증: 대상 headless PTY 가 살아있고 pane 이 존재해야 한다(깔끔한 에러 메시지).
    match engine.pty_registry.get(pty_id) {
        None => {
            return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
        }
        Some(entry) if entry.has_exited() => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("headless pty {pty_id} already exited"),
            );
        }
        Some(_) => {}
    }
    if engine.find_pane_by_id(pane_id).is_none() {
        return JsonRpcResponse::invalid_params(id, format!("pane {pane_id} not found"));
    }

    let intent = crate::core::intent::DomainIntent::AdoptTerminal { pane_id, pty_id };
    let events = match core.apply(engine, intent) {
        Ok(events) => events,
        Err(e) => return super::structural_apply_error(id, &e),
    };

    let Some(crate::core::intent::CoreEvent::TabCreated {
        pane_id,
        tab_id,
        surface_id,
        ..
    }) = events.into_iter().next()
    else {
        return JsonRpcResponse::internal_error(id, "Core::apply returned no TabCreated event");
    };

    // tab.create 와 동형 cascade — tab.created/surface.created host event enqueue +
    // polling baseline 동기화. 이 호출을 빠뜨리면 데이터만 옮겨지고 화면엔 안 뜬다.
    crate::app::dispatch_domain::cascade_tab_created(state, engine, pane_id, tab_id, surface_id);

    JsonRpcResponse::success(
        id,
        json!({
            "pane_id": pane_id,
            "tab_id": tab_id,
            "surface_id": surface_id,
        }),
    )
}

/// `pty.list` — 살아있는 headless PTY 전체 목록. **포커스 독립성**: 필터 없이 전 목록을
/// 무조건 반환한다. 접근 시점에 idle TTL 을 lazy sweep 한다.
pub(crate) fn handle_list(engine: &mut CoreState, id: Value) -> JsonRpcResponse {
    lazy_sweep(engine);
    let ptys: Vec<Value> = engine
        .pty_registry
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "owner_agent_id": e.owner_agent_id,
                "cwd": e.cwd,
                "command": e.command,
                "has_exited": e.has_exited(),
                "exit_code": e.exit().and_then(|x| x.code),
            })
        })
        .collect();
    JsonRpcResponse::success(id, json!({ "ptys": ptys }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// `handle_spawn` 이 소비하는 `Core::now_instant`(Clock port) 용 최소
    /// fixture. `TempDir` 은 호출자가 명명된 binding 으로 받아 즉시 drop 되지 않게 한다.
    fn core() -> (crate::core::Core, tempfile::TempDir) {
        use std::sync::{Arc, Mutex};

        use tasty_memory::MemoryStorage;
        use tasty_themes::{ThemeStorage, ThemeStore};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::core::builder::CoreBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let home = TmpHome::new(home_tmp.path().to_path_buf());

        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner::default()))
            .with_home(Arc::new(home))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core build");

        (core, home_tmp)
    }

    fn ok(resp: JsonRpcResponse) -> Value {
        resp.result.expect("expected success result")
    }

    /// exit cell 이 채워질 때까지 bounded poll(최대 ~8s). 종료 정보를 반환.
    fn wait_for_exit(engine: &mut CoreState, pty_id: u32) -> Value {
        for _ in 0..800 {
            let resp = handle_wait(engine, json!(1), &json!({ "id": pty_id }));
            let v = ok(resp);
            if v["exited"] == Value::Bool(true) {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("headless pty {pty_id} did not exit within timeout");
    }

    #[test]
    fn spawn_write_wait_kill_list_e2e() {
        let mut e = engine();
        let (mut c, _home) = core();
        let caller = CallerContext::Local;

        // spawn: bare shell (command 없음). pty id 는 disjoint 고범위.
        let resp = handle_spawn(&mut c, &mut e, &caller, json!(1), &json!({}));
        let spawned = ok(resp);
        let pty_id = spawned["pty_id"].as_u64().unwrap() as u32;
        assert!(pty_id >= crate::core::pty_registry::PTY_ID_BASE);

        // list: 방금 만든 PTY 가 보인다(필터 없이 전체 반환).
        let listed = ok(handle_list(&mut e, json!(2)));
        let arr = listed["ptys"].as_array().unwrap();
        assert!(arr.iter().any(|p| p["id"].as_u64() == Some(pty_id as u64)));

        // write: 셸에 `exit 3` 을 보내 종료를 유발(명령 실행 경로 검증).
        let w = ok(handle_write(
            &mut e,
            json!(3),
            &json!({ "id": pty_id, "text": "exit 3\n" }),
        ));
        assert_eq!(w["id"].as_u64(), Some(pty_id as u64));

        // wait: 실제 exit code 3 을 exit-watcher 가 잡아야 한다.
        let exited = wait_for_exit(&mut e, pty_id);
        assert_eq!(exited["exit_code"].as_i64(), Some(3));
        assert_eq!(exited["success"], Value::Bool(false));

        // kill: 두 store 에서 회수 → list 에 안 보인다.
        let killed = ok(handle_kill(&mut e, json!(4), &json!({ "id": pty_id })));
        assert_eq!(killed["killed"], Value::Bool(true));
        assert!(!e.pty_registry.contains(pty_id));
        assert!(e.find_terminal_by_id(pty_id).is_none());
        let listed2 = ok(handle_list(&mut e, json!(5)));
        assert!(
            listed2["ptys"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p["id"].as_u64() != Some(pty_id as u64))
        );
    }

    #[test]
    fn spawn_with_command_captures_exit_code() {
        // command 를 initial_input 으로 즉시 실행하는 경로 + exit-code 캡처 검증.
        let mut e = engine();
        let (mut c, _home) = core();
        let caller = CallerContext::Local;
        let resp = handle_spawn(
            &mut c,
            &mut e,
            &caller,
            json!(1),
            &json!({ "command": ["exit", "7"] }),
        );
        let pty_id = ok(resp)["pty_id"].as_u64().unwrap() as u32;
        let exited = wait_for_exit(&mut e, pty_id);
        assert_eq!(exited["exit_code"].as_i64(), Some(7));
        // 정리: 살아있는 PTY 종료(응답은 확인 불필요).
        handle_kill(&mut e, json!(9), &json!({ "id": pty_id }));
    }

    #[test]
    fn spawn_beyond_limit_returns_error() {
        let mut e = engine();
        let (mut c, _home) = core();
        // 상한을 2 로 낮춰 3 번째 spawn 이 LimitReached 로 실패하는지 확인.
        e.pty_registry = crate::core::pty_registry::PtyRegistry::with_limits(
            2,
            crate::core::pty_registry::DEFAULT_IDLE_TTL,
        );
        let caller = CallerContext::Local;
        let a = handle_spawn(&mut c, &mut e, &caller, json!(1), &json!({}));
        let b = handle_spawn(&mut c, &mut e, &caller, json!(2), &json!({}));
        assert!(a.result.is_some());
        assert!(b.result.is_some());
        let resp = handle_spawn(&mut c, &mut e, &caller, json!(3), &json!({}));
        assert!(
            resp.error.is_some(),
            "3rd spawn must fail with LimitReached"
        );
        let err = resp.error.unwrap();
        assert!(
            err.message.contains("limit"),
            "error should mention limit: {}",
            err.message
        );
        // 정리: 살아있는 두 PTY 종료(응답은 확인 불필요).
        for pid in e.pty_registry.ids() {
            handle_kill(&mut e, json!(0), &json!({ "id": pid }));
        }
    }

    /// 회귀(waker dedup 게이트 누수): `pty.kill` 은 회수하는 pty_id 의 waker 게이트를
    /// 반드시 `forget_surface` 로 정리해야 한다(대상 pty 만 — 다른 pty 게이트는 보존).
    #[test]
    fn kill_forgets_only_target_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        let mut e = engine();
        let (mut c, _home) = core();
        let factory = RecordingWakerFactory::new();
        let shared: crate::waker::SharedWakerFactory = factory.clone();
        e.waker_factory = Some(shared);
        let caller = CallerContext::Local;

        // spawn 2 개 — 각 pty_id 로 targeted 게이트가 생성된다(make_waker → factory).
        let a = ok(handle_spawn(&mut c, &mut e, &caller, json!(1), &json!({})))["pty_id"]
            .as_u64()
            .unwrap() as u32;
        let b = ok(handle_spawn(&mut c, &mut e, &caller, json!(2), &json!({})))["pty_id"]
            .as_u64()
            .unwrap() as u32;
        assert!(
            factory.made().contains(&a) && factory.made().contains(&b),
            "spawn 은 pty_id 별 targeted 게이트를 만든다"
        );

        // kill a → forget_surface(a) 호출, b 게이트는 건드리지 않는다.
        ok(handle_kill(&mut e, json!(3), &json!({ "id": a })));
        assert!(
            factory.forgotten().contains(&a),
            "handle_kill 은 회수하는 pty_id 의 waker 게이트를 정리해야 한다"
        );
        assert!(
            !factory.forgotten().contains(&b),
            "kill 은 대상 pty 의 게이트만 정리(다른 pty 보존)"
        );

        // 정리: 남은 b 종료.
        handle_kill(&mut e, json!(4), &json!({ "id": b }));
    }

    /// 회귀(waker dedup 게이트 누수): idle TTL sweep(`lazy_sweep`)도 회수하는 pty_id 의
    /// waker 게이트를 `forget_surface` 로 정리해야 한다. TTL 0 으로 즉시 만료시킨다.
    #[test]
    fn idle_sweep_forgets_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        let mut e = engine();
        let (mut c, _home) = core();
        // TTL 0: 다음 접근(lazy_sweep) 시 touch 안 된 headless PTY 는 즉시 만료 회수.
        e.pty_registry =
            crate::core::pty_registry::PtyRegistry::with_limits(8, std::time::Duration::ZERO);
        let factory = RecordingWakerFactory::new();
        let shared: crate::waker::SharedWakerFactory = factory.clone();
        e.waker_factory = Some(shared);
        let caller = CallerContext::Local;

        let a = ok(handle_spawn(&mut c, &mut e, &caller, json!(1), &json!({})))["pty_id"]
            .as_u64()
            .unwrap() as u32;
        assert!(factory.made().contains(&a), "spawn 이 게이트를 만든다");

        // handle_list → lazy_sweep → a 가 idle 만료로 회수되며 게이트도 정리된다.
        ok(handle_list(&mut e, json!(2)));
        assert!(
            !e.pty_registry.contains(a),
            "TTL 0 이므로 sweep 이 a 를 회수해야 한다"
        );
        assert!(
            factory.forgotten().contains(&a),
            "lazy_sweep 은 회수하는 pty_id 의 waker 게이트를 정리해야 한다"
        );
    }

    #[test]
    fn write_read_wait_on_unknown_id_errors() {
        let mut e = engine();
        let bogus = crate::core::pty_registry::PTY_ID_BASE + 999;
        assert!(
            handle_write(&mut e, json!(1), &json!({ "id": bogus, "text": "x" }))
                .error
                .is_some()
        );
        assert!(
            handle_read(&mut e, json!(1), &json!({ "id": bogus }))
                .error
                .is_some()
        );
        assert!(
            handle_wait(&mut e, json!(1), &json!({ "id": bogus }))
                .error
                .is_some()
        );
        assert!(
            handle_kill(&mut e, json!(1), &json!({ "id": bogus }))
                .error
                .is_some()
        );
    }
}
