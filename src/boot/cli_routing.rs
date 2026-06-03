//! `fn main` 진입 직후의 CLI 라우팅 결정.
//!
//! `-a/--all` / clap parse / plugin CLI fallback / subcommand / augmented help
//! 5가지 분기를 `Routed` enum 으로 압축. i18n 은 호출자가 mode helper 안에서 한다.

use crate::cli;

/// CLI 라우팅 결과. `parse_or_route` 가 반환한다.
pub(crate) enum Routed {
    /// `cli_routing` 안에서 이미 출력/실행됨. 호출자는 즉시 Ok(()) 반환.
    /// - `-a/--all` → `cli::print_command_tree` (i18n 무관, clap get_about println)
    /// - clap parse 에러 → `cli::format_parse_error` 내부 std::process::exit (unreachable)
    /// - plugin CLI 매칭 → `cli::try_run_plugin_cli` 실행 완료 (에러는 Result 채널로 propagate)
    AlreadyHandled,
    /// `cli.command.is_some()` — client mode 진입 (i18n 후 `cli::run_client`).
    Subcommand(cli::Commands),
    /// `TASTY_SURFACE_ID` + `!cli.launch` — augmented help (i18n 후 `cli::print_augmented_help`).
    AugmentedHelp,
    /// 본 GUI. 호출자가 event loop / app 생성.
    Gui(cli::Cli),
}

/// 모든 라우팅 결정을 한 곳에 모은다. plugin CLI 실행 에러는 Result 로 전파.
pub(crate) fn parse_or_route() -> anyhow::Result<Routed> {
    use clap::Parser;

    // -a/--all 은 clap parse 우회 — clap 의 -h 가 먼저 exit 하므로 args 를 직접 본다.
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "-a" || a == "--all") {
            cli::print_command_tree();
            return Ok(Routed::AlreadyHandled);
        }
    }

    // 정적 Cli 파싱. InvalidSubcommand 시 plugin CLI 동적 등록에서 한 번 더 매칭 시도.
    // 정적이 항상 우선이므로 plugin 이 호스트 명령을 가릴 수 없다.
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            if matches!(err.kind(), clap::error::ErrorKind::InvalidSubcommand)
                && let Some(result) = cli::try_run_plugin_cli()
            {
                result?; // plugin 실행 에러는 그대로 main 까지 propagate
                return Ok(Routed::AlreadyHandled);
            }
            cli::format_parse_error(err); // 내부 process::exit
            unreachable!();
        }
    };

    if let Some(command) = cli.command {
        return Ok(Routed::Subcommand(command));
    }
    if !cli.launch && std::env::var("TASTY_SURFACE_ID").is_ok() {
        return Ok(Routed::AugmentedHelp);
    }
    Ok(Routed::Gui(cli))
}
