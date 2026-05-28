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

pub(crate) mod busy_tick;
pub(crate) mod cli_routing;
pub(crate) mod event_loop;
pub(crate) mod locale;
pub(crate) mod os;
pub(crate) mod waker;
pub(crate) mod wiring;

use crate::{App, cli, clipboard, hooks};

pub(crate) fn run() -> anyhow::Result<()> {
    os::attach_windows_console_if_needed();
    os::init_crash_report();

    match cli_routing::parse_or_route()? {
        cli_routing::Routed::AlreadyHandled => Ok(()),
        cli_routing::Routed::Subcommand(cmd) => run_subcommand(cmd),
        cli_routing::Routed::AugmentedHelp => run_augmented_help(),
        cli_routing::Routed::Gui(cli) => run_gui(cli),
    }
}

/// `cli.command.is_some()` — i18n 후 client mode 진입.
fn run_subcommand(cmd: cli::Commands) -> anyhow::Result<()> {
    locale::init();
    cli::run_client(cmd)
}

/// `TASTY_SURFACE_ID` + `!cli.launch` — i18n 후 augmented help 출력.
fn run_augmented_help() -> anyhow::Result<()> {
    locale::init();
    cli::print_augmented_help()
}

/// 본 GUI 부트.
fn run_gui(cli: cli::Cli) -> anyhow::Result<()> {
    locale::init();

    let (event_loop, proxy) = event_loop::build()?;
    os::install_macos_delegate(&proxy);

    clipboard::poll_thread::spawn(proxy.clone());
    busy_tick::spawn(proxy.clone());

    // CWD는 OSC 7 시퀀스에만 의존한다. 모든 플랫폼 공통.
    // zsh/fish는 기본 지원, bash는 PROMPT_COMMAND 설정 필요.

    // Phase D.3.C.M.1 — Settings 와 Memory store 를 App 생성 *이전* 에 초기화.
    // Core 가 처음부터 실 Memory store 의 Arc 를 보유하기 위함. window_lifecycle
    // 의 init_with_config 호출은 idempotent 라 그대로 두어도 no-op.
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
    if let Err(e) = tasty_memory::init_with_config(memory_config) {
        tracing::warn!("memory.db init at boot failed: {e}");
    }
    let memory_arc = tasty_memory::store_arc();

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
