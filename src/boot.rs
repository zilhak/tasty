//! `fn main` 부팅 시퀀스 오케스트레이션.
//!
//! `run()` 이 단일 진입점. 내부 단계 순서:
//!
//! 1. OS 보정 (Windows console attach, crash_report::init — panic hook + stderr tracing)
//! 2. CLI 라우팅 결정 (`cli_routing::parse_or_route`)
//! 3. 결정에 따라 mode helper 호출:
//!    - `AlreadyHandled` → Ok(())
//!    - `Subcommand` → i18n init + `cli::run_client`
//!    - `AugmentedHelp` → i18n init + `cli::print_augmented_help`
//!    - `Gui` → 공유 로그 파일 개방(`os::enable_host_file_log`) + i18n init +
//!      event loop / background threads / App / event_loop.run_app
//!      (gui 빌드 + `!cli.headless`) — 또는 `run_headless` (headless 빌드 / `--headless`)

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

/// 대량 삭제 직후에만 압축(freelist 가 클 때) — `pruned == 0` 이면 평소 부팅처럼 no-op.
fn vacuum_if_needed(store: &mut tasty_memory::MemoryStore, pruned: u64) {
    if pruned == 0 {
        return;
    }
    tracing::info!("boot memory maintenance: pruned {pruned} stale log rows");
    log_vacuum_result(store.vacuum_if_fragmented(10_000));
}

/// 이미 비대해진 WAL 회수 — `journal_size_limit`(`tasty_memory::WAL_SIZE_LIMIT_BYTES`)
/// 은 앞으로 커지는 것만 막으므로, 기존 인스턴스의 큰 WAL 은 되감기를 한 번
/// 강제해야 줄어든다.
///
/// **VACUUM 뒤에** 부른다: VACUUM 은 DB 전체를 다시 쓰므로 그 자체로 WAL 을 크게
/// 부풀린다. 순서를 뒤집으면 잘라낸 직후 다시 커진 채로 부팅이 끝난다.
fn truncate_wal(store: &mut tasty_memory::MemoryStore) {
    match store.checkpoint_truncate() {
        Ok(true) => {}
        // busy — 다른 커넥션이 읽는 중이라 이번엔 못 줄였다. 다음 부팅에 다시 시도되고
        // 그 사이에도 pragma 가 상한을 지키므로 정보 수준으로만 남긴다.
        Ok(false) => {
            tracing::info!("boot memory maintenance: wal checkpoint was busy; wal left as is")
        }
        Err(e) => tracing::warn!("boot memory maintenance: wal checkpoint failed: {e}"),
    }
}

/// boot 시 1회 memory.db 위생 정리.
///
/// audit/telemetry 는 append-only 로그라 `memory` 테이블을 무한 채운다(per-IPC audit
/// 가 수십만 행 누적). put 은 이제 O(1)(전체 스캔 제거)이라 성능 목적은 아니며, 무한
/// 누적으로 인한 디스크 증가와 1GB regular quota 도달을 막는 retention 이다. 정책은
/// `adapters::ipc::log_retention` 이 소유하고 런타임 append 경로와 공유한다 — 부팅
/// 경로만 있으면 재시작 전까지 무제한으로 자란다(그게 원래 상태였다). 조용히(이벤트
/// 없이) 삭제 후 단편화가 크면 1회 VACUUM 으로 회수하며, 최초 1회만 대량(수십만 행)
/// 삭제로 ~2s 소요될 수 있고 이후 부팅은 초과분만 정리한다.
fn maintain_memory_at_boot(arc: &std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>) {
    // 상한 값은 `adapters::ipc::log_retention` 이 단독으로 소유한다 — 런타임 집행
    // 경로가 같은 테이블을 읽는다. 여기에 숫자를 다시 적으면 두 경로가 갈린다.
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
    for policy in &crate::adapters::ipc::log_retention::ALL {
        pruned += policy.enforce(&mut *store, now_ms);
    }
    vacuum_if_needed(&mut store, pruned);
    truncate_wal(&mut store);
}

pub(crate) fn run() -> anyhow::Result<()> {
    os::attach_windows_console_if_needed();
    os::init_crash_report();

    match cli_routing::parse_or_route()? {
        cli_routing::Routed::AlreadyHandled => Ok(()),
        cli_routing::Routed::Subcommand(cmd, port_file) => run_subcommand(cmd, port_file),
        cli_routing::Routed::AugmentedHelp => run_augmented_help(),
        cli_routing::Routed::Gui(cli) => {
            // 공유 로그 파일은 host 만 연다(= 여기서 연다). CLI 클라이언트도 같은
            // 바이너리라, 역할 판정 전에 열면 CLI 를 한 번 돌릴 때마다 실행 중인
            // host 의 로그가 truncate 된다.
            os::enable_host_file_log();
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
    // 이벤트 루프 stall 관측 시작 — 펌프가 멎으면 이 스레드만 살아남아 로그를 남긴다.
    crate::stall_watchdog::spawn();
    event_loop.run_app(&mut app)?;
    drop_app_with_trace(app);

    Ok(())
}

/// `event_loop.run_app` 반환 후의 **Drop tail** 계측 (S5 계열).
///
/// `app` 을 스코프 끝 암묵 drop 에 맡기면 계측을 끼울 자리가 없어 명시 drop 으로
/// 전환했다(동작은 동일 — drop 시점이 같은 함수의 몇 줄 앞으로 당겨질 뿐이다).
///
/// 이 구간이 중요한 이유: `event_loop.exit()` 시점에 창은 이미 사라졌는데 블로킹
/// destructor(LuaEngine join / PluginProcess wait / SshTunnel wait / PTY kill)가
/// 여기서 직렬로 돈다. 즉 **종료 화면으로 덮을 수 없는 체감 시간**이며,
/// `shutdown_total` 만 재면 통째로 놓친다.
#[cfg(feature = "gui")]
fn drop_app_with_trace(app: App) {
    use std::time::Instant;

    use crate::app::shutdown_trace;

    // Drop tail 내부 세분(S5b/S5c)은 destructor 가 surface·세션 수만큼 반복돼
    // 개별 로그로는 읽기 어렵다. 크레이트별 전역 누적기의 **전후 델타**로 잰다.
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

/// Drop tail 세분 계측(S5b/S5c)의 스냅샷 — 크레이트별 전역 누적기 값.
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

    /// `before` 대비 증가분을 S5b/S5c 로 찍는다.
    fn log_delta(&self, before: &Self) {
        use crate::app::shutdown_trace::duration_ms;

        tracing::info!(
            target: "tasty::shutdown",
            ms = duration_ms(self.pty.0.saturating_sub(before.pty.0)),
            // 필드명이 S3 의 `surfaces` 와 다른 이유: 여기서 세는 건 **PTY 를 실제로
            // 가진 backend** 라 layout 상의 surface 수와 일치하지 않는다(child
            // terminal / headless PTY 는 layout 밖에도 있고, PTY 없는 surface 도 있다).
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

/// 시간축 — 이번 바퀴에 due 한 타이머 키를 전부 실행한다. gui `about_to_wait` 의
/// drain 블록과 동형이며, 각 arm 이 무엇의 headless 등가인지는 arm 주석에 있다.
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
                // 렌더가 없어 로컬 redraw 는 무의미하지만(반환값 무시), attach
                // client 로의 busy forward 는 headless 가 원격 attach 의 주
                // 시나리오라 필수 — gui `app/busy.rs` 의 `poll_busy_states` 와
                // 동형(엔진 1 개라 순회 불필요).
                // StatusBar 브랜치 캐시(`core/state/branch.rs`)는 **의도적으로**
                // 여기에 배선하지 않는다 — headless 는 StatusBar 를 렌더하지 않아
                // 읽는 쪽이 없고(그래서 캐시 자체가 `gui` feature 게이트다), 갱신하면
                // 읽히지도 않을 `.git/HEAD` 를 초당 한 번 여는 것이 된다.
                engine.refresh_busy_surfaces();
                engine.forward_busy_activity(&app.stream_hub);
                // attention forward 도 같은 tick(gui `app/busy.rs` 와 동형).
                // headless 가 원격 attach 의 주 시나리오라 이 배선이 없으면
                // mirror 는 서버 attention 을 영원히 못 받는다.
                engine.forward_attention(&app.stream_hub);
                // 글로벌 훅 — gui `app/global_hooks.rs` 의 `poll_global_hooks` 와
                // 동형(엔진 1 개라 순회 불필요).
                engine.poll_global_hooks();
                // IdleTimeout 훅 — gui `app/idle_hooks.rs` 의
                // `poll_idle_timeout_hooks` 와 동형(엔진 1 개라 순회 불필요).
                // 바인딩 실행 + host event enqueue 는 여기서 직접 한다(엔진
                // 레이어는 순수 조회만 함 — `CoreState::poll_idle_timeout_hooks`).
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
                // plugin 소켓이 조용해도 healthcheck/재시작 타이머가 진행되도록
                // 1Hz 안전망으로 편승(주 wake 경로는 TerminalOutput(None)).
                headless_plugins::pump_plugins(app, state, engine);
            }
            // TTL 정리 3종 — gui `app/sweeps.rs` 와 동형(엔진 1 개라 순회 불필요).
            // 접근 시점 lazy 경로를 대체하지 않고 보완한다
            // (`docs/adr/0050-headless-pty-primitive.md` "좀비 회수 시점").
            // headless 야말로 이 보완이 가장 필요한 실행 형태다 — GUI 조작이
            // 아예 없어 lazy 를 굴릴 사용자 접근 자체가 없다.
            crate::app::timers::Tick::PtySweep => {
                // 반환 id 는 쓰지 않는다 — 두 store 회수까지 공용 함수가 끝냈다.
                let _ = engine.sweep_idle_ptys(Instant::now());
            }
            crate::app::timers::Tick::CaptureSweep => {
                engine.capture_uploads.sweep_expired(Instant::now());
            }
            crate::app::timers::Tick::LogPrune => {
                let now_ms = u64::try_from(app.core.now_unix_millis()).unwrap_or(0);
                app.core.with_memory(|mem| {
                    crate::adapters::ipc::log_retention::maybe_prune(mem, now_ms);
                });
            }
        }
    }
}

/// PTY 출력 wake 처리 — dedup 게이트 해제 후 대상 surface(또는 전체)를 drain 한다.
#[cfg(not(feature = "gui"))]
fn handle_terminal_output(
    app: &mut crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    id: Option<u32>,
) {
    // Early reset: drain 직전에 dedup 게이트를 풀어 경합 wake 유실 방지
    // (research §8). headless 는 단일 engine 이라 순회 불필요.
    if let Some(factory) = engine.waker_factory.as_ref() {
        factory.note_drained(id);
    }
    // Targeted wake 는 해당 surface 만, default wake 는 전체 drain.
    // 반환 CoreEvent 중 소비하는 것은 `TerminalOutputMatch` 뿐이다 — 나머지
    // (Notification/Bell/Title/Cwd/Exit)는 cascade 주체(view/plugin)가 없어
    // 버린다. 직접 부수효과(observer/command_index/OSC52)는 process 함수
    // 내부에서 이미 적용됐다. 하나만 소비하는 근거는
    // `fire_output_match_hooks` 의 doc 참조.
    let outcome = match id {
        Some(sid) => app.core.process_pty_output(engine, sid), // targeted: 해당 surface 만 drain
        None => {
            let outcome = app.core.process_all_pty_output(engine); // default: 전체 drain
            // plugin 프로세스 수신 스레드도 이 default waker 를 공유한다
            // (headless_plugins 모듈 주석 참조) — hello 응답/PaintFrame 등
            // plugin 이벤트도 이 wake 로 도착하므로 여기서 함께 pump.
            headless_plugins::pump_plugins(app, state, engine);
            outcome
        }
    };
    fire_output_match_hooks(app, engine, outcome.events);
}

/// PTY drain 이 돌려준 `CoreEvent` 에서 `output-match` 훅만 골라 발화한다.
///
/// gui 는 `App::cascade_terminal_output_match`(`app/dispatch_domain.rs`)가 하는
/// 일이고, headless 에는 그 cascade 층이 없다(`app/dispatch_domain_stubs.rs`).
/// stub 의 근거는 "cascade 는 View 의 모든 window 에 broadcast 하는 것" 인데
/// **훅 실행은 view 와 무관한 부수효과**라 그 근거가 닿지 않는다 — 같은 stub
/// 파일에 묶여 함께 죽어 있었다. `tasty set hook --event output-match:...` 는
/// CLI 로 노출된 에이전트 기능이므로 headless 에서도 동작해야 한다
/// (`docs/identity.md` 원칙 2).
///
/// **`PendingHostEvent::HookFired` enqueue 는 일부러 하지 않는다.** 그 큐의
/// 배수 주체는 `app/dispatch/host_events.rs` 하나뿐이고 `src/app.rs` 가 그
/// 모듈을 `#[cfg(feature = "gui")]` 로 걸어, headless 에는 빼 가는 쪽이 없다.
///
/// 주의 — **그 큐는 headless 에서 이미 자라고 있다.** 같은 파일의 idle-timeout
/// 경로가 훅 발화마다 넣는데 빼 가는 쪽이 없다(`intent/headless.rs` 모듈 주석도
/// 같은 사실을 적는다). 그러니 여기서 넣지 않는 것은 "증가를 막는" 것이 아니라
/// **증가율을 올리지 않는** 것이다 — 이쪽은 매칭되는 라인마다 1 건이라 상시 구동
/// 데몬의 가장 뜨거운 경로가 된다. 그러면서 관측 가능한 효과는 여전히 0 이다.
/// headless 에 배수 주체가 생기면 그때 idle-timeout 배선과 함께 다시 본다 —
/// 그 조건이 성립하면 이 생략은 결함이 되고, 저쪽 누수는 정상 경로가 된다.
#[cfg(not(feature = "gui"))]
fn fire_output_match_hooks(
    app: &crate::app::App,
    engine: &mut crate::core::CoreState,
    events: Vec<crate::core::intent::CoreEvent>,
) {
    let injector = app.core.host_ipc_injector.get().cloned();
    for event in events {
        let crate::core::intent::CoreEvent::TerminalOutputMatch { surface_id, text } = event else {
            continue;
        };
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[tasty_hooks::HookEvent::OutputMatch(text)]);
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

/// 부팅 시 memory.db 초기화 — 설정의 상한/쿼터를 반영하고 유지보수를 1 회 돌린다.
/// 실패해도 데몬은 뜬다(memory 없는 상태로 계속) — 반환 `None` 이 그 상태다.
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
            None
        }
    };
    memory_arc
}

/// IPC accept 스레드를 띄우고, 그 라우터를 필요로 하는 전역 레지스트리를 시드한다.
/// `start_ipc` 가 injector 를 돌려주지 않으면(IPC 미기동) 시드도 하지 않는다 —
/// 시드 대상이 전부 IPC 라우터를 전제하기 때문이다.
#[cfg(not(feature = "gui"))]
fn start_ipc_and_seed(
    app: &mut crate::app::App,
    waker: &crate::adapters::production::headless_waker::HeadlessWaker,
) {
    let stream_ctx = crate::adapters::production::stream_hub::StreamContext {
        hub: app.stream_hub.clone(),
        inbound_tx: app.stream_inbound_tx.clone(),
        waker: waker.stream_waker(),
    };
    if let Some(injector) = app.hub.start_ipc(waker.ipc_waker(), stream_ctx) {
        // 웹훅 리스너 init (headless). start_ipc 이후 = (B)IPC 처리 가능. config 는
        // 아래 CoreState::new_with_ids 에서 로드되지만 리스너는 IPC 라우터만
        // 필요하므로 이 시점 주입으로 충분. headless 엔 init_app_state 가 없어 여기서 호출.
        // headless 는 toast UI 가 없으므로 포트 미설정/bind 실패 경고는 리스너 내부
        // `tracing::warn!` 로만 노출된다(S8) — 반환 report 는 여기서 소비하지 않는다.
        //
        // 공유 훅 핸들러 레지스트리 시드(host embedded 기본값 + user config). 웹훅
        // 바인딩·`hook_handler.*` 조회가 이 전역 레지스트리를 보므로 리스너 init 전에
        // 채운다(plugin contribution 은 이후 discover_and_start 에서 병합).
        crate::hook_handler::install_default_sources();
        // 완료 판정 전략 레지스트리 시드 — 훅 핸들러와 대칭 위치.
        // notify_via 참조 무결성 검증이 훅 핸들러 레지스트리를 보므로 그 뒤에 둔다.
        crate::completion_strategy::install_default_sources();
        // 반환 report 는 여기서 소비하지 않는다 — headless 엔 toast UI 가 없어
        // 포트 미설정/bind 실패는 리스너 내부 `tracing::warn!` 로만 노출된다.
        let _ = crate::webhook::init_from_config(injector.clone());
        app.core.set_host_ipc_injector(injector);
    }
}

/// Engine 부트스트랩 — gui 가 첫 MainView 생성 시 하는 일의 headless 등가.
#[cfg(not(feature = "gui"))]
fn bootstrap_engine(
    app: &mut crate::app::App,
    boot_settings: &crate::settings::Settings,
    waker: &crate::adapters::production::headless_waker::HeadlessWaker,
) -> anyhow::Result<crate::core::CoreState> {
    let factory = waker.waker_factory();
    let base_waker = factory.make_default_waker();
    // gui 의 `begin_boot` 과 같은 부팅 1 회 훅 — 레거시 `layout.json` 마이그레이션 +
    // 전 슬롯 union scrollback GC. `new_with_ids` 가 슬롯을 읽기 전이어야 한다.
    crate::core::layout_persistence::migrate_and_gc_on_boot(boot_settings.general.restore_layout);
    let mut engine =
        // 슬롯 `None` — headless 는 레이아웃 복원을 적용하지 않으므로(위 "0-C")
        // 어떤 슬롯도 점유하지 않고 로드·저장 모두 하지 않는다.
        crate::core::CoreState::new_with_ids(80, 24, base_waker, None, None, app.core.memory_arc())?;
    engine.waker_factory = Some(factory);
    // agent task runner 재시작 정화(결정 2) — 자동 시작은 하지 않는다(결정 1).
    // CoreState 확보 직후, 어떤 client 도 아직 붙기 전에 1 회만 수행.
    app.core.purge_stale_agent_state_on_boot(&engine);
    // 렌더 경로(DAG surface 러너 배지)가 `Core` 없이도 러너 생사를 물을 수
    // 있도록 같은 레지스트리 Arc 를 CoreState 에 심는다(memory 주입과 동형).
    if engine
        .agent_runner_registry
        .set(app.core.agent_runner_registry())
        .is_err()
    {
        tracing::warn!("agent runner registry already injected into CoreState");
    }
    // attach/detach 단계 3: force-detach 통지가 stream client 로 push 되도록 IPC
    // 서버와 동일한 StreamHub 를 attach registry 에 주입.
    engine.attach.set_notifier(app.stream_hub.clone());
    Ok(engine)
}

/// 한 바퀴의 대기 결과.
#[cfg(not(feature = "gui"))]
enum Wait {
    /// 이벤트가 도착했다.
    Event(crate::AppEvent),
    /// 타이머 데드라인에 도달했다 — 이번 바퀴는 타이머만 돌린다.
    Deadline,
    /// 송신단이 전부 사라졌다 — 루프를 끝낸다.
    Disconnected,
}

/// 데드라인 인지 수신 — gui 의 `about_to_wait` 와 대칭이다. gui 는 waker 스레드가
/// 이벤트 루프를 깨우지만, headless 는 메인 루프가 직접 `recv_timeout` 으로 허브
/// 데드라인을 지키므로 wake 신호를 위한 ticker 스레드가 아예 필요 없다.
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
        // 등록된 타이머가 없다 — 깨울 이유가 없으므로 무기한 블로킹.
        None => match rx.recv() {
            Ok(ev) => Wait::Event(ev),
            Err(_) => Wait::Disconnected,
        },
    }
}

/// `RunLuaScript` 처리 — gui event_handler 와 동일. headless 발신원은 현재 없지만
/// (단축키=gui, debug IPC=App 경로) 이벤트 계약상 동작을 미러링한다.
#[cfg(not(feature = "gui"))]
fn run_lua_script(app: &crate::app::App, source: &str, name: &str) {
    if let Some(engine) = app.lua_engine.as_ref() {
        engine.run_script(source, Some(name));
    } else {
        tracing::warn!(target: "tasty_lua", "RunLuaScript dropped — lua engine unavailable");
    }
}

/// 도착한 이벤트 하나를 처리한다. `Break` 면 메인 루프를 끝낸다.
#[cfg(not(feature = "gui"))]
fn dispatch_headless_event(
    app: &mut crate::app::App,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    event: crate::AppEvent,
) -> std::ops::ControlFlow<()> {
    use crate::AppEvent;
    match event {
        AppEvent::Shutdown | AppEvent::QuitRequested => return std::ops::ControlFlow::Break(()),
        AppEvent::TerminalOutput(id) => handle_terminal_output(app, state, engine, id),
        // pump 가 `system.shutdown` 을 받으면 break 를 돌려준다 — 데몬을 멈추는
        // 유일한 IPC 경로다(gui 의 winit proxy 에 대응).
        AppEvent::IpcReady => return headless_dispatch::pump_ipc(app, state, engine),
        AppEvent::StreamReady => headless_stream::handle_stream_ready(app, state, engine),
        AppEvent::RunLuaScript { source, name } => run_lua_script(app, &source, &name),
    }
    std::ops::ControlFlow::Continue(())
}

/// Headless 부트. winit / wgpu / egui 가 없는 빌드 (`--no-default-features`) 전용.
///
/// 시퀀스:
/// 1. `mpsc::channel::<AppEvent>` 생성 + `HeadlessWaker` 로 IPC/PTY waker 발급
/// 2. Settings/Memory store 초기화 (gui 와 동일 정책)
/// 3. `App::new_headless` 로 Core+Hub+plugin_manager 초기화
/// 4. `hub.start_ipc(ipc_waker, stream_ctx)` — accept 스레드 분리 (+ 스트림 승격 경로)
/// 5. 데드라인 인지 수신 loop — 중앙 타이머 허브의 `next_deadline()` 까지만
///    `recv_timeout` 으로 기다리고, 매 바퀴 due 한 타이머 키를 실행한다.
///    Shutdown / QuitRequested 수신 시 break (`docs/dev-guide/timer-hub.md`)
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

    // 번들 plugin 을 **설치**한다 — 띄우지는 않는다. gui 가 창을 만들 때
    // `install_builtins_if_needed` 를 부르는 것과 같은 자리이며, 헤드리스에만 이
    // 호출이 없으면 갓 만든 홈은 package 0 인 채로 남는다(`plugin.list` 가 0 을
    // 답하고 어떤 plugin 메서드도 소속이 안 잡힌다).
    //
    // 예전에는 이 설치가 "호스트가 모르는 이름을 처음 부를 때" 딸려 왔다 — 즉
    // **오타 하나가 plugin 을 설치·기동**했다. 소속 판정을 매니페스트로 옮기면서
    // (ADR-0173) 그 우연한 트리거가 사라졌으므로, 설치는 제 자리인 부팅으로 온다.
    // 기동은 여전히 지연이다: 여기서 프로세스는 하나도 안 뜬다.
    headless_plugins::ensure_plugin_manager_metadata(&mut app, &engine);
    if let Some(mgr) = app.plugin_manager.as_mut() {
        crate::plugin::install_builtins_if_needed(mgr);
        mgr.refresh_packages();
    }

    tracing::info!("headless daemon ready; PTY pump + IPC dispatch active");

    loop {
        // 블로킹 대기에 들어가기 전에 Intent 큐를 비운다. 정상 경로에서는 발화 지점
        // (IPC / plugin 호출)이 이미 응답 전에 drain 하므로 여기서는 비어 있지만,
        // 앞으로 다른 발화점이 생겨도 큐가 프로세스 수명 동안 쌓이지 않게 하는
        // 최종 방어선이다 — `docs/adr/0111-headless-drains-the-intent-queue.md`.
        crate::intent::headless::drain_pending_intents(&mut app.core, &mut state, &mut engine);
        crate::intent::headless::drain_pending_host_events(&app.core, &mut state, &engine);
        // plugin manager 는 자기 허브를 따로 소유한다 — 대기 계산은 min 으로 합성.
        let deadline = crate::app::timers::min_deadline(
            app.timers.next_deadline(),
            app.plugin_manager.as_ref().and_then(|m| m.next_deadline()),
        );
        let pending = match wait_for_event(&rx, deadline) {
            Wait::Event(ev) => Some(ev),
            Wait::Deadline => None,
            Wait::Disconnected => break,
        };

        // 시간축 — due 한 타이머 키 실행. gui `about_to_wait` 의 drain 블록과 동형.
        run_due_timers(&mut app, &mut state, &mut engine);

        let Some(event) = pending else {
            continue;
        };
        if dispatch_headless_event(&mut app, &mut state, &mut engine, event).is_break() {
            break;
        }
    }
    Ok(())
}
