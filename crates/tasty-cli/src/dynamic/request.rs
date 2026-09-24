//! clap 매칭 결과를 JSON-RPC 메서드와 인자로 변환한다.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use clap::ArgMatches;
use serde_json::{Map, Value};

use tasty_ipc::protocol::JsonRpcRequest;
use tasty_plugin_manifest::{AutoWaitDecl, CliArgType, CompletionStrategyDecl, PollingDecl};

use super::stdin::{extract_value, merge_stdin_params, read_stdin_json};
use super::{AutoWaitPlan, PluginCliEntry};

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
                            crate::cwd_resolve::normalize_cwd_arg(raw).map_err(|e| {
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
                            crate::cwd_resolve::normalize_file_arg(raw).map_err(|e| {
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
        // 호출자 surface는 대상 surface와 구분하며 호스트 키로 복제하지 않는다.
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

    // CLI의 이름 기반 전략 조회는 아직 연결되지 않았다. 빈 목록으로 넘겨
    // 이름 참조는 unknown strategy로 거절하고 인라인 polling만 처리한다.
    let available_strategies: HashMap<String, CompletionStrategyDecl> = HashMap::new();
    let auto_wait_plan = sub_decl
        .auto_wait
        .as_ref()
        .map(|aw| build_auto_wait_plan(aw, &params, &available_strategies))
        .transpose()?;

    Ok((
        JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
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

/// 인라인 polling 또는 등록된 이름을 PollingDecl로 해석한다.
/// 매니페스트 검증은 둘 중 하나만 허용하며, 이름이 목록에 없으면 오류를 반환한다.
pub(super) fn resolve_auto_wait_polling(
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
pub(super) fn build_auto_wait_plan(
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
