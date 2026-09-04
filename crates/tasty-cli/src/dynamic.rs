//! 매니페스트 `contributes.cli`를 런타임에 clap 서브커맨드로 등록하고,
//! 매칭된 결과를 JSON-RPC 메서드+params로 변환한다.
//!
//! 호스트 정적 `Cli` 파싱이 `InvalidSubcommand`로 실패할 때 진입한다 — 정적 우선,
//! 정적이 모르는 이름만 plugin CLI에서 찾는다. plugin이 호스트 명령을 가릴 수 없다.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Result, anyhow};
use clap::{Arg, ArgAction, ArgMatches, Command, CommandFactory};
use serde_json::{Map, Value};

use tasty_ipc::protocol::JsonRpcRequest;
use tasty_plugin_manifest::{
    AutoWaitDecl, CliArg, CliArgGroup, CliArgType, CliCommandDecl, CompletionStrategyDecl,
    Manifest, PollingDecl,
};

/// `spawn` / `tell` 같이 1 차 응답 후 chained wait 가 필요한 명령의 실행 계획.
/// `matches_to_request` 가 manifest `AutoWaitDecl` + 사용자 CLI 입력을 합쳐 빌드.
#[derive(Debug, Clone)]
pub struct AutoWaitPlan {
    pub method: String,
    pub polling: PollingDecl,
    pub map_from_response: HashMap<String, String>,
    pub map_from_request: HashMap<String, String>,
    pub timeout_field: String,
    /// 원 요청 params snapshot. wait params 구성 시 `map_from_request` 매핑과
    /// timeout 키 추출에 사용.
    pub request_params: Map<String, Value>,
    /// `--no-wait` 가 true 면 chain skip — caller 가 1 차 응답만 출력하고 종료.
    pub skipped: bool,
}

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
                    "{}",
                    tasty_i18n::t_fmt2(
                        "cli.plugin_cli.manifest_skipped",
                        &dir.display().to_string(),
                        &e.to_string()
                    )
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
        // `reject_repeat`: Set은 반복 지정 시 마지막 값만 조용히 남기고 앞선
        // 값을 버린다 — occurrence 자체가 유실되어 이후 판별이 불가능하다.
        // Append로 모든 occurrence를 보존해 두면 extract_value가 개수를 세어
        // 2개 이상이면 에러로 거부할 수 있다.
        _ if arg.reject_repeat => a.action(ArgAction::Append),
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
) -> Result<(JsonRpcRequest, Option<PollingDecl>, Option<AutoWaitPlan>)> {
    let (top_name, top_sub) = matches
        .subcommand()
        .ok_or_else(|| anyhow!("{}", tasty_i18n::t("cli.plugin_cli.no_subcommand")))?;
    let entry = entries
        .iter()
        .find(|e| e.cli.name == top_name)
        .ok_or_else(|| {
            anyhow!(
                "{}",
                tasty_i18n::t_fmt("cli.plugin_cli.not_plugin_command", top_name)
            )
        })?;
    let (sub_name, sub_args) = top_sub.subcommand().ok_or_else(|| {
        anyhow!(
            "{}",
            tasty_i18n::t_fmt("cli.plugin_cli.subcommand_required", top_name)
        )
    })?;
    let sub_decl = entry
        .cli
        .subcommands
        .iter()
        .find(|s| s.name == sub_name)
        .ok_or_else(|| {
            anyhow!(
                "{}",
                tasty_i18n::t_fmt2("cli.plugin_cli.unknown_subcommand", top_name, sub_name)
            )
        })?;
    let group = entry.cli.arg_groups.get(&sub_decl.args);

    let mut params = Map::new();
    if let Some(g) = group {
        for arg in g.positional.iter().chain(g.flags.iter()) {
            if let Some(v) = extract_value(sub_args, arg)? {
                // `path_kind = "directory"`/`"file"` 이 선언된 string 인자는 CLI
                // process cwd 기준 absolute path 로 정규화 + 존재(+종류) 검증.
                // 실패 시 즉시 에러 — 호스트/plugin 은 절대경로만 받는다는 contract.
                let v = if matches!(arg.ty, CliArgType::String)
                    && let Some(raw) = v.as_str()
                {
                    match arg.path_kind.as_deref() {
                        Some("directory") => Value::String(
                            super::cwd_resolve::normalize_cwd_arg(raw).map_err(|e| {
                                anyhow!(
                                    "{}",
                                    tasty_i18n::t_fmt2(
                                        "cli.plugin_cli.arg_invalid",
                                        &arg.name,
                                        &e.to_string()
                                    )
                                )
                            })?,
                        ),
                        Some("file") => Value::String(
                            super::cwd_resolve::normalize_file_arg(raw).map_err(|e| {
                                anyhow!(
                                    "{}",
                                    tasty_i18n::t_fmt2(
                                        "cli.plugin_cli.arg_invalid",
                                        &arg.name,
                                        &e.to_string()
                                    )
                                )
                            })?,
                        ),
                        _ => v,
                    }
                } else {
                    v
                };
                params.insert(arg.name.clone(), v);
            }
        }
        // `stdin_json = true` 인 서브커맨드는 (stdin 이 TTY 가 아닐 때) stdin 의
        // JSON 한 덩이를 읽어 CLI 로 명시되지 않은 params 필드를 채운다.
        // Claude Code 처럼 hook payload 를 stdin JSON 으로 전달하는 외부 시스템
        // 연동용. CLI 로 직접 지정된 값이 항상 우선.
        if sub_decl.stdin_json
            && let Some(stdin_json) = read_stdin_json()
        {
            merge_stdin_params(&mut params, g, &stdin_json)?;
        }
        // claude CLI의 resolve_surface_id와 동일한 폴백 규칙. plugin이 정의한
        // `surface` (u32) 인자가 사용자 입력에 없으면 TASTY_SURFACE_ID env로 채운다.
        // IPC handler 들은 통상 `surface_id` 키를 기대하므로, 두 키 모두 주입한다.
        let defines_surface = g
            .flags
            .iter()
            .chain(g.positional.iter())
            .any(|a| a.name == "surface" && matches!(a.ty, CliArgType::U32));
        if defines_surface
            && !params.contains_key("surface")
            && !params.contains_key("surface_id")
            && let Some(sid) = std::env::var("TASTY_SURFACE_ID")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
        {
            params.insert("surface".into(), Value::from(sid));
            params.insert("surface_id".into(), Value::from(sid));
        }
        // 사용자가 명시적으로 --surface 를 줬을 때도 surface_id 동기. (IPC handler
        // 가 surface_id 키만 보는 경우 대응.)
        if let Some(v) = params.get("surface").cloned() {
            params.entry(String::from("surface_id")).or_insert(v);
        }
        // `tell` 등 target(`surface`)과 caller 를 구분해야 하는 명령을 위한 자동
        // 채움. 필드명 `caller_surface` 로 고정(claude/codex 공용) — `surface`용
        // 자동 채움과 동일한 패턴이지만 별도 필드명이므로 독립 블록. plugin-private
        // 키라 `surface_id` 류 dual-write 는 하지 않는다(호스트 IPC 표준 키가 아님).
        let defines_caller_surface = g
            .flags
            .iter()
            .chain(g.positional.iter())
            .any(|a| a.name == "caller_surface" && matches!(a.ty, CliArgType::U32));
        if defines_caller_surface
            && !params.contains_key("caller_surface")
            && let Some(sid) = std::env::var("TASTY_SURFACE_ID")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
        {
            params.insert("caller_surface".into(), Value::from(sid));
        }
    }

    // Track B(completion strategy registry)가 아직 병합되지 않아 이 매니페스트가
    // 실제로 이름으로 등록한 strategy 를 조회할 곳이 없다 — registry 가 들어오면
    // `entry`(혹은 그 소속 Manifest)에서 모은 실 데이터를 여기 채운다. 그때까지
    // `AutoWaitDecl.strategy` 는 항상 "unknown strategy" 로 reject 된다(인라인
    // `polling` 경로는 이 맵과 무관하게 그대로 동작).
    let available_strategies: HashMap<String, CompletionStrategyDecl> = HashMap::new();
    let auto_wait_plan = sub_decl
        .auto_wait
        .as_ref()
        .map(|aw| build_auto_wait_plan(aw, &params, &available_strategies))
        .transpose()?;

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
        auto_wait_plan,
    ))
}

/// `AutoWaitDecl.polling`(인라인) 또는 `.strategy`(이름 참조)를 실행 가능한
/// `PollingDecl` 로 해석한다. manifest validator 가 이미 정확히 하나만
/// 선언되도록 강제하므로(§ `validate_auto_wait_strategy`) 여기서는 그 불변식을
/// 신뢰해 매칭한다 — validator 를 통과했는데도 실패할 수 있는 경우는 오직
/// `available_strategies` 에 그 이름이 아직 없을 때뿐이다(같은 매니페스트 안의
/// registry 조회 실패).
fn resolve_auto_wait_polling(
    aw: &AutoWaitDecl,
    available_strategies: &HashMap<String, CompletionStrategyDecl>,
) -> Result<PollingDecl> {
    if let Some(polling) = &aw.polling {
        return Ok(polling.clone());
    }
    let strategy = aw.strategy.as_ref().ok_or_else(|| {
        anyhow!(
            "{}",
            tasty_i18n::t_fmt("cli.plugin_cli.auto_wait_no_mode", &aw.method)
        )
    })?;
    available_strategies
        .get(strategy)
        .map(CompletionStrategyDecl::to_polling_decl)
        .ok_or_else(|| {
            anyhow!(
                "{}",
                tasty_i18n::t_fmt2(
                    "cli.plugin_cli.auto_wait_unknown_strategy",
                    &aw.method,
                    strategy
                )
            )
        })
}

/// `AutoWaitDecl` 와 1 차 요청 params 로 실행 계획을 구성한다.
/// `--no-wait` (params 의 `no_wait_field` 가 true 인 경우) 면 `skipped = true`.
fn build_auto_wait_plan(
    aw: &AutoWaitDecl,
    request_params: &Map<String, Value>,
    available_strategies: &HashMap<String, CompletionStrategyDecl>,
) -> Result<AutoWaitPlan> {
    let skipped = request_params
        .get(&aw.no_wait_field)
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let polling = resolve_auto_wait_polling(aw, available_strategies)?;
    Ok(AutoWaitPlan {
        method: aw.method.clone(),
        polling,
        map_from_response: aw.map_from_response.clone(),
        map_from_request: aw.map_from_request.clone(),
        timeout_field: aw.timeout_field.clone(),
        request_params: request_params.clone(),
        skipped,
    })
}

/// `read_stdin_json`이 값을 만들지 못한 이유. 어느 경로였는지 로그로 구분하기
/// 위한 것 — `claude hook session-start`가 이 경로 중 하나로 `None`을 받으면
/// `claude-session-id` meta 가 기록되지 않는다(사고 2026-08-05, surface 3095).
enum StdinSkipReason {
    /// 사람이 터미널에서 직접 커맨드를 입력한 경우 정상적으로 발생.
    Tty,
    ReadError(std::io::Error),
    /// non-TTY 인데도 payload 가 비어 있음 — 호출자(예: Claude Code hook
    /// 시스템)가 stdin 을 채우지 않은 상태.
    Empty,
    ParseError(serde_json::Error),
}

impl StdinSkipReason {
    /// TTY 는 사람이 직접 커맨드를 입력했을 때 정상적으로 발생 — warn 대상이
    /// 아니다. 나머지(non-TTY 인데 값이 없음)는 호출자가 payload 를 못 채운
    /// 것이므로 warn.
    fn is_expected(&self) -> bool {
        matches!(self, Self::Tty)
    }

    fn describe(&self) -> String {
        match self {
            Self::Tty => "stdin is a TTY".to_string(),
            Self::ReadError(e) => format!("failed to read stdin (non-TTY): {e}"),
            Self::Empty => "stdin was empty (non-TTY but no data piped)".to_string(),
            Self::ParseError(e) => format!("failed to parse stdin as JSON: {e}"),
        }
    }
}

/// stdin 이 TTY 가 아닐 때 (= pipe / redirect 로 입력이 들어올 때) stdin 전체를
/// JSON 한 덩이로 파싱한다. TTY 이거나 파싱 실패 시 `None`. blocking read 를
/// 피하기 위해 TTY 체크를 먼저 한다 — TTY 라면 사용자가 enter 칠 때까지 멈춰
/// 있을 위험이 있다.
fn read_stdin_json() -> Option<Value> {
    let reason = match try_read_stdin_json() {
        Ok(v) => return Some(v),
        Err(reason) => reason,
    };
    if reason.is_expected() {
        tracing::debug!("read_stdin_json: {}", reason.describe());
    } else {
        tracing::warn!("read_stdin_json: {}", reason.describe());
    }
    None
}

fn try_read_stdin_json() -> Result<Value, StdinSkipReason> {
    use std::io::{IsTerminal, Read};
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(StdinSkipReason::Tty);
    }
    let mut buf = String::new();
    stdin
        .read_to_string(&mut buf)
        .map_err(StdinSkipReason::ReadError)?;
    if buf.trim().is_empty() {
        return Err(StdinSkipReason::Empty);
    }
    serde_json::from_str(&buf).map_err(StdinSkipReason::ParseError)
}

/// CLI 로 지정되지 않은 params 필드를, stdin JSON 의 해당 키에서 꺼내 채운다.
/// 매칭 키는 `arg.stdin_field` 우선, 없으면 `arg.name`. CLI 가 이미 채운 키는
/// 건드리지 않는다.
///
/// **선언 타입은 여기서도 강제한다.** 이 경로는 `extract_value`(=`--flag` 경로)를
/// 지나지 않으므로, 강제를 그쪽에만 두면 같은 `CliArg` 선언이 **들어온 문으로만**
/// 참인 보증이 된다(ADR-0132). 매니페스트가 `u32` 라고 적어둔 인자에 stdin JSON 이
/// 객체나 문자열을 실어 보내면 그대로 params 에 들어가 하류가 그것을 받는다.
fn merge_stdin_params(
    params: &mut Map<String, Value>,
    group: &CliArgGroup,
    stdin: &Value,
) -> Result<()> {
    let Some(obj) = stdin.as_object() else {
        return Ok(());
    };
    for arg in group.positional.iter().chain(group.flags.iter()) {
        if params.contains_key(&arg.name) {
            continue;
        }
        let key = arg.stdin_field.as_deref().unwrap_or(&arg.name);
        if let Some(v) = obj.get(key)
            && !v.is_null()
        {
            params.insert(arg.name.clone(), coerce_stdin_value(v, arg)?);
        }
    }
    Ok(())
}

/// stdin JSON 값을 선언 타입에 맞춘다. 숫자 타입은 `--flag` 경로와 **같은 규칙**을
/// 쓴다 — 숫자로 읽히면 숫자로, 아니면 거부. `string`/`bool` 은 JSON 이 이미 타입을
/// 싣고 오므로 그대로 통과시킨다(그 둘의 강제는 이 ADR 의 범위 밖이다).
fn coerce_stdin_value(v: &Value, arg: &CliArg) -> Result<Value> {
    match arg.ty {
        CliArgType::U32 => coerce_stdin_number::<u32>(v, arg),
        CliArgType::I64 => coerce_stdin_number::<i64>(v, arg),
        CliArgType::String | CliArgType::Bool => Ok(v.clone()),
    }
}

/// JSON 값을 문자열 한 형태로 눕힌 뒤 `parse_number` 에 넘긴다. 눕히는 이유는
/// 오류 메시지를 `--flag` 경로와 같게 하기 위해서다 — 호출자는 두 문 중 어느 쪽으로
/// 들어왔든 "어느 인자의 어떤 값이 문제인가" 를 같은 문장으로 받는다.
/// `1.5`(정수 아님) · `true` · 배열/객체는 전부 여기서 거부된다.
fn coerce_stdin_number<T>(v: &Value, arg: &CliArg) -> Result<Value>
where
    T: std::str::FromStr,
    Value: From<T>,
{
    let raw = match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Ok(Value::from(parse_number::<T>(&raw, arg)?))
}

/// 숫자 인자는 **값이 왔는데 못 읽으면 오류**다.
///
/// `parse().ok()` 로 `None` 을 만들면 하류에서 **"플래그가 아예 없음" 과 구별되지 않는다.**
/// 그러면 없을 때 도는 기본값 경로가 그대로 돌아, 사용자가 지정한 대상 대신 기본 대상으로
/// 조용히 실행된다 — `--surface` 의 기본값은 호출자 **자신**이라 명령이 자기에게 배달된다.
/// 종료코드는 0 이고 오류도 없어서, 응답의 주소를 따로 대조하지 않으면 드러나지 않는다.
fn parse_number<T>(raw: &str, arg: &CliArg) -> Result<T>
where
    T: std::str::FromStr,
{
    raw.parse::<T>().map_err(|_| {
        anyhow!(
            "{}",
            tasty_i18n::t_fmt2(
                "cli.plugin_cli.flag_not_a_number",
                arg.flag
                    .as_deref()
                    .unwrap_or(&arg.name)
                    .trim_start_matches('-'),
                raw
            )
        )
    })
}

fn extract_value(matches: &ArgMatches, arg: &CliArg) -> Result<Option<Value>> {
    // `reject_repeat` 인자는 build_arg 가 ArgAction::Append 로 등록하므로(모든
    // occurrence 보존), get_one 대신 get_many 로 개수부터 확인한다 — Set 전용
    // 접근(get_one)을 Append 인자에 섞으면 clap 내부 불변식과 어긋난다.
    if arg.reject_repeat {
        let mut values = matches
            .get_many::<String>(&arg.name)
            .map(|it| it.collect::<Vec<_>>())
            .unwrap_or_default();
        if values.len() > 1 {
            return Err(anyhow!(
                "{}",
                tasty_i18n::t_fmt2(
                    "cli.plugin_cli.flag_repeated",
                    arg.flag
                        .as_deref()
                        .unwrap_or(&arg.name)
                        .trim_start_matches('-'),
                    &values.len().to_string()
                )
            ));
        }
        return Ok(match arg.ty {
            CliArgType::U32 => match values.pop() {
                Some(s) => Some(Value::from(parse_number::<u32>(s, arg)?)),
                None => None,
            },
            CliArgType::I64 => match values.pop() {
                Some(s) => Some(Value::from(parse_number::<i64>(s, arg)?)),
                None => None,
            },
            CliArgType::String => values.pop().map(|s| Value::String(s.clone())),
            // build_arg는 Bool을 항상 SetTrue로 등록한다(reject_repeat 무관) —
            // 여기 도달하면 매니페스트 오설정이니 get_flag로 안전하게 처리.
            CliArgType::Bool => Some(Value::Bool(matches.get_flag(&arg.name))),
        });
    }
    Ok(match arg.ty {
        CliArgType::Bool => Some(Value::Bool(matches.get_flag(&arg.name))),
        CliArgType::U32 => match matches.get_one::<String>(&arg.name) {
            Some(s) => Some(Value::from(parse_number::<u32>(s, arg)?)),
            None => None,
        },
        CliArgType::I64 => match matches.get_one::<String>(&arg.name) {
            Some(s) => Some(Value::from(parse_number::<i64>(s, arg)?)),
            None => None,
        },
        CliArgType::String => matches
            .get_one::<String>(&arg.name)
            .map(|s| Value::String(s.clone())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tasty_plugin_manifest::{CliArg, CliArgGroup, CliArgType, CliSubcommandDecl};

    fn new_subcommand(name: &str, ipc_method: &str, args: &str) -> CliSubcommandDecl {
        CliSubcommandDecl {
            name: name.into(),
            ipc_method: ipc_method.into(),
            args: args.into(),
            description: None,
            description_i18n_key: None,
            stdin_json: false,
            polling: None,
            auto_wait: None,
        }
    }

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
                        path_kind: None,
                        reject_repeat: false,
                    },
                    CliArg {
                        name: "prompt".into(),
                        ty: CliArgType::String,
                        flag: Some("--prompt".into()),
                        required: false,
                        default: None,
                        help: None,
                        stdin_field: None,
                        path_kind: None,
                        reject_repeat: false,
                    },
                    CliArg {
                        name: "force".into(),
                        ty: CliArgType::Bool,
                        flag: Some("--force".into()),
                        required: false,
                        default: None,
                        help: None,
                        stdin_field: None,
                        path_kind: None,
                        reject_repeat: false,
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
                    path_kind: None,
                    reject_repeat: false,
                }],
                flags: vec![CliArg {
                    name: "timeout".into(),
                    ty: CliArgType::U32,
                    flag: Some("--timeout".into()),
                    required: false,
                    default: Some(toml::Value::Integer(60)),
                    help: None,
                    stdin_field: None,
                    path_kind: None,
                    reject_repeat: false,
                }],
            },
        );
        PluginCliEntry {
            cli: CliCommandDecl {
                name: "codex".into(),
                description: None,
                description_i18n_key: None,
                subcommands: vec![
                    new_subcommand("spawn", "codex.spawn", "spawn_args"),
                    new_subcommand("broadcast", "codex.broadcast", "broadcast_args"),
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
                    path_kind: None,
                    reject_repeat: false,
                },
                CliArg {
                    name: "message".into(),
                    ty: CliArgType::String,
                    flag: Some("--message".into()),
                    required: false,
                    default: None,
                    help: None,
                    stdin_field: None,
                    path_kind: None,
                    reject_repeat: false,
                },
            ],
        };
        let stdin = serde_json::json!({
            "session_id": "abc-123",
            "message": "hi",
            "irrelevant": 42
        });
        let mut params = Map::new();
        merge_stdin_params(&mut params, &group, &stdin)
            .expect("이 회차의 stdin 값은 선언 타입과 맞다");
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
                path_kind: None,
                reject_repeat: false,
            }],
        };
        let stdin = serde_json::json!({ "session_id": "from-stdin" });
        let mut params = Map::new();
        params.insert("session".into(), Value::String("from-cli".into()));
        merge_stdin_params(&mut params, &group, &stdin)
            .expect("이 회차의 stdin 값은 선언 타입과 맞다");
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
                path_kind: None,
                reject_repeat: false,
            }],
        };
        let stdin = serde_json::json!({ "session_id": null });
        let mut params = Map::new();
        merge_stdin_params(&mut params, &group, &stdin)
            .expect("이 회차의 stdin 값은 선언 타입과 맞다");
        assert!(!params.contains_key("session"));
    }

    #[test]
    fn flag_with_value_maps_to_params() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--surface", "5", "--prompt", "hello"]);
        let (req, _polling, _auto) = matches_to_request(&entries, &m).unwrap();
        assert_eq!(req.method, "codex.spawn");
        let p = req.params.as_object().unwrap();
        assert_eq!(p["surface"], Value::from(5_u32));
        assert_eq!(p["prompt"], Value::String("hello".into()));
    }

    /// 숫자 플래그에 비수치가 오면 **거부**한다.
    ///
    /// 예전에는 `parse().ok()` 로 `None` 이 되어 하류에서 "플래그 없음" 과 같아졌고,
    /// 그 자리에 없을 때 도는 기본값이 들어갔다. `--surface` 의 기본값은 호출자 자신이라
    /// 명령이 **자기에게 배달**됐다 — 종료코드 0, 오류 없음. 실제로 그렇게 잃은 적이 있다.
    /// stdin JSON 경로는 `extract_value` 를 지나지 않는다. 강제를 한쪽 문에만 두면
    /// 같은 `CliArg` 선언이 들어온 문에 따라 다른 뜻이 된다 — 그 비대칭을 고정한다.
    fn spawn_group(entry: &PluginCliEntry) -> &CliArgGroup {
        entry
            .cli
            .arg_groups
            .get("spawn_args")
            .expect("전제: sample_entry 에 spawn_args 가 있다")
    }

    #[test]
    fn stdin_json_number_flag_takes_a_number_and_a_numeric_string() {
        tasty_i18n::init("en");
        let entry = sample_entry();
        let g = spawn_group(&entry);

        let mut params = Map::new();
        let stdin = serde_json::json!({ "surface": 42 });
        merge_stdin_params(&mut params, g, &stdin).expect("숫자는 통과해야 한다");
        assert_eq!(params.get("surface"), Some(&Value::from(42u32)));

        // 문자열이라도 숫자로 읽히면 `--surface 42` 와 같게 다룬다 — 두 문의 규칙이
        // 달라지면 그 자체가 다음 오보의 자리가 된다.
        let mut params = Map::new();
        let stdin = serde_json::json!({ "surface": "42" });
        merge_stdin_params(&mut params, g, &stdin).expect("숫자 문자열도 통과해야 한다");
        assert_eq!(params.get("surface"), Some(&Value::from(42u32)));
    }

    #[test]
    fn stdin_json_non_numeric_value_for_a_number_flag_is_rejected() {
        tasty_i18n::init("en");
        let entry = sample_entry();
        let g = spawn_group(&entry);

        for bad in [
            serde_json::json!({ "surface": "conductor" }),
            serde_json::json!({ "surface": 1.5 }),
            serde_json::json!({ "surface": true }),
            serde_json::json!({ "surface": { "id": 1 } }),
        ] {
            let mut params = Map::new();
            let err = merge_stdin_params(&mut params, g, &bad)
                .expect_err("비수치 stdin 값은 오류여야 한다: {bad}");
            let msg = err.to_string();
            assert!(msg.contains("surface"), "어느 인자인지 담아야 한다: {msg}");
            assert!(
                params.get("surface").is_none(),
                "거부된 값이 params 에 남으면 안 된다"
            );
        }
    }

    #[test]
    fn stdin_json_does_not_override_a_value_the_cli_already_gave() {
        tasty_i18n::init("en");
        let entry = sample_entry();
        let g = spawn_group(&entry);

        // CLI 가 이미 채운 키는 stdin 이 무엇을 싣고 오든 건드리지 않는다. 그래서
        // 그 값이 비수치여도 여기서는 오류가 나지 않는다 — `--flag` 경로가 이미
        // 검사한 뒤이기 때문이다(같은 값을 두 번 판정하지 않는다).
        let mut params = Map::new();
        params.insert("surface".into(), Value::from(7u32));
        params.insert("prompt".into(), Value::String("hi".into()));
        let stdin = serde_json::json!({ "surface": "conductor", "prompt": "bye" });
        merge_stdin_params(&mut params, g, &stdin).expect("CLI 값이 우선이라 통과한다");
        assert_eq!(params.get("surface"), Some(&Value::from(7u32)));
        assert_eq!(params.get("prompt"), Some(&Value::String("hi".into())));
    }

    #[test]
    fn stdin_json_string_and_bool_args_pass_through_unchanged() {
        tasty_i18n::init("en");
        let entry = sample_entry();
        let g = spawn_group(&entry);

        let mut params = Map::new();
        let stdin = serde_json::json!({ "prompt": "hi", "force": true });
        merge_stdin_params(&mut params, g, &stdin).expect("문자열·불리언은 그대로");
        assert_eq!(params.get("prompt"), Some(&Value::String("hi".into())));
        assert_eq!(params.get("force"), Some(&Value::Bool(true)));
    }

    #[test]
    fn non_numeric_value_for_a_number_flag_is_rejected_not_dropped() {
        // `tasty_i18n::init` 은 프로세스당 1 회 `OnceLock` 이고, 이 바이너리의 다른
        // 테스트(`run.rs`)도 "en" 으로 초기화한다. 여기서 먼저 부르는 것은 값을 바꾸는
        // 것이 아니라 **순서 경합을 없애는 것**이다 — 부르지 않으면 언어팩 로드 여부가
        // 스레드 순서에 달려 메시지가 키(미로드)와 영문(로드) 사이에서 흔들린다.
        tasty_i18n::init("en");
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--surface", "conductor", "--prompt", "hi"]);
        let err = matches_to_request(&entries, &m).expect_err("비수치 --surface 는 오류여야 한다");
        let msg = err.to_string();
        assert_ne!(
            msg, "cli.plugin_cli.flag_not_a_number",
            "번역 키가 그대로 새어 나오면 안 된다"
        );
        assert!(
            msg.contains("surface"),
            "어느 플래그인지 담아야 한다: {msg}"
        );
        assert!(msg.contains("conductor"), "받은 값을 담아야 한다: {msg}");
    }

    /// 위 테스트의 대우 — 플래그가 **아예 없는** 것은 여전히 오류가 아니다.
    /// 둘을 가르지 못하는 것이 원래 결함이었으므로 양쪽을 함께 박는다.
    #[test]
    fn an_absent_number_flag_is_still_not_an_error() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--prompt", "hi"]);
        let (req, _polling, _auto) =
            matches_to_request(&entries, &m).expect("없는 플래그는 오류가 아니다");
        assert_eq!(
            req.params.as_object().unwrap()["prompt"],
            Value::String("hi".into())
        );
    }

    #[test]
    fn bool_flag_present_serializes_true() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn", "--force"]);
        let (req, _polling, _auto) = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(true));
    }

    #[test]
    fn bool_flag_absent_serializes_false() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "spawn"]);
        let (req, _polling, _auto) = matches_to_request(&entries, &m).unwrap();
        let p = req.params.as_object().unwrap();
        assert_eq!(p["force"], Value::Bool(false));
    }

    #[test]
    fn default_value_applied_when_missing() {
        let entries = vec![sample_entry()];
        let m = parse(&["codex", "broadcast", "hello"]);
        let (req, _polling, _auto) = matches_to_request(&entries, &m).unwrap();
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

    fn sample_auto_wait_decl() -> AutoWaitDecl {
        let mut map_from_response = HashMap::new();
        map_from_response.insert("child_surface_id".into(), "surface_id".into());
        let mut map_from_request = HashMap::new();
        map_from_request.insert("surface".into(), "surface".into());
        AutoWaitDecl {
            method: "claude.wait_by_surface".into(),
            map_from_response,
            map_from_request,
            polling: Some(PollingDecl {
                state_field: "state".into(),
                terminal_states: vec!["idle".into(), "exited".into()],
                interval_ms: 100,
                timeout_field: Some("timeout".into()),
            }),
            strategy: None,
            no_wait_field: "no_wait".into(),
            timeout_field: "timeout".into(),
        }
    }

    fn empty_strategies() -> HashMap<String, CompletionStrategyDecl> {
        HashMap::new()
    }

    #[test]
    fn auto_wait_skipped_when_no_wait_flag() {
        // --no-wait 가 params 에 true 로 들어오면 AutoWaitPlan.skipped = true.
        let aw = sample_auto_wait_decl();
        let mut params = Map::new();
        params.insert("no_wait".into(), Value::Bool(true));
        params.insert("surface".into(), Value::from(7_u32));
        let plan = build_auto_wait_plan(&aw, &params, &empty_strategies()).unwrap();
        assert!(plan.skipped, "no_wait=true should skip chain");
        assert_eq!(plan.method, "claude.wait_by_surface");
    }

    #[test]
    fn auto_wait_not_skipped_when_no_wait_absent_or_false() {
        // no_wait 키 부재 / false 면 chain 진행.
        let aw = sample_auto_wait_decl();
        let mut params = Map::new();
        params.insert("surface".into(), Value::from(7_u32));
        let plan = build_auto_wait_plan(&aw, &params, &empty_strategies()).unwrap();
        assert!(!plan.skipped);

        let mut params2 = Map::new();
        params2.insert("no_wait".into(), Value::Bool(false));
        let plan2 = build_auto_wait_plan(&aw, &params2, &empty_strategies()).unwrap();
        assert!(!plan2.skipped);
    }

    #[test]
    fn auto_wait_plan_snapshots_request_params() {
        // build_auto_wait_plan 은 1 차 요청 params 를 그대로 snapshot 해 둔다 —
        // 나중에 build_wait_params 가 map_from_request 매핑에 사용.
        let aw = sample_auto_wait_decl();
        let mut params = Map::new();
        params.insert("surface".into(), Value::from(42_u32));
        params.insert("prompt".into(), Value::String("hi".into()));
        let plan = build_auto_wait_plan(&aw, &params, &empty_strategies()).unwrap();
        assert_eq!(
            plan.request_params.get("surface"),
            Some(&Value::from(42_u32))
        );
        assert_eq!(
            plan.request_params.get("prompt"),
            Some(&Value::String("hi".into()))
        );
        // map_from_response / map_from_request 는 그대로 복사.
        assert_eq!(
            plan.map_from_response.get("child_surface_id"),
            Some(&"surface_id".into())
        );
        assert_eq!(
            plan.map_from_request.get("surface"),
            Some(&"surface".into())
        );
    }

    #[test]
    fn auto_wait_plan_carries_polling_and_timeout_field() {
        // polling 사양 + timeout_field 가 그대로 plan 에 전파되는지.
        let aw = sample_auto_wait_decl();
        let plan = build_auto_wait_plan(&aw, &Map::new(), &empty_strategies()).unwrap();
        assert_eq!(plan.polling.state_field, "state");
        assert_eq!(plan.polling.terminal_states, vec!["idle", "exited"]);
        assert_eq!(plan.polling.interval_ms, 100);
        assert_eq!(plan.timeout_field, "timeout");
    }

    #[test]
    fn auto_wait_custom_no_wait_field_name() {
        // manifest 가 `no_wait_field` 를 커스텀으로 지정한 경우 그 키를 본다.
        let mut aw = sample_auto_wait_decl();
        aw.no_wait_field = "skip_chain".into();
        let mut params = Map::new();
        params.insert("skip_chain".into(), Value::Bool(true));
        // 표준 "no_wait" 키는 true 가 아니므로 만약 잘못 보면 skipped=false.
        let plan = build_auto_wait_plan(&aw, &params, &empty_strategies()).unwrap();
        assert!(
            plan.skipped,
            "custom no_wait_field='skip_chain' should be honored"
        );
    }

    fn sample_auto_wait_decl_with_strategy(strategy: &str) -> AutoWaitDecl {
        let mut aw = sample_auto_wait_decl();
        aw.polling = None;
        aw.strategy = Some(strategy.into());
        aw
    }

    #[test]
    fn resolve_auto_wait_polling_finds_registered_strategy() {
        let aw = sample_auto_wait_decl_with_strategy("com.example.x/wait-ready");
        let decl: CompletionStrategyDecl = toml::from_str(
            r#"
                poll_method = "ex.wait"
                state_field = "state"
                terminal_states = ["idle"]
                interval_ms = 250
            "#,
        )
        .unwrap();
        let mut strategies = HashMap::new();
        strategies.insert("com.example.x/wait-ready".to_string(), decl);
        let plan = build_auto_wait_plan(&aw, &Map::new(), &strategies).unwrap();
        assert_eq!(plan.polling.state_field, "state");
        assert_eq!(plan.polling.terminal_states, vec!["idle"]);
        assert_eq!(plan.polling.interval_ms, 250);
        assert_eq!(
            plan.polling.timeout_field, None,
            "named-strategy resolution does not carry a CLI --timeout override"
        );
    }

    #[test]
    fn resolve_auto_wait_polling_errors_on_unknown_strategy() {
        // 에러 본문은 i18n 키를 거친다 — en 테이블을 올려 실제 문구(= 키 존재)로 검사한다.
        tasty_i18n::init("en");
        let aw = sample_auto_wait_decl_with_strategy("com.example.x/wait-ready");
        let err = build_auto_wait_plan(&aw, &Map::new(), &empty_strategies())
            .unwrap_err()
            .to_string();
        assert!(err.contains("unknown strategy"), "got: {err}");
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
