//! `fn main` 진입 직후의 CLI 라우팅 결정.
//!
//! `-a/--all` / clap parse / plugin CLI fallback / subcommand / augmented help
//! 5가지 분기를 `Routed` enum 으로 압축. i18n 은 `parse_or_route` 진입부에서 1회 올린다.

use crate::cli;

/// CLI 라우팅 결과. `parse_or_route` 가 반환한다.
pub(crate) enum Routed {
    /// `cli_routing` 안에서 이미 출력/실행됨. 호출자는 즉시 Ok(()) 반환.
    /// - `-a/--all` → `cli::print_command_tree` (i18n 무관, clap get_about 출력)
    /// - `-a/--all` → `cli::print_command_tree` (clap get_about println; plugin 매니페스트 경고만 i18n)
    /// - clap parse 에러 → `cli::format_parse_error` 내부 std::process::exit (unreachable)
    /// - plugin CLI 매칭 → `cli::try_run_plugin_cli` 실행 완료 (에러는 Result 채널로 propagate)
    AlreadyHandled,
    /// `cli.command.is_some()` — client mode 진입 (i18n 후 `cli::run_client`).
    /// 두 번째 필드는 전역 `--port-file` 값(없으면 None).
    Subcommand(cli::Commands, Option<String>),
    /// `TASTY_SURFACE_ID` + `!cli.launch` — augmented help (i18n 후 `cli::print_augmented_help`).
    AugmentedHelp,
    /// 본 GUI. 호출자가 event loop / app 생성.
    Gui(cli::Cli),
}

/// 모든 라우팅 결정을 한 곳에 모은다. plugin CLI 실행 에러는 Result 로 전파.
pub(crate) fn parse_or_route() -> anyhow::Result<Routed> {
    use clap::{CommandFactory, FromArgMatches};

    // i18n 은 라우팅 판정보다 먼저 올린다 — 아래의 plugin CLI 매칭(`try_run_plugin_cli`:
    // 매니페스트 경고·인자 오류)과 root `-h` 의 augmented help 가 번역 테이블을 읽는다.
    // 뒤의 `run_subcommand` 등이 다시 부르는 `locale::init()` 은 `Once` 가드라 no-op.
    super::locale::init();

    // -a/--all + -h/--help 는 clap parse 우회 — clap 의 built-in `--help` 가 plugin
    // contributes.cli 를 모르는 정적 `Cli::command()` 위에서 발화해 plugin 명령
    // (claude, codex) 이 도움말에서 누락되는 것을 방지한다. args 를 직접 본다.
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "-a" || a == "--all") {
            cli::print_command_tree(env!("CARGO_PKG_VERSION"))?;
            return Ok(Routed::AlreadyHandled);
        }
        // root-level `--help` / `-h` 만 가로챈다 — `args[1]` 위치 체크로 좁혀
        // 서브커맨드의 `--help` (예: `tasty new --help`) 는 그대로 clap 에 위임.
        if matches!(args.get(1).map(String::as_str), Some("-h") | Some("--help")) {
            cli::print_augmented_help()?;
            return Ok(Routed::AlreadyHandled);
        }
    }

    // tasty-cli 는 라이브러리 crate (CARGO_PKG_VERSION="0.1.0") 라서 clap 기본 `version`
    // 출력이 root 바이너리 버전과 어긋난다. 여기서 root 의 CARGO_PKG_VERSION 으로 override.
    let cmd = cli::localized_command().version(env!("CARGO_PKG_VERSION"));

    // 정적 Cli 파싱. InvalidSubcommand 시 plugin CLI 동적 등록에서 한 번 더 매칭 시도.
    // 정적이 항상 우선이므로 plugin 이 호스트 명령을 가릴 수 없다.
    let cli = match cmd.try_get_matches() {
        Ok(matches) => match cli::Cli::from_arg_matches(&matches) {
            Ok(cli) => cli,
            Err(err) => {
                cli::format_parse_error(err);
                unreachable!();
            }
        },
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
        // `cli.command` 만 부분 이동 — 다른 필드 `cli.port_file` 접근은 허용된다.
        return Ok(Routed::Subcommand(command, cli.port_file));
    }
    if !cli.launch && std::env::var("TASTY_SURFACE_ID").is_ok() {
        return Ok(Routed::AugmentedHelp);
    }
    Ok(Routed::Gui(cli))
}
