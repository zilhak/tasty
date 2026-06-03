//! 매니페스트 `contributes.cli`를 런타임에 clap 서브커맨드로 등록하고,
//! 매칭된 결과를 JSON-RPC 메서드+params로 변환한다.
//!
//! 호스트 정적 `Cli` 파싱이 `InvalidSubcommand`로 실패할 때 진입한다 — 정적 우선,
//! 정적이 모르는 이름만 plugin CLI에서 찾는다. plugin이 호스트 명령을 가릴 수 없다.

use std::path::Path;

use anyhow::{Result, anyhow};
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory};
use serde_json::{Map, Value};

use tasty_ipc::protocol::JsonRpcRequest;
use tasty_plugin_manifest::{
    CliArg, CliArgGroup, CliArgType, CliCommandDecl, Manifest, PollingDecl,
};

/// 한 plugin이 contribute한 CLI 묶음.
#[derive(Debug, Clone)]
pub struct PluginCliEntry {
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
                    out.push(PluginCliEntry { cli: cli.clone() });
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
    // arg_required_else_help: subcommand 누락 시 에러 메시지 대신 풀 도움말을 출력 —
    // 호스트의 derive 기반 CLI(tasty claude 등)와 동일한 UX.
    let mut top = Command::new(leak_static(&decl.name))
        .subcommand_required(true)
        .arg_required_else_help(true);
    if let Some(desc) = decl.description.as_deref().filter(|s| !s.is_empty()) {
        top = top.about(leak_static(desc));
    }
    for sub in &decl.subcommands {
        let mut sc = Command::new(leak_static(&sub.name));
        if let Some(desc) = sub.description.as_deref().filter(|s| !s.is_empty()) {
            sc = sc.about(leak_static(desc));
        }
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
    if let Some(help) = arg.help.as_deref().filter(|s| !s.is_empty()) {
        a = a.help(leak_static(help));
    }
    a
}

/// 매칭된 `ArgMatches`에서 plugin이 어떤 메서드를 호출할지 해석.
/// 호스트 정적 서브커맨드와 충돌하지 않는 plugin 최상위 이름에 한해 진행한다.
///
/// 반환의 두 번째 값은 manifest 가 선언한 polling 사양 (있으면). caller 가
/// `Some(polling)` 일 때 *terminal_states 도달까지 반복 IPC 호출* 한다.
pub fn matches_to_request(
    entries: &[PluginCliEntry],
    matches: &ArgMatches,
) -> Result<(JsonRpcRequest, Option<PollingDecl>)> {
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
        // `stdin_json = true` 인 서브커맨드는 (stdin 이 TTY 가 아닐 때) stdin 의
        // JSON 한 덩이를 읽어 CLI 로 명시되지 않은 params 필드를 채운다.
        // Claude Code 처럼 hook payload 를 stdin JSON 으로 전달하는 외부 시스템
        // 연동용. CLI 로 직접 지정된 값이 항상 우선.
        if sub_decl.stdin_json {
            if let Some(stdin_json) = read_stdin_json() {
                merge_stdin_params(&mut params, g, &stdin_json);
            }
        }
        // claude CLI의 resolve_surface_id와 동일한 폴백 규칙. plugin이 정의한
        // `surface` (u32) 인자가 사용자 입력에 없으면 TASTY_SURFACE_ID env로 채운다.
        // IPC handler 들은 통상 `surface_id` 키를 기대하므로, 두 키 모두 주입한다.
        let defines_surface = g
            .flags
            .iter()
            .chain(g.positional.iter())
            .any(|a| a.name == "surface" && matches!(a.ty, CliArgType::U32));
        if defines_surface && !params.contains_key("surface") && !params.contains_key("surface_id")
        {
            if let Some(sid) = std::env::var("TASTY_SURFACE_ID")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
            {
                params.insert("surface".into(), Value::from(sid));
                params.insert("surface_id".into(), Value::from(sid));
            }
        }
        // 사용자가 명시적으로 --surface 를 줬을 때도 surface_id 동기. (IPC handler
        // 가 surface_id 키만 보는 경우 대응.)
        if let Some(v) = params.get("surface").cloned() {
            params.entry(String::from("surface_id")).or_insert(v);
        }
    }

    Ok((
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: sub_decl.ipc_method.clone(),
            params: Value::Object(params),
            id: Some(Value::from(1)),
            session_token: std::env::var("TASTY_SESSION_TOKEN")
                .ok()
                .filter(|s| !s.is_empty()),
        },
        sub_decl.polling.clone(),
    ))
}

/// stdin 이 TTY 가 아닐 때 (= pipe / redirect 로 입력이 들어올 때) stdin 전체를
/// JSON 한 덩이로 파싱한다. TTY 이거나 파싱 실패 시 `None`. blocking read 를
/// 피하기 위해 TTY 체크를 먼저 한다 — TTY 라면 사용자가 enter 칠 때까지 멈춰
/// 있을 위험이 있다.
fn read_stdin_json() -> Option<Value> {
    use std::io::{IsTerminal, Read};
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return None;
    }
    let mut buf = String::new();
    if stdin.read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
        return None;
    }
    serde_json::from_str(&buf).ok()
}

/// CLI 로 지정되지 않은 params 필드를, stdin JSON 의 해당 키에서 꺼내 채운다.
/// 매칭 키는 `arg.stdin_field` 우선, 없으면 `arg.name`. CLI 가 이미 채운 키는
/// 건드리지 않는다.
fn merge_stdin_params(params: &mut Map<String, Value>, group: &CliArgGroup, stdin: &Value) {
    let Some(obj) = stdin.as_object() else {
        return;
    };
    for arg in group.positional.iter().chain(group.flags.iter()) {
        if params.contains_key(&arg.name) {
            continue;
        }
        let key = arg.stdin_field.as_deref().unwrap_or(&arg.name);
        if let Some(v) = obj.get(key) {
            if !v.is_null() {
                params.insert(arg.name.clone(), v.clone());
            }
        }
    }
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
    use std::collections::HashMap;
    use tasty_plugin_manifest::{CliArg, CliArgGroup, CliArgType, CliSubcommandDecl};

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
                        help: None,
                        stdin_field: None,
                    },
                    CliArg {
                        name: "prompt".into(),
                        ty: CliArgType::String,
                        flag: Some("--prompt".into()),
                        required: false,
                        default: None,
                        help: None,
                        stdin_field: None,
                    },
                    CliArg {
                        name: "force".into(),
                        ty: CliArgType::Bool,
                        flag: Some("--force".into()),
                        required: false,
                        default: None,
                        help: None,
                        stdin_field: None,
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
                    help: None,
                    stdin_field: None,
                }],
                flags: vec![CliArg {
                    name: "timeout".into(),
                    ty: CliArgType::U32,
                    flag: Some("--timeout".into()),
                    required: false,
                    default: Some(toml::Value::Integer(60)),
                    help: None,
                    stdin_field: None,
                }],
            },
        );
        PluginCliEntry {
            cli: CliCommandDecl {
                name: "codex".into(),
                description: None,
                description_i18n_key: None,
                subcommands: vec![
                    CliSubcommandDecl {
                        name: "spawn".into(),
                        ipc_method: "codex.spawn".into(),
                        args: "spawn_args".into(),
                        description: None,
                        description_i18n_key: None,
                        stdin_json: false,
                        polling: None,
                    },
                    CliSubcommandDecl {
                        name: "broadcast".into(),
                        ipc_method: "codex.broadcast".into(),
                        args: "broadcast_args".into(),
                        description: None,
                        description_i18n_key: None,
                        stdin_json: false,
                        polling: None,
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
    fn merge_stdin_uses_stdin_field_alias() {
        // stdin JSON 의 키 이름이 CLI arg name 과 다른 경우 (`session_id` →
        // `session`) stdin_field 매핑이 적용되는지 확인. Claude Code hook payload
        // 의 session_id 가 `--session` 인자로 들어오는 동작이 이걸로 보장된다.
        let group = CliArgGroup {
            positional: vec![],
            flags: vec![
                CliArg {
                    name: "session".into(),
                    ty: CliArgType::String,
                    flag: Some("--session".into()),
                    required: false,
                    default: None,
                    help: None,
                    stdin_field: Some("session_id".into()),
                },
                CliArg {
                    name: "message".into(),
                    ty: CliArgType::String,
                    flag: Some("--message".into()),
                    required: false,
                    default: None,
                    help: None,
                    stdin_field: None,
                },
            ],
        };
        let stdin = serde_json::json!({
            "session_id": "abc-123",
            "message": "hi",
            "irrelevant": 42
        });
        let mut params = Map::new();
        merge_stdin_params(&mut params, &group, &stdin);
        assert_eq!(params["session"], Value::String("abc-123".into()));
        assert_eq!(params["message"], Value::String("hi".into()));
        // CLI arg 에 없는 stdin 키는 params 에 들어오지 않는다.
        assert!(!params.contains_key("irrelevant"));
    }

    #[test]
    fn merge_stdin_does_not_override_cli_explicit() {
        // CLI 로 명시된 값이 stdin 보다 우선.
        let group = CliArgGroup {
            positional: vec![],
            flags: vec![CliArg {
                name: "session".into(),
                ty: CliArgType::String,
                flag: Some("--session".into()),
                required: false,
                default: None,
                help: None,
                stdin_field: Some("session_id".into()),
            }],
        };
        let stdin = serde_json::json!({ "session_id": "from-stdin" });
        let mut params = Map::new();
        params.insert("session".into(), Value::String("from-cli".into()));
        merge_stdin_params(&mut params, &group, &stdin);
        assert_eq!(params["session"], Value::String("from-cli".into()));
    }

    #[test]
    fn merge_stdin_ignores_null_fields() {
        // stdin JSON 에 키가 있어도 값이 null 이면 params 에 넣지 않는다.
        let group = CliArgGroup {
            positional: vec![],
            flags: vec![CliArg {
                name: "session".into(),
                ty: CliArgType::String,
                flag: Some("--session".into()),
                required: false,
                default: None,
                help: None,
                stdin_field: Some("session_id".into()),
            }],
        };
        let stdin = serde_json::json!({ "session_id": null });
        let mut params = Map::new();
        merge_stdin_params(&mut params, &group, &stdin);
        assert!(!params.contains_key("session"));
    }

    #[test]
    fn flag_with_value_maps_to_params() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--surface", "5", "--prompt", "hello"]);
        let (req, _polling) = matches_to_request(&entries, &m).unwrap();
        assert_eq!(req.method, "codex.spawn");
        let p = req.params.as_object().unwrap();
        assert_eq!(p["surface"], Value::from(5_u32));
        assert_eq!(p["prompt"], Value::String("hello".into()));
    }

    #[test]
    fn bool_flag_present_serializes_true() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--force"]);
        let (req, _polling) = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(true));
    }

    #[test]
    fn bool_flag_absent_serializes_false() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn"]);
        let (req, _polling) = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(false));
    }

    #[test]
    fn default_value_applied_when_missing() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "broadcast", "hello"]);
        let (req, _polling) = matches_to_request(&entries, &m).unwrap();
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
        std::fs::write(plugin_bad.join("tasty-plugin.toml"), "not toml at all = {").unwrap();

        let entries = discover_plugin_clis(dir.path());
        let names: Vec<&str> = entries.iter().map(|e| e.cli.name.as_str()).collect();
        assert_eq!(names, vec!["a"]);
    }
}
