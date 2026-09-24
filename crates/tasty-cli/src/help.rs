//! `-a/--all` 트리 인쇄, 파싱 에러 포맷, `print_augmented_help` 등 도움말 헬퍼.

use anyhow::Result;

use super::dynamic;
use crate::out::outln;

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = match tasty_host_plugin::plugin_root() {
        Some(root) => dynamic::build_augmented_cli(&dynamic::discover_plugin_clis(&root)),
        None => crate::help_i18n::command(),
    };
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

/// Print the host and plugin command tree with usage details.
/// The caller supplies the application version; this library has its own version.
/// A closed stdout pipe is treated as normal completion.
pub fn print_command_tree(version: &str) -> Result<()> {
    crate::out::quiet_if_stdout_closed(print_command_tree_inner(version))
}

fn print_command_tree_inner(version: &str) -> Result<()> {
    let entries = match tasty_host_plugin::plugin_root() {
        Some(root) => dynamic::discover_plugin_clis(&root),
        None => Vec::new(),
    };
    let cmd = if entries.is_empty() {
        crate::help_i18n::command()
    } else {
        dynamic::build_augmented_cli(&entries)
    };
    outln!("{} {}", cmd.get_name(), version)?;
    outln!(
        "{}",
        cmd.get_about().map(|s| s.to_string()).unwrap_or_default()
    )?;
    outln!()?;

    fn print_node(cmd: &clap::Command, prefix: &str, _connector: &str) -> Result<()> {
        let about = cmd.get_about().map(|s| s.to_string()).unwrap_or_default();
        let args = format_args(cmd);
        if args.is_empty() {
            outln!("{}{} — {}", prefix, cmd.get_name(), about)
        } else {
            outln!("{}{} {} — {}", prefix, cmd.get_name(), args, about)
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
            print_node(sub, prefix, connector)?;
        } else {
            let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
            outln!("{}{} — {}", prefix, sub.get_name(), about)?;
            let child_count = children.len();
            for (j, child) in children.iter().enumerate() {
                let child_is_last = j == child_count - 1;
                let child_prefix = if child_is_last {
                    "└── "
                } else {
                    "├── "
                };
                print_node(child, &format!("{}{}", connector, child_prefix), connector)?;
            }
        }
    }
    Ok(())
}

/// Format a contextual error message for a failed parse.
pub fn format_parse_error(err: clap::Error) {
    use clap::error::ErrorKind;

    match err.kind() {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        | ErrorKind::DisplayVersion => err.exit(),
        _ => {
            let (current, cmd_path) = resolve_command_path();
            let children = visible_subcommands(&current);

            crate::out::errln!("{}", crate::help_error::render(&err));

            if !children.is_empty() {
                crate::out::errln!(
                    "{}",
                    tasty_i18n::t_fmt("cli.help_frame.available", &cmd_path)
                );
                for sub in &children {
                    let about = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
                    let args = format_args(sub);
                    if args.is_empty() {
                        crate::out::errln!("  {} {:16} {}", cmd_path, sub.get_name(), about);
                    } else {
                        crate::out::errln!(
                            "  {} {} {} — {}",
                            cmd_path,
                            sub.get_name(),
                            args,
                            about
                        );
                    }
                }
            } else {
                let args = visible_args(&current);
                let required: Vec<_> = args.iter().filter(|a| a.required).collect();
                let optional: Vec<_> = args.iter().filter(|a| !a.required).collect();

                if !required.is_empty() {
                    crate::out::errln!(
                        "{}",
                        tasty_i18n::t_fmt("cli.help_frame.required", &cmd_path)
                    );
                    for arg in &required {
                        crate::out::errln!("{}", arg.detail());
                    }
                }
                if !optional.is_empty() {
                    crate::out::errln!("{}", tasty_i18n::t("cli.help_frame.optional"));
                    for arg in &optional {
                        crate::out::errln!("{}", arg.detail());
                    }
                }
            }
            crate::out::errln!();
            crate::out::errln!("{}", tasty_i18n::t_fmt("cli.help_frame.details", &cmd_path));
        }
    }
    std::process::exit(2);
}

/// 플러그인을 포함한 도움말을 출력한다. discovery 실패 시 정적 도움말은 유지한다.
/// stdout의 BrokenPipe는 정상 종료로 처리한다.
pub fn print_augmented_help() -> Result<()> {
    crate::out::quiet_if_stdout_closed(print_augmented_help_inner())
}

fn print_augmented_help_inner() -> Result<()> {
    let entries = match tasty_host_plugin::plugin_root() {
        Some(root) => dynamic::discover_plugin_clis(&root),
        None => Vec::new(),
    };
    let mut cmd = if entries.is_empty() {
        crate::help_i18n::command()
    } else {
        dynamic::build_augmented_cli(&entries)
    };
    crate::out::from_io(cmd.print_help())?;
    outln!()?;
    Ok(())
}
