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
use crate::{App, clipboard};
use crate::{cli, hooks};

pub(crate) fn run() -> anyhow::Result<()> {
    os::attach_windows_console_if_needed();
    os::init_crash_report();

    match cli_routing::parse_or_route()? {
        cli_routing::Routed::AlreadyHandled => Ok(()),
        cli_routing::Routed::Subcommand(cmd, port_file) => run_subcommand(cmd, port_file),
        cli_routing::Routed::AugmentedHelp => run_augmented_help(),
        cli_routing::Routed::Gui(cli) => {
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

    clipboard::poll_thread::spawn(proxy.clone());
    busy_tick::spawn(proxy.clone());

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
        Ok(arc) => Some(arc),
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
        Ok(arc) => Some(arc),
        Err(e) => {
            tracing::warn!("memory.db init at boot failed: {e}");
            None
        }
    };

    let mut app = App::new_headless(waker.terminal_waker(), cli.port_file, memory_arc)?;
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
        "tasty.startup.post",
        &serde_json::Value::Null,
    );

    tracing::info!("headless daemon ready; PTY pump + IPC dispatch active");

    while let Ok(event) = rx.recv() {
        match event {
            AppEvent::Shutdown | AppEvent::QuitRequested => break,
            AppEvent::TerminalOutput(_id) => {
                // 단일 engine 전체 drain — reader 채널을 비워 블록을 방지하고
                // termwiz 파싱 + observer/command_index/OSC52 부수효과를 적용한다.
                // 반환 CoreEvent (Notification/Bell/Title/Cwd/Exit) 는 cascade 주체
                // (view/plugin)가 없으므로 단계 0 에선 무시한다.
                let _ = app.core.process_all_pty_output(&mut engine);
            }
            AppEvent::IpcReady => {
                headless_dispatch::pump_ipc(&mut app, &mut state, &mut engine);
            }
            AppEvent::StreamReady => {
                // 스트림 클라가 보낸 inbound 프레임 drain. debug 빌드는 echo 회신,
                // release 는 drop (실제 소비자는 단계 4+). 끊긴 client 의 attach lock 은
                // 자동 free 환원(단계 3).
                let disconnected = app.stream_hub.pump_inbound(&app.stream_inbound_rx);
                for client_id in disconnected {
                    engine.attach.release_all_for_client(client_id);
                }
            }
            AppEvent::BusyPoll => {
                // 단계 0 범위 밖 — busy indicator 미구현.
            }
        }
    }
    Ok(())
}
