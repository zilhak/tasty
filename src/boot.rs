//! `fn main` 부팅 시퀀스 오케스트레이션.
//!
//! `run()` 이 단일 진입점. 내부 단계 순서:
//!
//! 1. OS 보정 (Windows console attach, crash_report::init)
//! 2. CLI 라우팅 결정 (`cli_routing::parse_or_route`)
//! 3. 결정에 따라 mode helper 호출:
//!    - `AlreadyHandled` → Ok(())
//!    - `Subcommand` → i18n init + `cli::run_client`
//!    - `AugmentedHelp` → i18n init + `cli::print_augmented_help`
//!    - `Gui` → i18n init + event loop / background threads / App / event_loop.run_app
//!      (gui 빌드 + `!cli.headless`) — 또는 `run_headless` (headless 빌드 / `--headless`)

#[cfg(feature = "gui")]
pub(crate) mod attach_tick;
#[cfg(feature = "gui")]
pub(crate) mod busy_tick;
pub(crate) mod cli_routing;
#[cfg(feature = "gui")]
pub(crate) mod event_loop;
#[cfg(not(feature = "gui"))]
pub(crate) mod headless_dispatch;
pub(crate) mod locale;
pub(crate) mod os;
#[cfg(feature = "gui")]
pub(crate) mod waker;
pub(crate) mod wiring;

#[cfg(feature = "gui")]
use crate::App;
use crate::{cli, hooks};

/// boot 시 1회 memory.db 위생 정리.
///
/// audit/telemetry 는 append-only 로그라 `memory` 테이블을 무한 채운다(per-IPC audit
/// 가 수십만 행 누적). put 은 이제 O(1)(전체 스캔 제거)이라 성능 목적은 아니며, 무한
/// 누적으로 인한 디스크 증가와 1GB regular quota 도달을 막는 count 기반 retention 이다.
/// 최근 N 개만 남기고 조용히(이벤트 없이) 삭제 후, 단편화가 크면 1회 VACUUM 으로 회수.
/// 최초 1회만 대량(수십만 행) 삭제로 ~2s 소요될 수 있고 이후 부팅은 초과분만 정리한다.
fn maintain_memory_at_boot(arc: &std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>) {
    // 로그 키별 보존 개수 상한. audit 은 보안 감사용이라 넉넉히, telemetry 는 짧게.
    const AUDIT_KEEP: u64 = 50_000;
    const TELEMETRY_KEEP: u64 = 20_000;

    let mut store = match arc.lock() {
        Ok(s) => s,
        Err(p) => p.into_inner(),
    };
    let mut pruned = 0u64;
    for (prefix, keep) in [
        (crate::adapters::ipc::audit::AUDIT_KEY_PREFIX, AUDIT_KEEP),
        (tasty_telemetry::EVENT_KEY_PREFIX, TELEMETRY_KEEP),
    ] {
        match store.prune_prefix_keep_recent(prefix, keep) {
            Ok(n) => pruned += n,
            Err(e) => tracing::warn!("boot memory maintenance: prune {prefix} failed: {e}"),
        }
    }
    if pruned > 0 {
        tracing::info!("boot memory maintenance: pruned {pruned} stale log rows");
        // 대량 삭제 직후에만 압축 (freelist 가 클 때). 평소 부팅은 no-op.
        match store.vacuum_if_fragmented(10_000) {
            Ok(true) => tracing::info!("boot memory maintenance: vacuumed memory.db"),
            Ok(false) => {}
            Err(e) => tracing::warn!("boot memory maintenance: vacuum failed: {e}"),
        }
    }
}

pub(crate) fn run() -> anyhow::Result<()> {
    os::attach_windows_console_if_needed();
    os::init_crash_report();

    match cli_routing::parse_or_route()? {
        cli_routing::Routed::AlreadyHandled => Ok(()),
        cli_routing::Routed::Subcommand(cmd, port_file) => run_subcommand(cmd, port_file),
        cli_routing::Routed::AugmentedHelp => run_augmented_help(),
        cli_routing::Routed::Gui(cli) => {
            // 호스트(터미널·plugin 을 spawn 하는 프로세스)에서만 자식 결박 job 을 생성한다.
            // CLI client / augmented-help 경로는 터미널을 띄우지 않으므로 제외. 이 job 이
            // tasty 프로세스 사망 시 자식 셸 트리를 함께 정리한다(비-Windows 는 no-op).
            tasty_reaper::init_host_reaper();
            #[cfg(feature = "gui")]
            {
                if cli.headless {
                    tracing::warn!(
                        "--headless requested in gui build; gui build does not embed headless mode. \
                         Build with --no-default-features to enable headless. Falling back to run_gui."
                    );
                }
                run_gui(cli)
            }
            #[cfg(not(feature = "gui"))]
            {
                run_headless(cli)
            }
        }
    }
}

/// `cli.command.is_some()` — i18n 후 client mode 진입.
fn run_subcommand(cmd: cli::Commands, port_file: Option<String>) -> anyhow::Result<()> {
    locale::init();
    cli::run_client(cmd, port_file.as_deref())
}

/// `TASTY_SURFACE_ID` + `!cli.launch` — i18n 후 augmented help 출력.
fn run_augmented_help() -> anyhow::Result<()> {
    locale::init();
    cli::print_augmented_help()
}

/// 본 GUI 부트.
#[cfg(feature = "gui")]
fn run_gui(cli: cli::Cli) -> anyhow::Result<()> {
    locale::init();

    let (event_loop, proxy) = event_loop::build()?;
    os::install_macos_delegate(&proxy);

    busy_tick::spawn(proxy.clone());
    attach_tick::spawn(proxy.clone());

    // CWD는 OSC 7 시퀀스에만 의존한다. 모든 플랫폼 공통.
    // zsh/fish는 기본 지원, bash는 PROMPT_COMMAND 설정 필요.

    // Phase D.3.C.M.19 — Settings 와 Memory store 를 App 생성 *이전* 에 초기화.
    // Core 가 처음부터 실 Memory store 의 Arc 를 보유한다. 글로벌 STORE 싱글톤은
    // 폐기됨 — Arc 가 유일한 store handle.
    let boot_settings = crate::settings::Settings::load();
    let memory_config = tasty_memory::MemoryConfig {
        entry_max_bytes: boot_settings
            .memory
            .entry_max_mb
            .saturating_mul(1024 * 1024),
        secret_quota_per_owner_bytes: boot_settings
            .memory
            .secret_quota_mb_per_plugin
            .saturating_mul(1024 * 1024),
        regular_quota_total_bytes: boot_settings
            .memory
            .regular_quota_mb_total
            .saturating_mul(1024 * 1024),
    };
    let memory_arc = match tasty_memory::init_with_config(memory_config) {
        Ok(arc) => {
            maintain_memory_at_boot(&arc);
            Some(arc)
        }
        Err(e) => {
            tracing::warn!("memory.db init at boot failed: {e}");
            None
        }
    };

    let mut app = App::new(
        proxy,
        cli.port_file,
        memory_arc,
        #[cfg(debug_assertions)]
        cli.enable_input_simulation,
    )?;
    hooks::lua::fire(
        app.lua_engine.as_ref(),
        hooks::lua::AutofireCtx {
            scripts: &boot_settings.scripts,
            guard: &mut app.lua_autofire,
        },
        "tasty.startup.post",
        &serde_json::Value::Null,
    );
    event_loop.run_app(&mut app)?;

    Ok(())
}

/// Headless 부트. winit / wgpu / egui 가 없는 빌드 (`--no-default-features`) 전용.
///
/// 시퀀스:
/// 1. `mpsc::channel::<AppEvent>` 생성 + `HeadlessWaker` 로 IPC/PTY waker 발급
/// 2. Settings/Memory store 초기화 (gui 와 동일 정책)
/// 3. `App::new_headless` 로 Core+Hub+plugin_manager 초기화
/// 4. `hub.start_ipc(ipc_waker, stream_ctx)` — accept 스레드 분리 (+ 스트림 승격 경로)
/// 5. `rx.recv()` loop — Shutdown / QuitRequested 수신 시 break
#[cfg(not(feature = "gui"))]
fn run_headless(cli: cli::Cli) -> anyhow::Result<()> {
    use std::sync::mpsc;

    use crate::AppEvent;
    use crate::adapters::production::headless_waker::HeadlessWaker;
    use crate::app::App;

    locale::init();

    let (tx, rx) = mpsc::channel::<AppEvent>();
    let waker = HeadlessWaker::new(tx);

    let boot_settings = crate::settings::Settings::load();
    let memory_config = tasty_memory::MemoryConfig {
        entry_max_bytes: boot_settings
            .memory
            .entry_max_mb
            .saturating_mul(1024 * 1024),
        secret_quota_per_owner_bytes: boot_settings
            .memory
            .secret_quota_mb_per_plugin
            .saturating_mul(1024 * 1024),
        regular_quota_total_bytes: boot_settings
            .memory
            .regular_quota_mb_total
            .saturating_mul(1024 * 1024),
    };
    let memory_arc = match tasty_memory::init_with_config(memory_config) {
        Ok(arc) => {
            maintain_memory_at_boot(&arc);
            Some(arc)
        }
        Err(e) => {
            tracing::warn!("memory.db init at boot failed: {e}");
            None
        }
    };

    let mut app = App::new_headless(cli.port_file, memory_arc)?;
    let stream_ctx = crate::adapters::production::stream_hub::StreamContext {
        hub: app.stream_hub.clone(),
        inbound_tx: app.stream_inbound_tx.clone(),
        waker: waker.stream_waker(),
    };
    if let Some(injector) = app.hub.start_ipc(waker.ipc_waker(), stream_ctx) {
        app.core.set_host_ipc_injector(injector);
    }

    // ── Engine 부트스트랩 ──────────────────────────────────────────────
    // gui 는 첫 MainView 생성 시 CoreState/AppState 를 만든다 (window_lifecycle).
    // headless 는 창이 없으므로 여기서 직접 1 회 만든다. `CoreState::new_with_ids`
    // 가 default workspace + 터미널 1 개를 spawn 하므로 client 0 명에도 PTY 가 산다.
    // 터미널 reader 스레드는 factory 가 발급한 waker 로 `TerminalOutput` 을 push,
    // 아래 메인 루프가 `process_all_pty_output` 으로 채널을 drain 한다.
    //
    // 0-B: 창이 없어 grid 크기를 측정할 수 없으므로 기본 80×24.
    // 0-C: layout 복원은 gui 의 plugin-pump 경로(ApplyPendingLayoutRestore)에
    //      종속이라 headless 엔 미적용 — 항상 fallback default workspace 로 뜬다.
    let factory = waker.waker_factory();
    let base_waker = factory.make_default_waker();
    let mut engine =
        crate::core::CoreState::new_with_ids(80, 24, base_waker, None, app.core.memory_arc())?;
    engine.waker_factory = Some(factory);
    // attach/detach 단계 3: force-detach 통지가 stream client 로 push 되도록 IPC
    // 서버와 동일한 StreamHub 를 attach registry 에 주입.
    engine.attach.set_notifier(app.stream_hub.clone());
    let preset_store = app.core.preset_store.clone();
    let memory = app.core.memory_arc();
    let mut state = crate::state::AppState::new(&mut engine, preset_store, memory);

    hooks::lua::fire(
        app.lua_engine.as_ref(),
        hooks::lua::AutofireCtx {
            scripts: &boot_settings.scripts,
            guard: &mut app.lua_autofire,
        },
        "tasty.startup.post",
        &serde_json::Value::Null,
    );

    tracing::info!("headless daemon ready; PTY pump + IPC dispatch active");

    while let Ok(event) = rx.recv() {
        match event {
            AppEvent::Shutdown | AppEvent::QuitRequested => break,
            AppEvent::TerminalOutput(id) => {
                // Early reset: drain 직전에 dedup 게이트를 풀어 경합 wake 유실 방지
                // (research §8). headless 는 단일 engine 이라 순회 불필요.
                if let Some(factory) = engine.waker_factory.as_ref() {
                    factory.note_drained(id);
                }
                // Targeted wake 는 해당 surface 만, default wake 는 전체 drain.
                // 반환 CoreEvent (Notification/Bell/Title/Cwd/Exit) 는 cascade 주체
                // (view/plugin)가 없으므로 단계 0 에선 무시한다 — 직접 부수효과
                // (observer/command_index/OSC52) 는 process 함수 내부에서 이미 적용됨.
                match id {
                    Some(sid) => {
                        let _ = app.core.process_pty_output(&mut engine, sid); // targeted: 해당 surface 만 drain — CoreEvent 무시 사유는 위 주석 참조
                    }
                    None => {
                        let _ = app.core.process_all_pty_output(&mut engine); // default: 전체 drain — CoreEvent 무시 사유는 위 주석 참조
                    }
                }
            }
            AppEvent::IpcReady => {
                headless_dispatch::pump_ipc(&mut app, &mut state, &mut engine);
            }
            AppEvent::StreamReady => {
                // 스트림 클라 inbound 를 분류해 attach 결선(단계 4): attach 요청 →
                // lock+스냅샷+출력 forward, 입력 Data → 점유 surface PTY, 끊김 →
                // lock free 환원(단계 3). 비-attach client 의 Data 는 debug echo.
                let outcome = app.stream_hub.pump_inbound(&app.stream_inbound_rx);
                for (client_id, surface_id) in outcome.attach_requests {
                    engine.attach_surface_for_stream(surface_id, client_id, &app.stream_hub);
                }
                for (client_id, workspace_id) in outcome.workspace_attach_requests {
                    engine.attach_workspace_for_stream(workspace_id, client_id, &app.stream_hub);
                }
                for (client_id, bytes) in outcome.input_frames {
                    // workspace mode(단계 6)면 입력은 surface-prefixed → demux 후 지정
                    // surface 로. 아니면 단계 4 의 bare 입력(점유 단일 surface).
                    let routed = if engine.attach.client_holds_workspace(client_id) {
                        match crate::ipc::stream::decode_mux(&bytes) {
                            Some((sid, payload)) => {
                                engine.feed_attached_workspace_input(client_id, sid, payload)
                            }
                            None => false,
                        }
                    } else {
                        engine.feed_attached_input(client_id, &bytes)
                    };
                    #[cfg(debug_assertions)]
                    if !routed {
                        // 단계 1 echo client(점유 surface 없음): debug 빌드 회신.
                        let echo_frame = crate::ipc::stream::StreamFrame::new(
                            crate::ipc::stream::StreamTag::Data,
                            bytes,
                        );
                        let _ = app.stream_hub.push(client_id, echo_frame); // best-effort echo — PushResult(Result 아님) 무시: client 끊김 시 무해.
                    }
                    #[cfg(not(debug_assertions))]
                    let _ = routed; // release: echo 분기 없어 routed 미사용 — 값 drop(Result 아님).
                }
                for (client_id, op_id, op) in outcome.structural_ops {
                    // mirror client 가 forward 한 구조 op — anchor 워크스페이스를 그
                    // client 가 점유(holder)할 때만 실행하고 StructuralResult 로 회신.
                    let anchor = op.anchor_surface_id();
                    let result = match engine.attach.workspace_of_surface(anchor) {
                        Some(ws) if engine.attach.workspace_holder(ws) == Some(client_id) => {
                            crate::core::attach_runtime::execute_forwarded_structural_op(
                                &mut app.core,
                                &mut state,
                                &mut engine,
                                &op,
                            )
                        }
                        Some(_) => Err("not workspace holder".to_string()),
                        None => Err("workspace not found".to_string()),
                    };
                    let (ok, reason) = match result {
                        Ok(()) => (true, None),
                        Err(reason) => (false, Some(reason)),
                    };
                    let reply =
                        crate::ipc::stream::StreamControl::StructuralResult { op_id, ok, reason };
                    let frame = crate::ipc::stream::StreamFrame::new(
                        crate::ipc::stream::StreamTag::Control,
                        serde_json::to_vec(&reply).unwrap_or_default(),
                    );
                    let _ = app.stream_hub.push(client_id, frame); // best-effort 회신 — 무시.
                }
                for client_id in outcome.disconnected {
                    engine.attach.release_all_for_client(client_id);
                }
            }
            AppEvent::BusyPoll => {
                // 단계 0 범위 밖 — busy indicator 미구현.
            }
            AppEvent::AttachPoll => {
                // headless 는 렌더가 없어 readonly display mirror·client mirror 가
                // 무의미하다(작업 J 는 GUI 통합). gui 에서만 처리한다.
            }
            AppEvent::RunLuaScript { source, name } => {
                // gui event_handler 와 동일 처리. headless 발신원은 현재 없지만
                // (단축키=gui, debug IPC=App 경로) 이벤트 계약상 동작을 미러링한다.
                if let Some(engine) = app.lua_engine.as_ref() {
                    engine.run_script(&source, Some(&name));
                } else {
                    tracing::warn!(target: "tasty_lua", "RunLuaScript dropped — lua engine unavailable");
                }
            }
        }
    }
    Ok(())
}
