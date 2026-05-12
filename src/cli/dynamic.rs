//! 매니페스트 `contributes.cli`를 런타임에 clap 서브커맨드로 등록하고,
//! 매칭된 결과를 JSON-RPC 메서드+params로 변환한다.
//!
//! 호스트 정적 `Cli` 파싱이 `InvalidSubcommand`로 실패할 때 진입한다 — 정적 우선,
//! 정적이 모르는 이름만 plugin CLI에서 찾는다. plugin이 호스트 명령을 가릴 수 없다.

use std::path::Path;

use anyhow::{anyhow, Result};
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory};
use serde_json::{Map, Value};

use crate::ipc::protocol::JsonRpcRequest;
use crate::plugin::manifest::{
    CliArg, CliArgGroup, CliArgType, CliCommandDecl, Manifest,
};

/// 한 plugin이 contribute한 CLI 묶음.
#[derive(Debug, Clone)]
pub struct PluginCliEntry {
    pub plugin_id: String,
    pub cli: CliCommandDecl,
}

/// `~/.tasty/plugins/*` 스캔. 파싱 실패한 매니페스트는 stderr에 경고만 찍고 스킵.
pub fn discover_plugin_clis(plugins_root: &Path) -> Vec<PluginCliEntry> {
    let mut out = Vec::new();
    let Ok(read_dir) = std::fs::read_dir(plugins_root) else {
        return out;
    };
    for entry in read_dir.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        if !dir.join("tasty-plugin.toml").exists() {
            continue;
        }
        match Manifest::load(&dir) {
            Ok(manifest) => {
                for cli in &manifest.contributes.cli {
                    out.push(PluginCliEntry {
                        plugin_id: manifest.id.clone(),
                        cli: cli.clone(),
                    });
                }
            }
            Err(e) => {
                eprintln!(
                    "warning: plugin manifest at {} skipped: {}",
                    dir.display(),
                    e
                );
            }
        }
    }
    out
}

/// 호스트 정적 `Cli`에 plugin 서브커맨드를 추가한 `clap::Command`. `--help` 출력
/// 통합과 동적 파싱에 공통 사용.
pub fn build_augmented_cli(entries: &[PluginCliEntry]) -> Command {
    let mut cmd = <super::Cli as CommandFactory>::command();
    for entry in entries {
        cmd = cmd.subcommand(build_cli_subcommand(&entry.cli));
    }
    cmd
}

/// clap 4의 빌더 API는 `&'static str`을 기대하는 곳이 있어, 매니페스트에서 읽은
/// 동적 문자열은 leak해서 정적화한다. CLI 진입은 프로세스당 한 번이며 plugin
/// 메니페스트 규모는 제한적이므로 누수 양이 무시 가능.
/// 같은 패턴이 `plugin::remote_kind`에도 있다.
fn leak_static(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn build_cli_subcommand(decl: &CliCommandDecl) -> Command {
    let mut top = Command::new(leak_static(&decl.name)).subcommand_required(true);
    for sub in &decl.subcommands {
        let mut sc = Command::new(leak_static(&sub.name));
        if let Some(group) = decl.arg_groups.get(&sub.args) {
            sc = apply_arg_group(sc, group);
        }
        top = top.subcommand(sc);
    }
    top
}

fn apply_arg_group(mut cmd: Command, group: &CliArgGroup) -> Command {
    for (idx, arg) in group.positional.iter().enumerate() {
        cmd = cmd.arg(build_arg(arg, Some(idx + 1)));
    }
    for arg in &group.flags {
        cmd = cmd.arg(build_arg(arg, None));
    }
    cmd
}

fn build_arg(arg: &CliArg, positional_index: Option<usize>) -> Arg {
    let mut a = Arg::new(leak_static(&arg.name));
    if let Some(i) = positional_index {
        a = a.index(i);
    } else if let Some(flag) = &arg.flag {
        a = a.long(leak_static(flag.trim_start_matches('-')));
    }
    a = a.required(arg.required);
    a = match arg.ty {
        CliArgType::Bool => a.action(ArgAction::SetTrue),
        _ => a.action(ArgAction::Set),
    };
    if let Some(default) = &arg.default {
        let s = match default {
            toml::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        a = a.default_value(leak_static(&s));
    }
    a
}

/// 매칭된 `ArgMatches`에서 plugin이 어떤 메서드를 호출할지 해석.
/// 호스트 정적 서브커맨드와 충돌하지 않는 plugin 최상위 이름에 한해 진행한다.
pub fn matches_to_request(
    entries: &[PluginCliEntry],
    matches: &ArgMatches,
) -> Result<JsonRpcRequest> {
    let (top_name, top_sub) = matches
        .subcommand()
        .ok_or_else(|| anyhow!("no subcommand supplied"))?;
    let entry = entries
        .iter()
        .find(|e| e.cli.name == top_name)
        .ok_or_else(|| anyhow!("'{}' is not a plugin-contributed cli command", top_name))?;
    let (sub_name, sub_args) = top_sub
        .subcommand()
        .ok_or_else(|| anyhow!("'{}' requires a subcommand", top_name))?;
    let sub_decl = entry
        .cli
        .subcommands
        .iter()
        .find(|s| s.name == sub_name)
        .ok_or_else(|| anyhow!("unknown subcommand '{} {}'", top_name, sub_name))?;
    let group = entry.cli.arg_groups.get(&sub_decl.args);

    let mut params = Map::new();
    if let Some(g) = group {
        for arg in g.positional.iter().chain(g.flags.iter()) {
            if let Some(v) = extract_value(sub_args, arg) {
                params.insert(arg.name.clone(), v);
            }
        }
    }

    Ok(JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: sub_decl.ipc_method.clone(),
        params: Value::Object(params),
        id: Some(Value::from(1)),
    })
}

fn extract_value(matches: &ArgMatches, arg: &CliArg) -> Option<Value> {
    match arg.ty {
        CliArgType::Bool => Some(Value::Bool(matches.get_flag(&arg.name))),
        CliArgType::U32 => matches
            .get_one::<String>(&arg.name)
            .and_then(|s| s.parse::<u32>().ok())
            .map(Value::from),
        CliArgType::I64 => matches
            .get_one::<String>(&arg.name)
            .and_then(|s| s.parse::<i64>().ok())
            .map(Value::from),
        CliArgType::String => matches
            .get_one::<String>(&arg.name)
            .map(|s| Value::String(s.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::{CliArg, CliArgGroup, CliArgType, CliSubcommandDecl};
    use std::collections::HashMap;

    fn sample_entry() -> PluginCliEntry {
        let mut arg_groups: HashMap<String, CliArgGroup> = HashMap::new();
        arg_groups.insert(
            "spawn_args".into(),
            CliArgGroup {
                positional: vec![],
                flags: vec![
                    CliArg {
                        name: "surface".into(),
                        ty: CliArgType::U32,
                        flag: Some("--surface".into()),
                        required: false,
                        default: None,
                    },
                    CliArg {
                        name: "prompt".into(),
                        ty: CliArgType::String,
                        flag: Some("--prompt".into()),
                        required: false,
                        default: None,
                    },
                    CliArg {
                        name: "force".into(),
                        ty: CliArgType::Bool,
                        flag: Some("--force".into()),
                        required: false,
                        default: None,
                    },
                ],
            },
        );
        arg_groups.insert(
            "broadcast_args".into(),
            CliArgGroup {
                positional: vec![CliArg {
                    name: "text".into(),
                    ty: CliArgType::String,
                    flag: None,
                    required: true,
                    default: None,
                }],
                flags: vec![CliArg {
                    name: "timeout".into(),
                    ty: CliArgType::U32,
                    flag: Some("--timeout".into()),
                    required: false,
                    default: Some(toml::Value::Integer(60)),
                }],
            },
        );
        PluginCliEntry {
            plugin_id: "com.example.codex".into(),
            cli: CliCommandDecl {
                name: "codex".into(),
                description_i18n_key: None,
                subcommands: vec![
                    CliSubcommandDecl {
                        name: "spawn".into(),
                        ipc_method: "codex.spawn".into(),
                        args: "spawn_args".into(),
                        description_i18n_key: None,
                    },
                    CliSubcommandDecl {
                        name: "broadcast".into(),
                        ipc_method: "codex.broadcast".into(),
                        args: "broadcast_args".into(),
                        description_i18n_key: None,
                    },
                ],
                arg_groups,
            },
        }
    }

    fn parse(args: &[&str]) -> ArgMatches {
        let entry = sample_entry();
        let augmented = build_augmented_cli(&[entry]);
        augmented
            .try_get_matches_from(std::iter::once("tasty").chain(args.iter().copied()))
            .expect("parse")
    }

    #[test]
    fn flag_with_value_maps_to_params() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--surface", "5", "--prompt", "hello"]);
        let req = matches_to_request(&entries, &m).unwrap();
        assert_eq!(req.method, "codex.spawn");
        let p = req.params.as_object().unwrap();
        assert_eq!(p["surface"], Value::from(5_u32));
        assert_eq!(p["prompt"], Value::String("hello".into()));
    }

    #[test]
    fn bool_flag_present_serializes_true() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--force"]);
        let req = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(true));
    }

    #[test]
    fn bool_flag_absent_serializes_false() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn"]);
        let req = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(false));
    }

    #[test]
    fn default_value_applied_when_missing() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "broadcast", "hello"]);
        let req = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["text"], Value::String("hello".into()));
        assert_eq!(p["timeout"], Value::from(60_u32));
    }

    #[test]
    fn positional_required() {
        let entries = vec![sample_entry()];
        let augmented = build_augmented_cli(&entries);
        let err = augmented.try_get_matches_from(["tasty", "codex", "broadcast"]);
        assert!(err.is_err(), "missing required positional should error");
    }

    #[test]
    fn unknown_top_level_subcommand_errors() {
        let entries = vec![sample_entry()];
        let augmented = build_augmented_cli(&entries);
        let res = augmented.try_get_matches_from(["tasty", "nonexistent", "spawn"]);
        assert!(res.is_err());
    }

    #[test]
    fn discover_skips_invalid_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plugin_a = dir.path().join("a");
        std::fs::create_dir_all(&plugin_a).unwrap();
        std::fs::write(
            plugin_a.join("tasty-plugin.toml"),
            r#"
manifest_version = 1
id = "com.example.a"
name = "A"
version = "0.1.0"
api_version = "1"

[entry]
type = "process"
command = "x"

[[contributes.ipc_namespace]]
prefix = "a"

[[contributes.cli]]
name = "a"
subcommands = [
  { name = "ping", ipc_method = "a.ping", args = "empty" },
]

[contributes.cli.arg_groups.empty]
"#,
        )
        .unwrap();

        let plugin_bad = dir.path().join("bad");
        std::fs::create_dir_all(&plugin_bad).unwrap();
        std::fs::write(plugin_bad.join("tasty-plugin.toml"), "not toml at all = {")
            .unwrap();

        let entries = discover_plugin_clis(dir.path());
        let ids: Vec<&str> = entries.iter().map(|e| e.plugin_id.as_str()).collect();
        assert_eq!(ids, vec!["com.example.a"]);
    }
}
