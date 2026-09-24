//! clap 명령 트리의 도움말을 파싱 전에 번역한다.
//! 번역 키가 없으면 컴파일된 영어를 유지한다. 영어 원문은 doc 주석이며
//! lang/en.toml은 slots로 추출한 값과 정확히 일치해야 한다.

use clap::{Arg, Command};

/// 키 앞머리. 이 아래는 전부 도움말 문자열이다.
pub const PREFIX: &str = "cli.help";

/// 루트 명령의 자리 이름. 서브커맨드 이름과 겹치지 않도록 `_` 로 시작한다.
const ROOT: &str = "_root";

/// 같은 이름의 명령과 인자를 구분하는 키 구성요소.
const ARG: &str = "arg";

/// TOML에서 문자열 값과 하위 테이블이 같은 키를 쓰지 않도록 말단 이름을 붙인다.
const ABOUT: &str = "about";
const LONG: &str = "long";
const HELP: &str = "help";

/// clap 이 스스로 넣는 항목. 우리 문자열이 아니므로 키를 만들지 않는다 — 만들면 번역자가
/// 채울 수 없는 키가 목록에 섞이고, parity 가드가 그것을 결함으로 센다.
fn is_clap_builtin_arg(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

/// 도움말 한 조각의 자리. 키와 컴파일된 영어를 함께 들고 다닌다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    /// `cli.help.…` 전체 키.
    pub key: String,
    /// 컴파일된 영어 원문. 여러 문단의 개행을 보존하도록 JSON 등으로 내보낸다.
    pub english: String,
}

/// 카탈로그 생성과 정합 검사가 함께 사용하는 도움말 목록.
pub fn slots(cmd: &Command) -> Vec<Slot> {
    let mut out = Vec::new();
    collect(cmd, ROOT, &mut out);
    out
}

fn collect(cmd: &Command, path: &str, out: &mut Vec<Slot>) {
    let base = format!("{PREFIX}.{path}");
    if let Some(about) = cmd.get_about() {
        out.push(Slot {
            key: format!("{base}.{ABOUT}"),
            english: about.to_string(),
        });
    }
    if let Some(long) = cmd.get_long_about() {
        out.push(Slot {
            key: format!("{base}.{LONG}"),
            english: long.to_string(),
        });
    }
    for arg in cmd.get_arguments() {
        if is_clap_builtin_arg(arg) {
            continue;
        }
        let id = arg.get_id().as_str();
        if let Some(help) = arg.get_help() {
            out.push(Slot {
                key: format!("{base}.{ARG}.{id}.{HELP}"),
                english: help.to_string(),
            });
        }
        if let Some(long) = arg.get_long_help() {
            out.push(Slot {
                key: format!("{base}.{ARG}.{id}.{LONG}"),
                english: long.to_string(),
            });
        }
    }
    for sub in cmd.get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue; // clap 이 만든 것
        }
        collect(sub, &format!("{path}.{name}"), out);
    }
}

/// 번역이 없으면 키 자체가 반환되므로 교체 전에 존재 여부를 확인한다.
fn translated(key: &str) -> Option<String> {
    let value = tasty_i18n::t(key);
    if value == key {
        None
    } else {
        Some(value.to_string())
    }
}

/// 트리를 순회하며 번역이 있는 자리만 갈아 끼운다. 없는 자리는 컴파일된 영어 그대로.
pub fn localize(cmd: Command) -> Command {
    localize_frame(localize_at(cmd, ROOT.to_string()))
}

fn localize_at(mut cmd: Command, path: String) -> Command {
    let base = format!("{PREFIX}.{path}");
    if cmd.get_about().is_some()
        && let Some(v) = translated(&format!("{base}.{ABOUT}"))
    {
        cmd = cmd.about(v);
    }
    if cmd.get_long_about().is_some()
        && let Some(v) = translated(&format!("{base}.{LONG}"))
    {
        cmd = cmd.long_about(v);
    }

    let arg_ids: Vec<String> = cmd
        .get_arguments()
        .filter(|a| !is_clap_builtin_arg(a))
        .map(|a| a.get_id().to_string())
        .collect();
    for id in arg_ids {
        let short = translated(&format!("{base}.{ARG}.{id}.{HELP}"));
        let long = translated(&format!("{base}.{ARG}.{id}.{LONG}"));
        if short.is_none() && long.is_none() {
            continue;
        }
        cmd = cmd.mut_arg(id, |mut a| {
            if a.get_help().is_some()
                && let Some(v) = short
            {
                a = a.help(v);
            }
            if a.get_long_help().is_some()
                && let Some(v) = long
            {
                a = a.long_help(v);
            }
            a
        });
    }

    let sub_names: Vec<String> = cmd
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .filter(|n| n != "help")
        .collect();
    for name in sub_names {
        let child = format!("{path}.{name}");
        cmd = cmd.mut_subcommand(&name, move |s| localize_at(s, child));
    }
    cmd
}

/// 제품 코드가 사용하는 번역된 명령 트리. Cli::command를 직접 호출하면 번역을 건너뛴다.
pub fn command() -> Command {
    use clap::CommandFactory;
    localize(crate::Cli::command())
}

/// Apply localized presentation to an already translated plugin command tree.
pub(crate) fn localize_frame(cmd: Command) -> Command {
    crate::help_frame::localize(cmd)
}
