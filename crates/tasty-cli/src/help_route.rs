//! 도움말 요청을 argv의 실제 옵션 위치로 판별한다.
//!
//! 루트 옵션의 값(`--port-file -a`)이나 하위 명령 뒤의 데이터(`send text -- --all`)를
//! 도움말 제어로 해석하지 않도록, 루트 명령의 인자 선언을 읽어 토큰을 건너뛴다.

use clap::Command;

/// 루트 옵션 구간을 해석한 결과.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootScan {
    /// 루트 위치에 `-a`/`--all`이 있다.
    pub all: bool,
    /// 루트 옵션 뒤 첫 명령 토큰의 argv 위치. 명령이 없거나 `--` 뒤라면 없다.
    pub command_index: Option<usize>,
}

/// argv[0]은 실행 파일 경로로 보고 건너뛴다.
pub fn scan_root(root: &Command, args: &[String]) -> RootScan {
    let mut all = false;
    let mut i = 1;
    while i < args.len() {
        let token = args[i].as_str();
        if token == "--" {
            return RootScan {
                all,
                command_index: None,
            };
        }
        if let Some(long) = token.strip_prefix("--") {
            let (name, inline_value) = match long.split_once('=') {
                Some((name, _)) => (name, true),
                None => (long, false),
            };
            all |= name == "all";
            if !inline_value && long_takes_value(root, name) {
                i += 1;
            }
        } else if let Some(shorts) = token.strip_prefix('-').filter(|s| !s.is_empty()) {
            let (has_all, consumes_next) = scan_short_cluster(root, shorts);
            all |= has_all;
            if consumes_next {
                i += 1;
            }
        } else {
            return RootScan {
                all,
                command_index: Some(i),
            };
        }
        i += 1;
    }
    RootScan {
        all,
        command_index: None,
    }
}

/// 짧은 옵션 묶음(`-ah`)을 읽는다. 값을 받는 옵션이 묶음 끝에 있으면 다음 토큰이 그 값이다.
fn scan_short_cluster(root: &Command, shorts: &str) -> (bool, bool) {
    let mut all = false;
    let mut chars = shorts.chars().peekable();
    while let Some(c) = chars.next() {
        all |= c == 'a';
        let takes_value = root
            .get_arguments()
            .find(|a| a.get_short() == Some(c))
            .is_some_and(|a| a.get_action().takes_values());
        if takes_value {
            // 나머지 문자는 이 옵션의 값이다.
            return (all, chars.peek().is_none());
        }
    }
    (all, false)
}

fn long_takes_value(root: &Command, name: &str) -> bool {
    root.get_arguments()
        .find(|a| a.get_long() == Some(name))
        .is_some_and(|a| a.get_action().takes_values())
}

/// 명령 경로 토큰. `help <경로>`는 `<경로>`와 같은 대상을 가리키므로 앞의 `help`를 뺀다.
pub fn command_path_tokens<'a>(root: &Command, args: &'a [String]) -> &'a [String] {
    let Some(start) = scan_root(root, args).command_index else {
        return &[];
    };
    let tokens = &args[start..];
    match tokens.first() {
        Some(first) if first == "help" && !has_real_help_command() => &tokens[1..],
        _ => tokens,
    }
}

/// clap이 만드는 help 하위 명령이 아닌 실제 `help` 명령이 선언돼 있는지 확인한다.
/// 호출자가 넘기는 트리는 en이 아닌 언어에서 이미 build돼 clap의 help가 들어 있으므로,
/// build 전의 선언 트리로 판정한다. plugin이 `help`라는 이름을 선언하면
/// `dynamic::build::host_command_names`가 예약 이름으로 거부하고 경고하므로 이 트리에 들어오지 않는다.
fn has_real_help_command() -> bool {
    static DECLARED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DECLARED.get_or_init(|| {
        use clap::CommandFactory;
        crate::Cli::command()
            .get_subcommands()
            .any(|s| s.get_name() == "help")
    })
}

/// 루트의 `tasty help`는 `-h`와 같은 짧은 도움말로 보낸다.
/// clap의 help 하위 명령은 긴 도움말을 출력하지만 `help <경로>`는 그대로 둔다.
pub fn normalize_bare_help(root: &Command, args: &[String]) -> Vec<String> {
    let mut out = args.to_vec();
    if let Some(i) = scan_root(root, args).command_index
        && i + 1 == args.len()
        && args[i] == "help"
        && !has_real_help_command()
    {
        out[i] = "-h".to_string();
    }
    out
}

/// 명령 경로의 첫 이름. 플러그인 명령인지 판별할 때 사용한다.
pub fn first_command_name<'a>(root: &Command, args: &'a [String]) -> Option<&'a str> {
    command_path_tokens(root, args)
        .first()
        .map(String::as_str)
        .filter(|t| !t.starts_with('-'))
}

/// 플러그인 트리로 다시 해석해야 하는 clap 오류. 정적 트리가 모르는 이름이거나
/// 도움말 출력 요청이면 플러그인 명령을 포함한 트리가 같은 요청을 다시 판단한다.
pub fn needs_plugin_tree(kind: clap::error::ErrorKind) -> bool {
    use clap::error::ErrorKind;
    matches!(
        kind,
        ErrorKind::InvalidSubcommand
            | ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        std::iter::once("tasty")
            .chain(s.split_whitespace())
            .map(str::to_string)
            .collect()
    }

    fn root() -> Command {
        crate::help_i18n::command()
    }

    #[test]
    fn root_all_is_found_only_in_root_option_position() {
        let r = root();
        assert!(scan_root(&r, &argv("-a")).all);
        assert!(scan_root(&r, &argv("-a -h")).all);
        assert!(scan_root(&r, &argv("-ha")).all);
        assert!(scan_root(&r, &argv("--all")).all);
        assert!(scan_root(&r, &argv("--port-file x --all")).all);
        // 루트 옵션의 값
        assert!(!scan_root(&r, &argv("--port-file -a list surfaces")).all);
        assert!(!scan_root(&r, &argv("--port-file --all list surfaces")).all);
        // 하위 명령 뒤의 데이터와 `--` 뒤의 값
        assert!(!scan_root(&r, &argv("send text -- --all")).all);
        assert!(!scan_root(&r, &argv("send text --all")).all);
        assert!(!scan_root(&r, &argv("-- --all")).all);
    }

    #[test]
    fn command_index_skips_root_options_and_their_values() {
        let r = root();
        assert_eq!(
            scan_root(&r, &argv("--port-file x help codex")).command_index,
            Some(3)
        );
        assert_eq!(
            scan_root(&r, &argv("--port-file=x list")).command_index,
            Some(2)
        );
        assert_eq!(
            scan_root(&r, &argv("--response-timeout-ms 5 --headless list")).command_index,
            Some(4)
        );
        assert_eq!(scan_root(&r, &argv("--help")).command_index, None);
    }

    #[test]
    fn help_prefix_is_dropped_from_the_command_path() {
        let r = root();
        let a = argv("--port-file x help memory goal");
        assert_eq!(command_path_tokens(&r, &a), &a[4..]);
        let a = argv("memory goal --help");
        assert_eq!(command_path_tokens(&r, &a), &a[1..]);
        assert_eq!(
            first_command_name(&r, &argv("help codex spawn")),
            Some("codex")
        );
        assert_eq!(
            first_command_name(&r, &argv("--port-file x codex")),
            Some("codex")
        );
        assert_eq!(first_command_name(&r, &argv("--help")), None);
    }

    #[test]
    fn only_a_bare_root_help_becomes_short_help() {
        let r = root();
        assert_eq!(normalize_bare_help(&r, &argv("help")), argv("-h"));
        assert_eq!(
            normalize_bare_help(&r, &argv("--port-file x help")),
            argv("--port-file x -h")
        );
        assert_eq!(normalize_bare_help(&r, &argv("help new")), argv("help new"));
        // 값 자리의 help는 명령이 아니다.
        assert_eq!(
            normalize_bare_help(&r, &argv("--port-file help")),
            argv("--port-file help")
        );
        assert_eq!(
            normalize_bare_help(&r, &argv("send text help")),
            argv("send text help")
        );
    }
}
