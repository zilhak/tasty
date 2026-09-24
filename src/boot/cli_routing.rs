//! CLI 인자를 해석해 명령 실행·도움말·호스트 시작을 선택한다.

use crate::cli;

pub(crate) enum Routed {
    /// 이미 도움말을 출력했거나 플러그인 명령을 실행했다.
    AlreadyHandled,
    /// 명령, 선택한 port-file, 요청 timeout 설정.
    Subcommand(cli::Commands, Option<String>, cli::Envelope),
    AugmentedHelp,
    /// 호스트 실행. GUI·헤드리스 선택은 호출자가 빌드 feature로 판단한다.
    Gui(cli::Cli),
}

pub(crate) fn parse_or_route() -> anyhow::Result<Routed> {
    use clap::FromArgMatches;

    // 플러그인 인자 오류·매니페스트 경고도 번역을 사용하므로 라우팅 전에 초기화한다.
    super::locale::init();

    // clap의 정적 도움말에 없는 플러그인 명령도 포함하도록 이 도움말 요청은 직접 처리한다.
    {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "-a" || a == "--all") {
            cli::print_command_tree(env!("CARGO_PKG_VERSION"))?;
            return Ok(Routed::AlreadyHandled);
        }
        // 첫 인자가 아닌 하위 명령의 --help는 clap에 맡긴다.
        if matches!(args.get(1).map(String::as_str), Some("-h") | Some("--help")) {
            cli::print_augmented_help()?;
            return Ok(Routed::AlreadyHandled);
        }
    }

    // tasty-cli 라이브러리 버전 대신 실행 바이너리의 버전을 표시한다.
    let cmd = cli::localized_command().version(env!("CARGO_PKG_VERSION"));

    // 호스트 명령을 먼저 해석하고 InvalidSubcommand일 때만 플러그인을 찾는다.
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
                result?;
                return Ok(Routed::AlreadyHandled);
            }
            cli::format_parse_error(err);
            unreachable!();
        }
    };

    if let Some(command) = cli.command {
        let envelope = cli::Envelope {
            response_timeout_ms: cli.response_timeout_ms,
        };
        return Ok(Routed::Subcommand(command, cli.port_file, envelope));
    }
    // 보낼 IPC 요청이 없는 도움말·호스트 시작에서는 요청 timeout 옵션을 거절한다.
    cli::Envelope {
        response_timeout_ms: cli.response_timeout_ms,
    }
    .refuse_if_set();
    if !cli.launch && std::env::var("TASTY_SURFACE_ID").is_ok() {
        return Ok(Routed::AugmentedHelp);
    }
    Ok(Routed::Gui(cli))
}
