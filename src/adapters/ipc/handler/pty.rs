//! surface 없이 백그라운드 PTY를 관리한다. 화면·포커스·복원 기록은 만들지 않는다.
//! 등록 수와 idle TTL을 제한하며, 메타데이터와 Terminal은 같은 PTY ID로 서로 다른 저장소에 둔다.
//! PTY_ID_BASE 이상의 ID를 사용해 surface ID와 구분하고 회수할 때 두 저장소를 함께 정리한다.

use super::params::{self, p_try};
use std::time::Instant;

use serde_json::{Value, json};

use super::surface::query::{ScreenDiag, with_screen_diagnostics};

use crate::core::CoreState;
use crate::core::pty_registry::{PtySpawnError, PtySpawnSpec};
use crate::ipc::caller::CallerContext;
use tasty_ipc::protocol::JsonRpcResponse;

use super::params::require_u32;

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

/// 주기 정리와 별도로 spawn/list 직전에도 만료 항목을 회수한다.
/// spawn 상한을 검사하기 전에 빈 슬롯을 확보해야 하므로 주기 타이머만으로 대체하지 않는다.
fn lazy_sweep(engine: &mut CoreState) {
    // 공용 함수가 회수까지 마쳤으므로 반환된 ID 목록은 사용하지 않는다.
    let _ = engine.sweep_idle_ptys(Instant::now());
}

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

    // surface를 만들지 않고 셸만 시작한다. command는 최초 stdin 입력으로 보낸다.
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
            engine.pty_registry.remove(pty_id);
            return JsonRpcResponse::internal_error(id, format!("pty spawn failed: {e}"));
        }
    };
    engine.terminals.insert(pty_id, terminal);

    // child를 watcher로 넘겨 종료 코드를 수집한다. 이후 Terminal 자체는 child를 소유하지 않는다.
    // kill은 Terminal을 제거해 PTY master를 닫고, watcher가 종료 결과를 기다린다.
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

/// 현재 화면을 읽고 idle 시간을 갱신한다. lines는 하단 빈 줄을 빼고 마지막 N줄을 고르며
/// 부족하면 스크롤백에서 채운다. show_dim은 기본 false로 dim 셀을 제외한다.
pub(crate) fn handle_read(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !engine.pty_registry.contains(pty_id) {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    let lines = p_try!(params::opt_int::<usize>(params, "lines", &id));
    let show_dim = params
        .get("show_dim")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let found = engine.find_terminal_by_id(pty_id);
    let (text, diag) = match found {
        Some(t) => (
            match lines {
                Some(n) => t.screen_text_lines(n, show_dim),
                None => t.screen_text(show_dim),
            },
            Some(ScreenDiag {
                scrollback_len: t.scrollback_len(),
                alt_screen: t.is_alternate_screen(),
            }),
        ),
        None => (String::new(), None),
    };
    engine.pty_registry.touch(pty_id, Instant::now());
    JsonRpcResponse::success(
        id,
        with_screen_diagnostics(json!({ "id": pty_id, "text": text }), diag),
    )
}

/// 종료 결과의 현재 상태를 즉시 반환한다. 기다리지는 않으며 idle 시간을 갱신한다.
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

/// registry와 Terminal을 제거해 PTY master를 닫는다. watcher는 자식 종료를 기다려 회수한다.
/// 이 응답이 자식의 종료 완료를 확인한 것은 아니다.
pub(crate) fn handle_kill(engine: &mut CoreState, id: Value, params: &Value) -> JsonRpcResponse {
    let pty_id = match require_u32(params, "id", &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let had_entry = engine.pty_registry.remove(pty_id).is_some();
    let had_terminal = engine.terminals.remove(pty_id).is_some();
    if !had_entry && !had_terminal {
        return JsonRpcResponse::invalid_params(id, format!("headless pty {pty_id} not found"));
    }
    // PTY별 waker 기록도 지워 반복 생성·삭제 시 누적되지 않게 한다.
    if let Some(factory) = engine.waker_factory.as_ref() {
        factory.forget_surface(pty_id);
    }
    JsonRpcResponse::success(id, json!({ "id": pty_id, "killed": true }))
}

/// 기존 PTY를 surface로 옮겨 지정 pane의 새 탭에 붙인다. Terminal 상태는 유지하며
/// PTY registry에서는 제거한다. SurfaceWrite/TerminalSpawn 권한이 필요하다.
/// tab.create와 같은 후속 처리로 생성 이벤트와 화면 갱신도 수행한다.
pub(crate) fn handle_attach_surface(
    core: &mut crate::core::Core,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
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

    // 생성 이벤트와 polling 기준 상태를 함께 갱신한다.
    crate::core::structural_cascade::cascade_tab_created(
        window, engine, pane_id, tab_id, surface_id,
    );

    JsonRpcResponse::success(
        id,
        json!({
            "pane_id": pane_id,
            "tab_id": tab_id,
            "surface_id": surface_id,
        }),
    )
}

/// idle 항목을 회수한 뒤 등록된 PTY 전체를 반환한다.
/// watch_phase는 시험 실패 진단에만 사용하므로 공개 응답에 포함하지 않는다.
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
    use std::time::Duration;

    use super::*;
    use crate::core::pty_registry::WatchPhase;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    // Clock이 포함된 Core를 준비하고 TempDir을 호출자에게 넘겨 시험 동안 유지한다.
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
            .with_process(Arc::new(MockProcessSpawner))
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

    /// 종료 신호가 오지 않는 시험을 끝낼 대기 상한. 실패를 숨기려고 늘리지 않는다.
    /// 실패 진단은 종료하지 않는 명령을 보내 별도로 확인한다.
    const EXIT_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(30);

    /// 대상 부재, 기한 소진, 예상보다 일찍 끝난 대기를 구분해 진단한다.
    fn exit_wait_failure(
        pty_id: u32,
        elapsed: std::time::Duration,
        budget: std::time::Duration,
        still_registered: bool,
        observed: &str,
    ) -> String {
        if !still_registered {
            return format!(
                "headless pty {pty_id} 가 레지스트리에 없다 — 기다린 것이 아니라 \
                 대상이 없었다({elapsed:?} 만에 반환). 상한({budget:?})을 늘려도 \
                 해결되지 않는 대상 부재 오류다"
            );
        }
        if elapsed >= budget {
            return format!(
                "headless pty {pty_id}의 종료 결과를 제한 시간 {budget:?} 안에 받지 못했다\
                 (경과 {elapsed:?}). {observed}"
            );
        }
        format!(
            "headless pty {pty_id} 대기가 제한 시간 {budget:?} 전에 끝났다\
             (경과 {elapsed:?}). 대상이 남아 있고 결과도 없는 상태에서는 \
             일어나면 안 된다. 대기 경로를 확인한다"
        )
    }

    /// Condvar로 종료 결과를 기다린다. 보낸 입력은 실패 시 에코와 다른 출력을 구분하는 데 쓴다.
    fn wait_for_exit(engine: &mut CoreState, pty_id: u32, sent: &str) -> Value {
        let started = std::time::Instant::now();
        match engine.pty_registry.wait_for_exit(pty_id, EXIT_WAIT_BUDGET) {
            Some(exit) => json!({
                "id": pty_id,
                "exited": true,
                "exit_code": exit.code,
                "success": exit.success,
            }),
            None => {
                let elapsed = started.elapsed();
                let still_registered = engine.pty_registry.contains(pty_id);
                let observed = observed_pty_state(engine, pty_id, sent);
                panic!(
                    "{}",
                    exit_wait_failure(
                        pty_id,
                        elapsed,
                        EXIT_WAIT_BUDGET,
                        still_registered,
                        &observed
                    )
                )
            }
        }
    }

    /// watcher의 마지막 단계로 결과 수집 상태를 설명한다.
    /// 단계 기록과 자식 종료 시각은 같지 않으므로 자식의 생사를 단정하지 않는다.
    fn watch_phase_note(phase: WatchPhase) -> &'static str {
        match phase {
            WatchPhase::NotAttached => "종료 결과를 수집할 watcher가 등록되지 않았다",
            WatchPhase::Spawned => {
                "watcher 스레드 생성은 시작했지만 wait 진입은 아직 확인되지 않았다"
            }
            WatchPhase::Waiting => {
                "watcher는 종료 대기 단계이며 결과 기록 완료는 아직 확인되지 않았다"
            }
            WatchPhase::Reaped => {
                "종료 결과가 기록됐다. 대기를 마친 뒤 확인한 상태이므로 \
                 결과 기록 시각과 스케줄 지연을 함께 확인한다(ADR-0046)"
            }
        }
    }

    /// 보낸 입력의 에코 외에 텍스트가 있는지 확인한다.
    /// PTY 라인 디시플린도 에코를 만들므로 에코만으로 셸 실행을 확인할 수 없다.
    fn screen_holds_more_than_our_echo(visible: &str, sent: &str) -> bool {
        let visible = visible.trim();
        if visible.is_empty() {
            return false;
        }
        let echo = sent.trim_matches(|c: char| c.is_whitespace());
        if echo.is_empty() {
            return true;
        }
        !visible.replacen(echo, "", 1).trim().is_empty()
    }

    /// 실패 시 watcher 상태와 화면 꼬리를 수집한다. raw 읽기/쓰기 바이트 계수는 없다.
    /// 화면과 에코 비교는 관측 보조 자료이며 자식의 생사나 실행 이력을 보장하지 않는다.
    fn observed_pty_state(engine: &CoreState, pty_id: u32, sent: &str) -> String {
        let watcher = match engine.pty_registry.get(pty_id) {
            Some(e) => watch_phase_note(e.watch_phase()),
            None => "registry에 항목이 없어 watcher 상태를 읽을 수 없다",
        };
        let Some(t) = engine.find_terminal_by_id(pty_id) else {
            return format!(
                "관측: [{watcher}] · [Terminal 없음: registry와 저장소가 불일치해 화면을 읽을 수 없다]"
            );
        };
        let screen = t.screen_text(false);
        let visible = screen.trim_end();
        let tail: String = visible
            .chars()
            .rev()
            .take(48)
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        let shell = if visible.trim().is_empty() {
            "현재 화면에 텍스트가 없다. 이 값만으로 실행 여부를 판단할 수 없다"
        } else if screen_holds_more_than_our_echo(visible, sent) {
            "에코와 다른 텍스트가 화면에 있다. 종료 상태는 watcher와 함께 확인한다"
        } else {
            "보낸 입력의 에코만 있다. 셸 실행 여부는 확인할 수 없다"
        };
        format!(
            "관측: [{watcher}] · [화면 {shell}] (scrollback {sb} 줄, alt-screen {alt}, \
             보낸 것=\"{sent}\", 꼬리=\"{tail}\")",
            sb = t.scrollback_len(),
            alt = t.is_alternate_screen(),
            sent = sent.escape_default(),
            tail = tail.escape_default(),
        )
    }

    #[test]
    fn the_watcher_phase_points_at_the_child_or_at_us_but_never_at_both() {
        let ours = [
            watch_phase_note(WatchPhase::NotAttached),
            watch_phase_note(WatchPhase::Spawned),
        ];
        for note in ours {
            assert!(
                !note.contains("자식이 안 죽었다"),
                "watcher 상태만으로 자식의 생존 여부를 단정했다: {note}"
            );
        }
        assert!(
            watch_phase_note(WatchPhase::Waiting).contains("결과 기록 완료는 아직 확인되지 않았다")
        );

        // Reaped는 결과 기록 완료만 말한다. 자식 종료 시각을 단정하지 않아야 한다.
        let reaped = watch_phase_note(WatchPhase::Reaped);
        assert!(
            !reaped.contains("대기 쪽"),
            "완료 상태에서 결과 미수신 대기를 원인으로 단정했다: {reaped}"
        );
        assert!(reaped.contains("종료 결과가 기록됐다"), "{reaped}");
        assert!(reaped.contains("ADR-0046"), "{reaped}");

        let all = [
            watch_phase_note(WatchPhase::NotAttached),
            watch_phase_note(WatchPhase::Spawned),
            watch_phase_note(WatchPhase::Waiting),
            watch_phase_note(WatchPhase::Reaped),
        ];
        let mut seen: Vec<&str> = all.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 4, "watcher 상태별 진단이 같아졌다: {all:?}");
    }

    #[test]
    fn the_echo_of_what_we_sent_is_not_evidence_that_the_child_ran() {
        // 실제 실패에서 받은 에코만 있는 화면. 셸 실행의 증거로 해석하면 안 된다.
        assert!(
            !screen_holds_more_than_our_echo("exit 7", "exit 7"),
            "우리가 보낸 것과 같은 화면을 자식의 출력으로 셌다"
        );
        assert!(
            !screen_holds_more_than_our_echo("exit 3", "exit 3\n"),
            "보낸 것의 개행 차이로 에코 판정이 갈렸다"
        );

        assert!(screen_holds_more_than_our_echo(
            "user@host ~ % exit 7",
            "exit 7"
        ));
        assert!(!screen_holds_more_than_our_echo("   ", "exit 7"));
        assert!(screen_holds_more_than_our_echo("$ ", ""));
    }

    #[test]
    fn the_exit_wait_failure_tells_which_of_the_two_events_happened() {
        let budget = std::time::Duration::from_secs(30);

        let gone = exit_wait_failure(7, std::time::Duration::from_millis(1), budget, false, "");
        assert!(gone.contains("레지스트리에 없다"), "{gone}");
        assert!(gone.contains("대상 부재"), "{gone}");

        let spent = exit_wait_failure(7, budget, budget, true, "관측: 화면 빈 채 — X, 꼬리=\"$\"");
        assert!(spent.contains("제한 시간"), "{spent}");
        assert!(
            spent.contains("관측:"),
            "제한 시간 초과 진단에 관측 결과가 없다: {spent}"
        );
        assert!(
            !spent.contains("레지스트리에 없다"),
            "대상이 있는데 없다고 적었다: {spent}"
        );

        let odd = exit_wait_failure(7, std::time::Duration::from_millis(1), budget, true, "");
        assert!(odd.contains("일어나면 안 된다"), "{odd}");
        assert!(
            !odd.contains("레지스트리에 없다") && !odd.contains("안에 받지 못했다"),
            "조기 반환 진단이 대상 부재나 시간 초과 진단과 구분되지 않는다: {odd}"
        );
    }

    #[test]
    fn spawn_write_wait_kill_list_e2e() {
        let mut e = engine();
        let (mut c, _home) = core();
        let caller = CallerContext::Local;

        let resp = handle_spawn(&mut c, &mut e, &caller, json!(1), &json!({}));
        let spawned = ok(resp);
        let pty_id = spawned["pty_id"].as_u64().unwrap() as u32;
        assert!(pty_id >= crate::core::pty_registry::PTY_ID_BASE);

        let listed = ok(handle_list(&mut e, json!(2)));
        let arr = listed["ptys"].as_array().unwrap();
        assert!(arr.iter().any(|p| p["id"].as_u64() == Some(pty_id as u64)));

        let w = ok(handle_write(
            &mut e,
            json!(3),
            &json!({ "id": pty_id, "text": "exit 3\n" }),
        ));
        assert_eq!(w["id"].as_u64(), Some(pty_id as u64));

        let exited = wait_for_exit(&mut e, pty_id, "exit 3\n");
        assert_eq!(exited["exit_code"].as_i64(), Some(3));
        assert_eq!(exited["success"], Value::Bool(false));

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
        let exited = wait_for_exit(&mut e, pty_id, "exit 7");
        assert_eq!(exited["exit_code"].as_i64(), Some(7));
        handle_kill(&mut e, json!(9), &json!({ "id": pty_id }));
    }

    #[test]
    fn spawn_beyond_limit_returns_error() {
        let mut e = engine();
        let (mut c, _home) = core();
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
        for pid in e.pty_registry.ids() {
            handle_kill(&mut e, json!(0), &json!({ "id": pid }));
        }
    }

    #[test]
    fn kill_forgets_only_target_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        let mut e = engine();
        let (mut c, _home) = core();
        let factory = RecordingWakerFactory::new();
        let shared: crate::waker::SharedWakerFactory = factory.clone();
        e.waker_factory = Some(shared);
        let caller = CallerContext::Local;

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

        ok(handle_kill(&mut e, json!(3), &json!({ "id": a })));
        assert!(
            factory.forgotten().contains(&a),
            "handle_kill 은 회수하는 pty_id 의 waker 게이트를 정리해야 한다"
        );
        assert!(
            !factory.forgotten().contains(&b),
            "kill 은 대상 pty 의 게이트만 정리(다른 pty 보존)"
        );

        handle_kill(&mut e, json!(4), &json!({ "id": b }));
    }

    #[test]
    fn idle_sweep_forgets_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        let mut e = engine();
        let (mut c, _home) = core();
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

    #[derive(Default)]
    struct RecordingWakerFactory {
        forgotten: std::sync::Mutex<Vec<u32>>,
    }

    impl tasty_terminal::waker_factory::WakerFactory for RecordingWakerFactory {
        fn make_targeted_waker(&self, _surface_id: u32) -> tasty_terminal::Waker {
            std::sync::Arc::new(|| {})
        }
        fn make_default_waker(&self) -> tasty_terminal::Waker {
            std::sync::Arc::new(|| {})
        }
        fn note_drained(&self, _surface_id: Option<u32>) {}
        fn forget_surface(&self, surface_id: u32) {
            self.forgotten
                .lock()
                .expect("forgotten poisoned")
                .push(surface_id);
        }
    }

    fn short_ttl_registry(max: usize) -> crate::core::pty_registry::PtyRegistry {
        crate::core::pty_registry::PtyRegistry::with_limits(max, Duration::from_millis(1))
    }

    // 주기 정리도 registry·Terminal·waker를 모두 회수해야 한다.
    #[test]
    fn periodic_sweep_clears_registry_terminal_store_and_waker_gate() {
        let mut e = engine();
        let (mut c, _home) = core();
        let recorder = std::sync::Arc::new(RecordingWakerFactory::default());
        e.waker_factory = Some(recorder.clone());
        e.pty_registry = short_ttl_registry(8);

        let spawned = ok(handle_spawn(
            &mut c,
            &mut e,
            &CallerContext::Local,
            json!(1),
            &json!({}),
        ));
        let pty_id = spawned["pty_id"].as_u64().expect("pty_id") as u32;
        assert!(e.pty_registry.contains(pty_id), "registry entry");
        assert!(e.terminals.get(pty_id).is_some(), "TerminalStore entry");

        std::thread::sleep(Duration::from_millis(5));
        let reaped = e.sweep_idle_ptys(Instant::now());

        assert_eq!(reaped, vec![pty_id], "회수 id");
        assert!(!e.pty_registry.contains(pty_id), "registry 에서 제거");
        assert!(
            e.terminals.get(pty_id).is_none(),
            "TerminalStore에서도 제거해 PTY master를 닫아야 한다"
        );
        assert_eq!(
            *recorder.forgotten.lock().expect("forgotten poisoned"),
            vec![pty_id],
            "waker dedup 게이트 해제 — 미해제 시 회수마다 게이트가 영구 누적된다"
        );
    }

    // 주기 정리를 기다리지 않고 spawn 직전에 만료 항목을 회수해야 상한을 정확히 검사한다.
    #[test]
    fn spawn_still_reclaims_idle_slots_before_checking_the_limit() {
        let mut e = engine();
        let (mut c, _home) = core();
        e.pty_registry = short_ttl_registry(1);
        e.pty_registry
            .register(
                crate::core::pty_registry::PtySpawnSpec {
                    owner_agent_id: "agent-1".into(),
                    cwd: None,
                    command: vec!["sleep".into(), "3600".into()],
                },
                Instant::now(),
            )
            .expect("첫 등록은 상한 안");
        std::thread::sleep(Duration::from_millis(5));

        let resp = handle_spawn(&mut c, &mut e, &CallerContext::Local, json!(1), &json!({}));
        assert!(
            resp.error.is_none(),
            "lazy sweep 이 먼저 돌아 슬롯을 회수해야 한다 (상한 초과 실패 = lazy 가 제거된 것): {:?}",
            resp.error
        );
    }
}
