//! clap 도움말의 런타임 번역 — **트리를 순회하며 문자열을 갈아 끼운다.**
//!
//! clap 파생 매크로는 `about`/`help` 를 **컴파일 타임**에 잡는다. 런타임 언어를 적용하려면
//! `Cli::command()` 가 만든 [`clap::Command`] 트리를 순회하며 빌더 API
//! (`mut_subcommand`/`mut_arg`/`about`/`help`)로 교체한 뒤 **그 트리로 파싱**해야 한다.
//! 서브커맨드의 `--help` 는 clap 이 파싱 도중 직접 출력하므로, 교체를 파싱 전에 끝내지
//! 않으면 그 경로만 영어로 남는다.
//!
//! # 키가 없으면 원문을 유지한다 — 그리고 그 판정을 여기서 한다
//!
//! [`tasty_i18n::t`] 는 키가 없으면 **키 문자열 자체**를 돌려준다. 그대로 넣으면 도움말에
//! `cli.help.new.cwd` 같은 것이 찍힌다. 그래서 교체 전에 `t(key) != key` 로 **있는가**를
//! 먼저 묻고, 없으면 컴파일된 영어를 그대로 둔다. 번역이 부분적이어도 섞여서 읽히지
//! 조용히 깨지지 않는다.
//!
//! # 영어는 소스가 원본이고, `lang/en.toml` 은 같은 값을 복제한다
//!
//! doc comment 가 영어 원본이다(`clap_help_text_is_english_only` 가드가 그것을 강제한다).
//! `lang/en.toml` 이 같은 값을 갖는 이유는 두 가지다 — **번역자에게 키 목록**이 되고,
//! 언어팩 폴백 사슬의 뿌리가 빈칸이 되지 않는다. 자유도가 없는 복제라 손으로 맞추지
//! 않는다: 가드가 어긋난 키를 짚는다.

use clap::{Arg, Command};

/// 키 앞머리. 이 아래는 전부 도움말 문자열이다.
pub const PREFIX: &str = "cli.help";

/// 루트 명령의 자리 이름. 서브커맨드 이름과 겹치지 않도록 `_` 로 시작한다.
const ROOT: &str = "_root";

/// 인자 마디. `cli.help.<체인>.arg.<이름>.help` 의 `arg` 다 — 이 마디가 없으면 **같은
/// 이름의 서브커맨드와 인자가 한 키를 두고 다툰다**(`tasty surface list` 의 `list` 같은 형태).
const ARG: &str = "arg";

/// 잎 마디. **키가 값이면서 동시에 하위 테이블일 수는 없다** — 언어 카탈로그가 TOML 이라
/// `cli.help._root` 를 문자열로 쓰면서 `cli.help._root.new` 를 그 아래 두는 것이 표현되지
/// 않는다(파서가 거부한다). 그래서 값이 놓이는 자리에 항상 잎 마디를 붙여, 경로 마디와
/// 값 마디를 섞지 않는다. 이 조건은 `no_key_is_a_prefix_of_another` 가 지킨다.
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
    /// 소스(doc comment)에서 온 영어 원문.
    pub english: String,
}

/// 트리의 모든 도움말 자리를 키와 함께 낸다. 순서는 순회 순서 — 안정적이다.
///
/// parity 가드와 `lang/en.toml` 생성이 같은 함수를 쓴다. 둘이 각자 트리를 걸으면 같은
/// 물음에 답이 둘이 되고, 갈린 쪽은 조용해진다.
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

/// 번역 테이블에 그 키가 **실제로** 있는가. `t()` 가 키를 되돌려주는 구조라 이 물음을
/// 따로 물어야 한다.
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
    localize_at(cmd, ROOT.to_string())
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

/// **도움말 트리의 단일 급소.** 프로덕션에서 `Cli::command()` 를 직접 부르지 않는다.
///
/// 트리를 만드는 자리가 여섯이고(라우팅 · 트리 인쇄 · 증강 도움말 · plugin 증강 ·
/// `memory` 의 인자 조회 둘), 그중 하나라도 번역을 안 거치면 **그 경로의 도움말만 영어로
/// 남는다.** 그 어긋남은 조용하다 — 영어가 나오는 것은 결함처럼 안 보인다. 그래서 트리를
/// 얻는 길을 하나로 좁힌다.
pub fn command() -> Command {
    use clap::CommandFactory;
    localize(crate::Cli::command())
}
