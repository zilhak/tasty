//! CLI를 먼저 처리하고 호스트 실행이면 gui feature에 따라 GUI 또는 헤드리스를 시작한다.
//! GUI 빌드는 --headless를 경고 후 무시한다.

pub(crate) mod cli_routing;
#[cfg(feature = "gui")]
pub(crate) mod event_loop;
#[cfg(not(feature = "gui"))]
pub(crate) mod headless_dispatch;
#[cfg(not(feature = "gui"))]
pub(crate) mod headless_plugins;
#[cfg(not(feature = "gui"))]
pub(crate) mod headless_stream;
pub(crate) mod locale;
pub(crate) mod locale_font;
pub(crate) mod os;
#[cfg(feature = "gui")]
pub(crate) mod trace;
#[cfg(feature = "gui")]
pub(crate) mod waker;
pub(crate) mod wiring;

#[cfg(feature = "gui")]
use crate::App;
use crate::{cli, hooks};

fn log_vacuum_result(result: tasty_memory::Result<bool>) {
    match result {
        Ok(true) => tracing::info!("boot memory maintenance: vacuumed memory.db"),
        Ok(false) => {}
        Err(e) => tracing::warn!("boot memory maintenance: vacuum failed: {e}"),
    }
}

fn vacuum_if_needed(store: &mut tasty_memory::MemoryStore, pruned: u64) {
    if pruned == 0 {
        return;
    }
    tracing::info!("boot memory maintenance: pruned {pruned} stale log rows");
    log_vacuum_result(store.vacuum_if_fragmented(10_000));
}

/// VACUUM이 WAL을 늘릴 수 있어 그 뒤에 축소를 요청한다.
/// journal_size_limit은 WAL 재사용 때 적용되며 활성 WAL의 크기 상한은 아니다.
fn truncate_wal(store: &mut tasty_memory::MemoryStore) {
    match store.checkpoint_truncate() {
        Ok(true) => {}
        // 다른 연결 때문에 축소하지 못했어도 부팅은 계속한다.
        Ok(false) => {
            tracing::info!("boot memory maintenance: wal checkpoint was busy; wal left as is")
        }
        Err(e) => tracing::warn!("boot memory maintenance: wal checkpoint failed: {e}"),
    }
}

/// 런타임과 같은 로그 보존 정책을 적용하고 필요하면 VACUUM·WAL 축소를 시도한다.
fn maintain_memory_at_boot(arc: &std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>) {
    let mut store = crate::poison::recover_mutex(
        arc.lock(),
        crate::core::MEMORY_WHAT,
        &crate::core::MEMORY_POISONED,
    );
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut pruned = 0u64;
    for policy in &crate::store::log_retention::ALL {
        pruned += policy.enforce(&mut *store, now_ms);
    }
    vacuum_if_needed(&mut store, pruned);
    truncate_wal(&mut store);
}

/// main에서 호출하는 프로세스 진입점.
pub fn run() -> anyhow::Result<()> {
    os::attach_windows_console_if_needed();
    os::init_crash_report();

    match cli_routing::parse_or_route()? {
        cli_routing::Routed::AlreadyHandled => Ok(()),
        cli_routing::Routed::Subcommand(cmd, port_file, envelope) => {
            run_subcommand(cmd, port_file, envelope)
        }
        cli_routing::Routed::AugmentedHelp => run_augmented_help(),
        cli_routing::Routed::Gui(cli) => {
            // CLI도 같은 바이너리라 호스트로 결정한 뒤 열어야 실행 중 호스트의 로그를 자르지 않는다.
            os::enable_host_file_log();
            // Windows 호스트 종료 시 자식 셸도 정리하도록 job을 만든다. CLI에는 만들지 않는다.
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

fn run_subcommand(
    cmd: cli::Commands,
    port_file: Option<String>,
    envelope: cli::Envelope,
) -> anyhow::Result<()> {
    locale::init();
    cli::run_client_with(cmd, port_file.as_deref(), envelope)
}

fn run_augmented_help() -> anyhow::Result<()> {
    locale::init();
    cli::print_augmented_help()
}

#[cfg(feature = "gui")]
fn run_gui(cli: cli::Cli) -> anyhow::Result<()> {
    locale::init();

    let (event_loop, proxy) = event_loop::build()?;
    os::install_macos_delegate(&proxy);

    // 탭의 CWD 표시는 OSC 7을 사용한다. bash는 PROMPT_COMMAND 설정이 필요하다.

    // App을 만들기 전에 메모리 저장소를 열어 Core에 같은 Arc를 전달한다.
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
            crate::boot::wiring::memory_fallback_after(&e)
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
    crate::stall_watchdog::spawn();
    event_loop.run_app(&mut app)?;
    drop_app_with_trace(app);

    Ok(())
}

/// event_loop 종료 이후 App Drop도 기다릴 수 있어 별도 시간으로 측정한다.
#[cfg(feature = "gui")]
fn drop_app_with_trace(app: App) {
    use std::time::Instant;

    use crate::app::shutdown_trace;

    // 여러 destructor 호출의 시간을 전역 누적기 전후 차이로 합산한다.
    let before = DropTailCounters::snapshot();

    let t_drop = Instant::now();
    drop(app);
    let drop_ms = shutdown_trace::elapsed_ms(t_drop);

    DropTailCounters::snapshot().log_delta(&before);
    tracing::info!(
        target: "tasty::shutdown",
        ms = drop_ms,
        "S5 drop_tail (run_app return -> App drop 완료)"
    );
    if let Some(t0) = shutdown_trace::started_at() {
        tracing::info!(
            target: "tasty::shutdown",
            ms = shutdown_trace::elapsed_ms(t0),
            "shutdown_total_with_drop (사용자 체감 종료 시간)"
        );
    }
}

#[cfg(feature = "gui")]
struct DropTailCounters {
    pty: (std::time::Duration, u64),
    ssh: (std::time::Duration, u64),
}

#[cfg(feature = "gui")]
impl DropTailCounters {
    fn snapshot() -> Self {
        Self {
            pty: tasty_terminal::pty_drop_totals(),
            ssh: tasty_ssh::tunnel_drop_totals(),
        }
    }

    fn log_delta(&self, before: &Self) {
        use crate::app::shutdown_trace::duration_ms;

        tracing::info!(
            target: "tasty::shutdown",
            ms = duration_ms(self.pty.0.saturating_sub(before.pty.0)),
            // layout surface 수가 아니라 실제 PTY를 가진 backend 수다.
            ptys = self.pty.1 - before.pty.1,
            "S5b pty_drop (PtyBackend::drop 합계)"
        );
        tracing::info!(
            target: "tasty::shutdown",
            ms = duration_ms(self.ssh.0.saturating_sub(before.ssh.0)),
            tunnels = self.ssh.1 - before.ssh.1,
            "S5c ssh_tunnel_drop (SshTunnel::drop 합계)"
        );
    }
}

#[cfg(not(feature = "gui"))]
fn run_due_timers(
    app: &mut crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) {
    use std::time::Instant;

    for key in app.timers.drain_due(Instant::now()) {
        match key {
            crate::app::timers::Tick::Busy => {
                // 화면은 없지만 원격 mirror에 상태를 전달해야 한다. 읽는 곳이 없는 StatusBar 브랜치 캐시는 갱신하지 않는다.
                engine.refresh_busy_surfaces();
                engine.forward_busy_activity(&app.stream_hub);
                engine.forward_attention(&app.stream_hub);
                engine.forward_surface_cwd(&app.stream_hub);
                engine.poll_global_hooks();
                let injector = app.core.host_ipc_injector.get().cloned();
                for (surface_id, f) in engine.poll_idle_timeout_hooks() {
                    crate::hook_handler::trigger::execute_binding(
                        &f.binding,
                        injector.as_ref(),
                        &f.event,
                        &f.received,
                        surface_id,
                    );
                    state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                        hook_id: f.hook_id,
                        event_kind: "idle-timeout".to_string(),
                        surface_id,
                        exit_code: None,
                    });
                }
                // 플러그인 소켓 입력이 없어도 상태 확인·재시작을 진행하는 주기 경로다.
                headless_plugins::pump_plugins(app, state, engine);
            }
            crate::app::timers::Tick::PtySweep => {
                // 회수는 함수 안에서 끝나며 반환 ID 목록은 여기서 사용하지 않는다.
                let _ = engine.sweep_idle_ptys(Instant::now());
            }
            crate::app::timers::Tick::CaptureSweep => {
                engine.capture_uploads.sweep_expired(Instant::now());
            }
            crate::app::timers::Tick::LogPrune => {
                let now_ms = u64::try_from(app.core.now_unix_millis()).unwrap_or(0);
                app.core.with_memory(|mem| {
                    crate::store::log_retention::maybe_prune(mem, now_ms);
                });
            }
        }
    }
    // 두 허브의 최솟값으로 기다렸으므로 플러그인 기한도 함께 처리한다.
    headless_plugins::pump_plugins_if_due(app, state, engine, Instant::now());
}

#[cfg(not(feature = "gui"))]
fn handle_terminal_output(
    app: &mut crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    id: Option<u32>,
) {
    // drain 전에 깨움 중복 방지 표지를 풀어 처리 도중 새 출력의 깨움을 잃지 않게 한다.
    if let Some(factory) = engine.waker_factory.as_ref() {
        factory.note_drained(id);
    }
    let outcome = match id {
        Some(sid) => app.core.process_pty_output(engine, sid),
        None => {
            let outcome = app.core.process_all_pty_output(engine);
            // 플러그인 수신도 이 기본 waker를 공유하므로 함께 처리한다.
            headless_plugins::pump_plugins(app, state, engine);
            outcome
        }
    };
    for event in outcome.events {
        match event {
            crate::core::intent::CoreEvent::TerminalProcessExited { surface_id } => {
                crate::app::process_exit::handle(&mut app.core, state, engine, surface_id);
            }
            crate::core::intent::CoreEvent::TerminalCwdChanged { surface_id } => {
                crate::intent::headless::apply_terminal_cwd_changed(engine, surface_id);
            }
            event => fire_terminal_hooks(app, state, engine, vec![event]),
        }
    }
    crate::intent::headless::drain_pending_host_events(&app.core, state, engine);
}

/// output-match 바인딩을 실행한다. PTY 종료는 호출자가 먼저 공용 process_exit 처리로 분기한다.
/// 직접 종료 이벤트가 들어온 경우에도 닫기 전 바인딩을 모으고 닫은 뒤 실행한다.
#[cfg(not(feature = "gui"))]
fn fire_terminal_hooks(
    app: &crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    events: Vec<crate::core::intent::CoreEvent>,
) {
    let injector = app.core.host_ipc_injector.get().cloned();
    for event in events {
        let (surface_id, hook_event, exited) = match event {
            crate::core::intent::CoreEvent::TerminalOutputMatch { surface_id, text } => {
                (surface_id, tasty_hooks::HookEvent::OutputMatch(text), false)
            }
            crate::core::intent::CoreEvent::TerminalProcessExited { surface_id } => {
                (surface_id, tasty_hooks::HookEvent::ProcessExit, true)
            }
            _ => continue,
        };
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[hook_event]);
        if exited {
            // intent-exempt: headless PTY 종료 이벤트의 cascade — GUI와 같은 종료 정리 경계다.
            state.close_surface_by_id_no_snapshot(engine, surface_id, true);
        }
        for f in fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                &f.received,
                surface_id,
            );
        }
    }
}

/// 메모리 저장소 초기화 실패는 로그를 남기고 None으로 반환해 계속 실행한다.
#[cfg(not(feature = "gui"))]
fn boot_memory(
    boot_settings: &crate::settings::Settings,
) -> Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>> {
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
            crate::boot::wiring::memory_fallback_after(&e)
        }
    };
    memory_arc
}

/// IPC가 시작됐을 때만 라우터에 의존하는 훅·완료 전략·웹훅을 초기화한다.
#[cfg(not(feature = "gui"))]
fn start_ipc_and_seed(
    app: &mut crate::app::App,
    waker: &crate::adapters::production::headless_waker::HeadlessWaker,
) {
    let stream_ctx = tasty_ipc::stream_hub::StreamContext {
        hub: app.stream_hub.clone(),
        inbound_tx: app.stream_inbound_tx.clone(),
        waker: waker.stream_waker(),
    };
    let connections = app.core.connections().clone();
    if let Some(injector) = app
        .hub
        .start_ipc(waker.ipc_waker(), stream_ctx, connections)
    {
        // 웹훅이 참조할 기본 훅 핸들러를 먼저 등록한다.
        crate::hook_handler::install_default_sources();
        // 완료 전략의 notify_via 검증이 훅 핸들러를 참조한다.
        crate::completion_strategy::install_default_sources();
        // 헤드리스에는 toast가 없어 초기화 실패는 함수 내부 경고 로그로만 알린다.
        let _ = crate::webhook::init_from_config(injector.clone());
        app.core.set_host_ipc_injector(injector);
    }
}

#[cfg(not(feature = "gui"))]
fn bootstrap_engine(
    app: &mut crate::app::App,
    boot_settings: &crate::settings::Settings,
    waker: &crate::adapters::production::headless_waker::HeadlessWaker,
) -> anyhow::Result<crate::core::CoreState> {
    let factory = waker.waker_factory();
    let base_waker = factory.make_default_waker();
    // 부팅 때 한 번 전체 슬롯을 기준으로 마이그레이션·scrollback GC를 수행한다.
    crate::core::layout_persistence::migrate_and_gc_on_boot(boot_settings.general.restore_layout);
    if let Some(notice) = layout_persistence_notice(boot_settings.general.restore_layout) {
        tracing::warn!("{notice}");
    }
    let mut engine =
        // 헤드리스는 슬롯을 점유하지 않으며 레이아웃을 저장·복원하지 않는다.
        crate::core::CoreState::new_with_ids(80, 24, base_waker, None, None, app.core.memory_arc())?;
    engine.waker_factory = Some(factory);
    // 이전 실행의 agent 상태를 정리하되 작업을 자동 재시작하지는 않는다.
    app.core.purge_stale_agent_state_on_boot(&engine);
    app.core.inject_agent_runner_registry(&engine);
    // force-detach 통지에 IPC 서버와 같은 스트림 허브를 사용한다.
    engine.attach.set_notifier(app.stream_hub.clone());
    Ok(engine)
}

/// 기본으로 켜진 레이아웃 복원 설정이 헤드리스에서는 적용되지 않음을 알린다.
#[cfg(not(feature = "gui"))]
fn layout_persistence_notice(restore_layout: bool) -> Option<&'static str> {
    restore_layout.then_some(
        "general.restore_layout is on, but a headless build does not save or restore layouts: \
         workspaces last for this process only (system.info reports layout_slot: null)",
    )
}

#[cfg(not(feature = "gui"))]
enum Wait {
    Event(crate::AppEvent),
    Deadline,
    Disconnected,
}

/// 별도 타이머 스레드 없이 다음 허브 기한까지 recv_timeout으로 기다린다.
#[cfg(not(feature = "gui"))]
fn wait_for_event(
    rx: &std::sync::mpsc::Receiver<crate::AppEvent>,
    deadline: Option<std::time::Instant>,
) -> Wait {
    match deadline {
        Some(at) => {
            match rx.recv_timeout(at.saturating_duration_since(std::time::Instant::now())) {
                Ok(ev) => Wait::Event(ev),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Wait::Deadline,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Wait::Disconnected,
            }
        }
        // 타이머가 없으면 이벤트 수신까지 기다린다.
        None => match rx.recv() {
            Ok(ev) => Wait::Event(ev),
            Err(_) => Wait::Disconnected,
        },
    }
}

#[cfg(not(feature = "gui"))]
fn dispatch_headless_event(
    app: &mut crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    waker: &crate::adapters::production::headless_waker::HeadlessWaker,
    event: crate::AppEvent,
) -> std::ops::ControlFlow<()> {
    use crate::AppEvent;
    match event {
        AppEvent::Shutdown | AppEvent::QuitRequested => return std::ops::ControlFlow::Break(()),
        AppEvent::TerminalOutput(id) => handle_terminal_output(app, state, engine, id),
        AppEvent::IpcReady => {
            // 회차 도중 새 명령이 다음 깨움을 예약할 수 있도록 표지를 먼저 푼다.
            waker.note_ipc_drained();
            let flow = headless_dispatch::pump_ipc(app, state, engine);
            // 예산에서 남긴 명령은 깨움이 이미 합쳐졌을 수 있어 채널 뒤에 다시 예약한다.
            rewake_if_left(flow, || ipc_commands_left(&app.core), || waker.wake_ipc());
            return flow;
        }
        AppEvent::StreamReady => headless_stream::handle_stream_ready(app, state, engine),
    }
    std::ops::ControlFlow::Continue(())
}

#[cfg(not(feature = "gui"))]
fn rewake_if_left(
    flow: std::ops::ControlFlow<()>,
    commands_left: impl FnOnce() -> bool,
    rewake: impl FnOnce(),
) {
    if flow.is_continue() && commands_left() {
        rewake();
    }
}

/// 아직 꺼내지 않은 명령이 있는지 확인한다. 입장 장부가 없으면 false다.
#[cfg(not(feature = "gui"))]
fn ipc_commands_left(core: &crate::core::Core) -> bool {
    core.host_ipc_injector
        .get()
        .and_then(|injector| injector.admission())
        .is_some_and(|ledger| ledger.snapshot().queued_commands > 0)
}

/// gui feature 없는 빌드의 호스트 루프. IPC·PTY·플러그인 이벤트와 타이머를 처리한다.
#[cfg(not(feature = "gui"))]
fn run_headless(cli: cli::Cli) -> anyhow::Result<()> {
    use std::sync::mpsc;

    use crate::adapters::production::headless_waker::HeadlessWaker;
    use crate::app::App;

    locale::init();

    let (tx, rx) = mpsc::channel::<crate::AppEvent>();
    let waker = HeadlessWaker::new(tx);

    let boot_settings = crate::settings::Settings::load();
    let memory_arc = boot_memory(&boot_settings);

    let mut app = App::new_headless(cli.port_file, memory_arc)?;
    start_ipc_and_seed(&mut app, &waker);

    let mut engine = bootstrap_engine(&mut app, &boot_settings, &waker)?;
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

    // 처음 만든 홈에도 namespace 선언이 있어야 하므로 번들은 부팅 때 설치한다. 프로세스 시작은 지연한다.
    headless_plugins::ensure_plugin_manager_metadata(&mut app, &engine);
    if let Some(mgr) = app.plugin_manager.as_mut() {
        crate::plugin::install_builtins_if_needed(mgr);
        // 권한 판정과 매니저가 같은 namespace 표를 공유하며 refresh_packages가 내용을 채운다.
        mgr.install_namespace_table_once();
        mgr.refresh_packages();
    }

    tracing::info!("headless daemon ready; PTY pump + IPC dispatch active");

    loop {
        crate::intent::headless::drain_pending_intents(&mut app.core, &mut state, &mut engine);
        crate::intent::headless::drain_pending_host_events(&app.core, &mut state, &engine);
        // 대기 전에 agent 이벤트를 발행한다. 대기 중 새 항목이 쌓이면 다음 루프에서 전달한다.
        {
            let mut agent_events = Vec::new();
            let mut dropped = 0u64;
            crate::app::agent_events::take_from(
                &engine.agent_event_queue,
                &mut agent_events,
                &mut dropped,
            );
            crate::app::agent_events::emit(app.plugin_manager.as_mut(), agent_events, dropped);
        }
        let deadline = crate::app::timers::min_deadline(
            app.timers.next_deadline(),
            app.plugin_manager.as_ref().and_then(|m| m.next_deadline()),
        );
        let pending = match wait_for_event(&rx, deadline) {
            Wait::Event(ev) => Some(ev),
            Wait::Deadline => None,
            Wait::Disconnected => break,
        };

        run_due_timers(&mut app, &mut state, &mut engine);

        let Some(event) = pending else {
            continue;
        };
        if dispatch_headless_event(&mut app, &mut state, &mut engine, &waker, event).is_break() {
            break;
        }
    }
    Ok(())
}

#[cfg(all(test, not(feature = "gui")))]
mod tests {
    use super::{layout_persistence_notice, rewake_if_left};
    use std::ops::ControlFlow;

    #[test]
    fn a_restore_layout_setting_is_announced_as_ignored() {
        let notice = layout_persistence_notice(true).expect("켜진 설정은 알려야 한다");
        assert!(notice.contains("restore_layout") && notice.contains("headless"));
        assert!(
            notice.contains("layout_slot: null"),
            "system.info에서 확인할 필드도 안내해야 한다"
        );
        assert_eq!(layout_persistence_notice(false), None);
    }

    #[test]
    fn a_round_that_left_commands_wakes_the_loop_again() {
        let mut woke = 0;
        rewake_if_left(ControlFlow::Continue(()), || true, || woke += 1);
        assert_eq!(woke, 1);
    }

    #[test]
    fn an_empty_or_final_round_does_not_wake_the_loop() {
        let mut woke = 0;
        rewake_if_left(ControlFlow::Continue(()), || false, || woke += 1);
        rewake_if_left(ControlFlow::Break(()), || true, || woke += 1);
        assert_eq!(woke, 0);
    }
}
