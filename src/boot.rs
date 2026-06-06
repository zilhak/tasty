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
/// 4. `hub.start_ipc(ipc_waker)` — accept 스레드 분리
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
    if let Some(injector) = app.hub.start_ipc(waker.ipc_waker()) {
        app.core.set_host_ipc_injector(injector);
    }

    hooks::lua::fire(
        app.lua_engine.as_ref(),
        "tasty.startup.post",
        &serde_json::Value::Null,
    );

    while let Ok(event) = rx.recv() {
        match event {
            AppEvent::Shutdown | AppEvent::QuitRequested => break,
            AppEvent::TerminalOutput(_id) => {
                // 후속 작업: app.on_terminal_output_headless(_id)
                // 현재는 PTY reader 가 직접 scrollback_store 에 기록.
            }
            AppEvent::IpcReady => {
                // 후속 작업: IPC pending queue dispatch via handler.
            }
            AppEvent::BusyPoll => {
                // 후속 작업: busy_tick 평가 — headless tick 미구현.
            }
        }
    }
    Ok(())
}
