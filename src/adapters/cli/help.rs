//! `-a/--all` 트리 인쇄, 파싱 에러 포맷, `print_augmented_help` 등 도움말 헬퍼.

use anyhow::Result;

use super::Cli;
use super::dynamic;

struct ArgInfo {
    name: String,
    flag: Option<String>,
    help: String,
    required: bool,
}

impl ArgInfo {
    /// Compact form: `<NAME>`, `--flag <NAME>`, `[--flag <NAME>]`
    fn compact(&self) -> String {
        match &self.flag {
            None => {
                if self.required {
                    format!("<{}>", self.name)
                } else {
                    format!("[{}]", self.name)
                }
            }
            Some(f) => {
                if self.required {
                    format!("{} <{}>", f, self.name)
                } else {
                    format!("[{} <{}>]", f, self.name)
                }
            }
        }
    }

    /// Detail form for error messages: `  --flag <NAME>   Help text`
    fn detail(&self) -> String {
        match &self.flag {
            None => format!("  <{}>          {}", self.name, self.help),
            Some(f) => {
                if self.required {
                    format!("  {} <{}>   {}", f, self.name, self.help)
                } else {
                    format!("  [{} <{}>] {}", f, self.name, self.help)
                }
            }
        }
    }
}

/// Extract visible arguments from a clap Command (filtering out help/version).
fn visible_args(cmd: &clap::Command) -> Vec<ArgInfo> {
    cmd.get_arguments()
        .filter(|a| a.get_id() != "help" && a.get_id() != "version")
        .map(|a| ArgInfo {
            name: a.get_id().to_string().to_uppercase(),
            flag: a
                .get_long()
                .map(|l| format!("--{}", l))
                .or_else(|| a.get_short().map(|s| format!("-{}", s))),
            help: a.get_help().map(|s| s.to_string()).unwrap_or_default(),
            required: a.is_required_set(),
        })
        .collect()
}

/// Extract visible subcommands (filtering out "help").
fn visible_subcommands(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|s| s.get_name() != "help")
        .collect()
}

/// Compact usage string: `<TEXT> [--surface <SURFACE>]`
fn format_args(cmd: &clap::Command) -> String {
    visible_args(cmd)
        .iter()
        .map(|a| a.compact())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve the deepest matched command from raw CLI args.
fn resolve_command_path() -> (clap::Command, String) {
    use clap::CommandFactory;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Cli::command();
    let mut current = root.clone();
    let mut matched_path: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') {
            break;
        }
        let found = current
            .get_subcommands()
            .find(|s| s.get_name() == arg.as_str());
        if let Some(sub) = found {
            matched_path.push(arg.clone());
            current = sub.clone();
        } else {
            break;
        }
    }

    let path = if matched_path.is_empty() {
        "tasty".to_string()
    } else {
        format!("tasty {}", matched_path.join(" "))
    };
    (current, path)
}

// ── Public entry points ──

/// Print all commands in a tree structure (2 levels deep) with usage details.
pub fn print_command_tree() {
    use clap::CommandFactory;

    let cmd = Cli::command();
    println!("{} {}", cmd.get_name(), cmd.get_version().unwrap_or(""));
    println!(
        "{}",
        cmd.get_about().map(|s| s.to_string()).unwrap_or_default()
    );
    println!();

    // `_connector`는 leaf print_node에서는 쓰지 않지만, 시그니처를 재귀 호출 측과
    // 동일하게 유지하기 위해 받기만 한다 (caller가 자식 노드 prefix 조립에 사용).
    fn print_node(cmd: &clap::Command, prefix: &str, _connector: &str) {
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        let args = format_args(cmd);
        if args.is_empty() {
            println!("{}{} — {}", prefix, cmd.get_name(), about);
        } else {
            println!("{}{} {} — {}", prefix, cmd.get_name(), args, about);
        }
    }

    let subs: Vec<_> = visible_subcommands(&cmd);
    let count = subs.len();
    for (i, sub) in subs.iter().enumerate() {
        let is_last = i == count - 1;
        let prefix = if is_last { "└── " } else { "├── " };
        let connector = if is_last { "    " } else { "│   " };

        let children = visible_subcommands(sub);
        if children.is_empty() {
            print_node(sub, prefix, connector);
        } else {
            let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
            println!("{}{} — {}", prefix, sub.get_name(), about);
            let child_count = children.len();
            for (j, child) in children.iter().enumerate() {
                let child_is_last = j == child_count - 1;
                let child_prefix = if child_is_last {
                    "└── "
                } else {
                    "├── "
                };
                print_node(child, &format!("{}{}", connector, child_prefix), connector);
            }
        }
    }
}

/// Format a contextual error message for a failed parse.
pub fn format_parse_error(err: clap::Error) {
    use clap::error::ErrorKind;

    match err.kind() {
        ErrorKind::MissingRequiredArgument
        | ErrorKind::InvalidValue
        | ErrorKind::UnknownArgument
        | ErrorKind::InvalidSubcommand => {
            let (current, cmd_path) = resolve_command_path();
            let children = visible_subcommands(&current);

            eprintln!("{}", err);

            if !children.is_empty() {
                eprintln!("Available subcommands for '{}':", cmd_path);
                for sub in &children {
                    let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
                    let args = format_args(sub);
                    if args.is_empty() {
                        eprintln!("  {} {:16} {}", cmd_path, sub.get_name(), about);
                    } else {
                        eprintln!("  {} {} {} — {}", cmd_path, sub.get_name(), args, about);
                    }
                }
            } else {
                let args = visible_args(&current);
                let required: Vec<_> = args.iter().filter(|a| a.required).collect();
                let optional: Vec<_> = args.iter().filter(|a| !a.required).collect();

                if !required.is_empty() {
                    eprintln!("Required arguments for '{}':", cmd_path);
                    for arg in &required {
                        eprintln!("{}", arg.detail());
                    }
                }
                if !optional.is_empty() {
                    eprintln!("Optional:");
                    for arg in &optional {
                        eprintln!("{}", arg.detail());
                    }
                }
            }
            eprintln!();
            eprintln!("Run '{} --help' for full details.", cmd_path);
        }
        _ => {
            err.exit();
        }
    }
    std::process::exit(2);
}

/// plugin contributes.cli가 합쳐진 도움말 출력. plugin 디스커버리에 실패해도
/// 정적 CLI 도움말은 항상 보장한다.
pub fn print_augmented_help() -> Result<()> {
    use clap::CommandFactory;
    let entries = match crate::plugin::plugin_root() {
        Some(root) => dynamic::discover_plugin_clis(&root),
        None => Vec::new(),
    };
    let mut cmd = if entries.is_empty() {
        Cli::command()
    } else {
        dynamic::build_augmented_cli(&entries)
    };
    cmd.print_help()?;
    println!();
    Ok(())
}
