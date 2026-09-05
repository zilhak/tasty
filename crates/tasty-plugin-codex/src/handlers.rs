//! `handle_ipc_method` 내부에서 각 codex.* 메서드를 처리한다.
//!
//! 자식 terminal 관리(spawn/tell/children/parent/kill/respawn/broadcast)는
//! 호스트가 내재화한 `terminal.*` IPC(ADR-0040 / occupancy-04)로 **위임**한다.
//! 이 plugin 은 더 이상 자체 child registry 를 보유하지 않는다(호스트 registry 가
//! 단일 SoT). 여기 남는 것은 codex **특화**뿐:
//! - `make_codex_command` — codex 바이너리 기동 명령 빌더(`--dangerously-bypass-hook-trust`
//!   포함 — hook 이 항상 fire 되게 한다).
//! - install/uninstall/hook — `~/.codex/config.toml` 조작 + trust 판정.
//! - hook 이 산출한 idle/active 신호를 `terminal.set_state` 로 호스트 registry 에 주입하고,
//!   `stop` 이벤트는 `surface.fire_hook`으로 `codex-idle`도 함께 쏜다.
//! - `handle_spawn`/`handle_tell` 이 완료 시(`codex-idle`/`process-exit`) caller 에게
//!   1 회성 알림을 보내는 hook 을 등록한다(`register_notify_hooks`).
//!
//! 모든 호스트 호출은 `host.call(...)`을 통해 동기로 이루어진다.

use serde_json::{Map, Value, json};
use tasty_plugin_agent_common::children::{indices_with, join_indices, state_of};
use tasty_plugin_agent_common::host_call::{HostCall, cleanup_sibling_hooks};
use tasty_plugin_agent_common::params::{TargetSurfaceError, forward, target_surface};
use tasty_plugin_agent_common::prompt_file;
use tasty_plugin_sdk::{HostHandle, IpcMethodError, i18n::Translator};

/// 번역된 틀에 `{토큰}` 을 채운다. `Translator::t_replace` 는 토큰 하나만 받으므로
/// 둘 이상인 문구를 위해 둔다 — 호출자가 `.replace` 사슬을 손으로 쓰면 한 토큰을
/// 빠뜨려도 컴파일이 통과해 `{surface}` 가 그대로 사용자에게 나간다.
pub(crate) fn t_args(tr: &Translator, key: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = tr.t(key).to_string();
    for (token, value) in pairs {
        out = out.replace(token, value);
    }
    out
}

/// 응답 매핑 헬퍼: `HostHandle::call` 결과를 `IpcMethodError` 로 변환.
///
/// **문구를 다시 감싸지 않는다.** `PluginError::HostCall` 의 Display 가 이미
/// `host call '<method>' failed: <message>` 라, 여기서 같은 틀로 한 번 더 감싸면
/// 사용자에게 그 접두가 두 번 나간다(실측: `host call 'terminal.tell' failed:
/// host call 'call#1' failed: no live surface 9999`). 형제 plugin(claude)은
/// 처음부터 `From` 만 쓴다.
fn host_call(host: &HostHandle, method: &str, params: Value) -> Result<Value, IpcMethodError> {
    host.call(method, params).map_err(IpcMethodError::from)
}

/// 필수 u32 파라미터를 읽는다 — **없는 것과 잘못된 것을 가른다.**
///
/// `as u32` 로 자르면 `4_294_967_297` 이 `1` 이 되고 `5_000_000_000` 이
/// `705_032_704` 가 된다. 둘 다 **실재할 수 있는 다른 surface 의 id** 다 — 못 읽는
/// 값이 조용히 남의 터미널로 배달된다. 자르지 말고 거부한다.
///
/// 메시지도 가른다. 키가 아예 없는 것은 호출자가 인자를 빠뜨린 것이고, 값이 왔는데
/// 안 읽히는 것은 오타이거나 타입이 틀린 것이다 — "missing" 이라고 답하면 호출자가
/// 자기가 준 값을 안 의심한다.
pub(crate) fn require_u32(
    params: &Value,
    key: &str,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    let Some(raw) = params.get(key).filter(|v| !v.is_null()) else {
        return Err(IpcMethodError::invalid_params(&tr.t_replace(
            "codex.params.missing",
            "{key}",
            key,
        )));
    };
    raw.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            IpcMethodError::invalid_params(&t_args(
                tr,
                "codex.params.not_a_number",
                &[("{key}", key), ("{raw}", &raw.to_string())],
            ))
        })
}

/// 대상 parent surface — 판정은 [`tasty_plugin_agent_common::params::target_surface`]
/// 한 벌이고, 여기서는 그 실패를 **codex 카탈로그의 문구로** 옮기기만 한다.
///
/// 이 저장소는 같은 물음에 **세 가지 답**을 갖고 있었다: claude 는 `surface_id` 만,
/// 여기 handlers 는 `surface` 만, `reboot.rs` 는 둘 다(먼저 온 것). 세 번째만 옳았고
/// 그것도 두 값이 다를 때를 안 봤다. 판정을 한 벌로 모으는 것이 이 정정의 전부다.
pub(crate) fn optional_target_surface(
    params: &Value,
    tr: &Translator,
) -> Result<Option<u32>, IpcMethodError> {
    target_surface(params).map_err(|e| match e {
        TargetSurfaceError::Malformed { key, raw } => IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.not_a_number",
            &[("{key}", key), ("{raw}", &raw)],
        )),
        TargetSurfaceError::Conflict {
            surface,
            surface_id,
        } => IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.surface_conflict",
            &[
                ("{surface}", &surface.to_string()),
                ("{surface_id}", &surface_id.to_string()),
            ],
        )),
    })
}

pub(crate) fn require_target_surface(
    params: &Value,
    tr: &Translator,
) -> Result<u32, IpcMethodError> {
    optional_target_surface(params, tr)?.ok_or_else(|| {
        IpcMethodError::invalid_params(&tr.t_replace("codex.params.missing", "{key}", "surface"))
    })
}

/// 호스트로 넘길 params 에 대상 surface 를 싣는다 — 실패 문구만 codex 것으로 옮긴다.
///
/// 호출자가 아무 이름도 안 줬으면 아무것도 안 싣는다: 호스트의 유일-parent 폴백이
/// 곧 `--surface` 생략 동작이라, 여기서 값을 지어내면 그 동작이 사라진다.
fn put_target_surface(
    dst: &mut serde_json::Map<String, Value>,
    params: &Value,
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    if let Some(surface) = optional_target_surface(params, tr)? {
        dst.insert("surface".into(), json!(surface));
    }
    Ok(())
}

fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(String::from)
}

/// codex 명령을 PTY로 보낼 문자열을 만든다. prompt가 있으면 임시 파일에 써서
/// `"$(cat '<path>')"` 로 주입한다.
///
/// prompt 를 직접 커맨드 문자열에 inline 하지 않는 이유: 이 문자열은 `surface.send`
/// 로 child PTY 에 **문자 그대로 타이핑**되므로, child 셸이 zsh 면 인터랙티브
/// history expansion 대상이 된다 — `!` 로 시작하는 텍스트(예: 마크다운 콜아웃
/// `[!NOTE]`)가 있으면 `zsh: event not found: NOTE]` 로 그 줄 자체가 깨지고, 남은
/// prompt 줄들이 codex 인자가 아니라 개별 셸 명령으로 실행되는 연쇄 실패로
/// 이어진다(실제 재현됨). zsh 의 history expansion 은 **큰따옴표 안에서도 적용**되므로
/// escape 로는 막을 수 없다 — 텍스트 자체가 타이핑되는 줄에 아예 나타나지 않게
/// 해야 한다. claude plugin 의 `claude_launch_command_with_prompt` 와 동일 패턴.
/// 파일 쓰기 실패는 warn 후에도 계속 진행한다(빈 프롬프트로라도 기동은 시도).
///
/// `TASTY_SURFACE_ID={surface_id}` inline env prefix를 항상 박는다. 이게 없으면
/// codex 프로세스 env에 `TASTY_SURFACE_ID`가 비어, `~/.codex/config.toml`의 hook
/// 명령 (`tasty codex hook X --surface $TASTY_SURFACE_ID`)이 surface ID 없이
/// 실행되어 `handle_hook`이 invalid_params로 거부 → idle/needs_input 상태가 영원히
/// 갱신되지 않는다. claude plugin의 `start_claude_in_surface`와 동일한 패턴.
///
/// `--dangerously-bypass-hook-trust` 는 사용자가 `/hooks` 로 수동 승인하기 전에도
/// tasty 가 install 한 hook 이 항상 fire 되게 한다. tasty 는 자기 hook을 스스로
/// 심으므로(hook source 를 스스로 vet함) 이 플래그의 정당한 사용 대상이다 —
/// 이게 없으면 codex 가 hook 을 fire 하지 않아 `codex-idle` 알림이 영원히 오지
/// 않는다.
///
/// `policy_args` 는 [`resolve_policy_args`] 가 만든 `-a ...`/`-s ...`/
/// `--dangerously-bypass-approvals-and-sandbox` 조각(또는 빈 문자열)이다 — 승인
/// 프롬프트가 자동화 흐름에서 자식을 영구히 멈추게 하는 문제
/// (docs/plugins/codex/index.md 의 승인/샌드박스 정책 플래그 절 참조)의 해결책.
/// prompt 임시파일 이름 prefix. 청소 스윕(`prompt_file::sweep_stale`)이 같은 패턴으로
/// 자기 파일만 매칭하도록 상수로 뽑는다. claude 쪽(`tasty-prompt-{surface_id}.txt`)과
/// 파일명이 겹치지 않도록 codex 전용 prefix 를 쓴다 — 두 plugin 이 같은 surface_id 로
/// 동시에 다른 자식(claude/codex)을 spawn 할 수 있다. suffix·TTL·쓰기·스윕은
/// `tasty-plugin-agent-common` 이 갖고, **prefix 만** 여기 남는다.
const PROMPT_FILE_PREFIX: &str = "tasty-codex-prompt-";
/// 파일 정리 시점: 자식이 `$(cat ...)` 로 이 파일을 다 읽은 순간을 tasty 가 알
/// 방법이 없다(`surface.send` 는 fire-and-forget 텍스트 주입) — 쓰자마자 지우면
/// 아직 안 읽은 자식과 레이스한다. 대신 매 spawn 마다 TTL 을 넘긴 이전 파일들을
/// 먼저 청소한다(`prompt_file::sweep_stale`) — 지연 삭제.
/// 권한은 생성 시점부터 0600(owner-only, Unix) 으로 좁힌다 — 생성 후 별도
/// `chmod` 로 좁히면 그 사이 기본 권한(보통 0644)으로 잠깐 노출되는 TOCTOU 창이
/// 생기므로, `OpenOptions`(Unix `mode`)로 처음부터 좁게 만든다.
fn make_codex_command(surface_id: u32, prompt: Option<&str>, policy_args: &str) -> String {
    let prefix = format!("TASTY_SURFACE_ID={surface_id} ");
    let policy_suffix = if policy_args.is_empty() {
        String::new()
    } else {
        format!(" {policy_args}")
    };
    match prompt {
        Some(p) if !p.is_empty() => {
            let temp_dir = std::env::temp_dir();
            prompt_file::sweep_stale(&temp_dir, PROMPT_FILE_PREFIX);
            let prompt_path = prompt_file::path_for(&temp_dir, PROMPT_FILE_PREFIX, surface_id);
            if let Err(e) = prompt_file::write(&prompt_path, p) {
                tracing::warn!("Failed to write codex prompt file: {e}");
            }
            format!(
                "{prefix}codex --dangerously-bypass-hook-trust{policy_suffix} \"$(cat '{}')\"\r",
                prompt_path.display()
            )
        }
        _ => format!("{prefix}codex --dangerously-bypass-hook-trust{policy_suffix}\r"),
    }
}

const VALID_APPROVAL_POLICIES: &[&str] = &["untrusted", "on-request", "never"];
const VALID_SANDBOX_MODES: &[&str] = &["read-only", "workspace-write", "danger-full-access"];

fn validate_choice(
    flag_name: &str,
    value: &str,
    valid: &[&str],
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    if valid.contains(&value) {
        Ok(())
    } else {
        Err(IpcMethodError::invalid_params(&t_args(
            tr,
            "codex.params.invalid_choice",
            &[
                ("{flag}", flag_name),
                ("{value}", value),
                ("{valid}", &valid.join(", ")),
            ],
        )))
    }
}

/// 전역 설정(`default_approval_policy`/`default_sandbox_mode`)에서 fallback 값을
/// 읽는다. 미설정이거나 `"inherit"`이면 None.
fn global_policy_default<H: HostCall>(host: &H, storage_key: &str) -> Option<String> {
    host.call(
        "settings.get_plugin_setting",
        json!({ "storage_key": storage_key }),
    )
    .ok()
    .and_then(|v| v.get("value").and_then(|v| v.as_str()).map(String::from))
    .filter(|s| s != "inherit" && !s.is_empty())
}

/// `--approval`/`--sandbox`/`--full-auto` 요청 params 를 codex CLI 인자 조각으로
/// 해석한다. 우선순위: 호출별 명시 파라미터 > 전역 설정(`default_approval_policy`/
/// `default_sandbox_mode`) > **하드코드 기본값**.
///
/// **승인(`approval`)은 결정되지 않으면 무조건 `never`로 떨어진다** — tasty 가 spawn 하는
/// codex 자식은 전부 완료 알림(idle/exit hook)만 기다리는 무인 자동화 흐름이고, codex 에는
/// needs_input 류 hook 이 없어 승인 프롬프트가 뜨면 아무도 응답할 수 없는 채로 영구 정지한다
/// (기본값이 무해하지 않으면 이 정지가 그대로 재현된다 — docs/plugins/codex/index.md 의
/// 승인/샌드박스 정책 플래그 절 참조). "설정을 안 건드리면 codex 자체 인터랙티브 기본값을
/// 쓴다"는 옛 의미는 더 이상 유효하지 않다 — 인터랙티브 승인이 필요하면 호출자가 `--approval
/// untrusted`/`on-request` 를 **명시적으로** 넘겨야 한다.
/// 샌드박스(`sandbox`)는 승인과 달리 결정 안 됐다고 자체적으로 멈추는 축이 아니므로(그 자체는
/// 프롬프트를 띄우지 않는다) 기존대로 미설정 시 플래그를 아예 안 붙여 codex 자체 기본값을 쓴다
/// — 단 이 프로젝트 sandbox(bubblewrap 기반 user namespace 격리)에서는 `read-only`/
/// `workspace-write` 처럼 codex 자체 샌드박스를 켜는 값이 중첩 샌드박스 환경에서 실패할 수 있어
/// (`RTM_NEWADDR: Operation not permitted` 류), 그런 환경의 호출자는 `full_auto`(샌드박스까지
/// 완전 우회)를 명시적으로 골라야 한다.
///
/// `full_auto` 는 `--dangerously-bypass-approvals-and-sandbox` 로 승인/샌드박스를
/// 완전히 우회한다 — `approval`/`sandbox` 와 동시에 오면 모순(둘 다 우회하면서
/// 개별 정책을 지정하는 셈)이므로 명시적으로 거부해 호출자의 의도 오해를 막는다.
pub(crate) fn resolve_policy_args<H: HostCall>(
    host: &H,
    params: &Value,
    tr: &Translator,
) -> Result<String, IpcMethodError> {
    let full_auto = params
        .get("full_auto")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let approval = optional_str(params, "approval");
    let sandbox = optional_str(params, "sandbox");

    if full_auto {
        if approval.is_some() || sandbox.is_some() {
            return Err(IpcMethodError::invalid_params(
                tr.t("codex.params.full_auto_conflict"),
            ));
        }
        return Ok("--dangerously-bypass-approvals-and-sandbox".to_string());
    }

    let approval = match approval {
        Some(v) => {
            validate_choice("approval", &v, VALID_APPROVAL_POLICIES, tr)?;
            Some(v)
        }
        None => Some(
            global_policy_default(host, "default_approval_policy")
                .unwrap_or_else(|| "never".to_string()),
        ),
    };
    let sandbox = match sandbox {
        Some(v) => {
            validate_choice("sandbox", &v, VALID_SANDBOX_MODES, tr)?;
            Some(v)
        }
        None => global_policy_default(host, "default_sandbox_mode"),
    };

    let mut parts = Vec::new();
    if let Some(a) = approval {
        parts.push(format!("-a {a}"));
    }
    if let Some(s) = sandbox {
        parts.push(format!("-s {s}"));
    }
    Ok(parts.join(" "))
}

pub fn handle_launch(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let workspace_name = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("codex")
        .to_string();
    let directory = optional_str(&params, "directory");
    let task = optional_str(&params, "task");

    // cwd 는 CLI 가 absolute path 로 정규화 + 검증해 전달 (path_kind hint).
    // 호스트 workspace.create 가 PTY working_dir 로 직접 사용 → `cd` echo 불필요.
    let mut ws_params = Map::new();
    ws_params.insert("name".into(), Value::String(workspace_name.clone()));
    ws_params.insert("type".into(), Value::String("terminal".into()));
    if let Some(dir) = directory.as_deref() {
        ws_params.insert("cwd".into(), Value::String(dir.to_string()));
    }
    let ws_result = host_call(host, "workspace.create", Value::Object(ws_params))?;
    let workspace_id = ws_result
        .get("id")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.launch.workspace_create_missing_id",
                "{resp}",
                &ws_result.to_string(),
            ))
        })? as u32;
    let surface_id = ws_result
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);

    if let Some(sid) = surface_id {
        let policy_args = resolve_policy_args(host, &params, tr)?;
        let cmd = make_codex_command(sid, task.as_deref(), &policy_args);
        host_call(
            host,
            "surface.send",
            json!({"surface_id": sid, "text": cmd}),
        )?;
    }

    Ok(json!({
        "workspace_id": workspace_id,
        "workspace_name": workspace_name,
        "surface_id": surface_id,
    }))
}

pub fn handle_parent(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    // 호스트 registry 가 parent 매핑의 SoT — 그대로 위임.
    let surface = require_target_surface(&params, tr)?;
    host_call(host, "terminal.parent", json!({ "surface": surface }))
}

/// 자식 surface 단건 상태 조회 — 호스트 `terminal.state` 로 위임.
/// `codex` namespace 안에 두는 이유는 완료 판정 전략의 `poll_method` 가 owner
/// namespace 밖을 참조할 수 없어서다(결정 2) — `codex.spawn` 기본 전략이 이
/// 메서드를 poll_method 로 참조한다(매니페스트 `[[contributes.completion_strategy]]`).
pub fn handle_state(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = require_target_surface(&params, tr)?;
    host_call(host, "terminal.state", json!({ "surface": surface }))
}

pub fn handle_tell(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface_id = require_target_surface(&params, tr)?;
    let message = params
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_message")))?;
    // 개행/제출 규칙(단일라인 평문 / 멀티라인 bracketed paste + 별도 `\r`)은 호스트
    // `terminal.tell` 이 동일하게 처리한다 → 본문 포맷을 재구현하지 않고 위임.
    let resp = host_call(
        host,
        "terminal.tell",
        json!({ "surface": surface_id, "text": message }),
    )?;

    // caller_surface 는 dynamic CLI 가 `TASTY_SURFACE_ID` 로 자동 채운다(명시
    // --caller-surface 도 허용). 없으면(예: 호스트가 직접 IPC 호출) 완료 알림을
    // 등록하지 않는다 — 누구에게 알릴지 모르므로.
    if let Ok(caller) = require_u32(&params, "caller_surface", tr) {
        register_notify_hooks(host, surface_id, caller, "tell");
    }

    Ok(resp)
}

pub fn handle_spawn(
    host: &HostHandle,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let parent_surface = require_target_surface(&params, tr)?;
    let prompt = optional_str(&params, "prompt");

    // 1) 호스트 registry 에 자식 등록 + soft 점유 + tab 생성 (command 미전송).
    //    workspace 는 required — 없으면 호스트가 invalid_params 로 거부한다.
    let mut sp = forward(&params, &["workspace", "pane", "cwd", "role", "nickname"]);
    sp.insert("parent".into(), json!(parent_surface));
    let resp = host_call(host, "terminal.spawn", Value::Object(sp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.spawn.missing_child_surface_id",
                "{resp}",
                &resp.to_string(),
            ))
        })?;

    // 2) codex 특화 기동 명령을 그 surface 에 전송(surface_id inline env 필요).
    let policy_args = resolve_policy_args(host, &params, tr)?;
    let cmd = make_codex_command(child_sid, prompt.as_deref(), &policy_args);
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    // 3) 완료 시(codex-idle/process-exit) parent 에게 1 회성 알림 등록.
    register_notify_hooks(host, child_sid, parent_surface, "spawn");

    // 4) child 개수 임계치 경고(soft) — spawn 자체를 막지 않는다.
    let mut out = resp;
    if let Some(warning) = compute_spawn_warning(host, tr, parent_surface) {
        if let Some(obj) = out.as_object_mut() {
            obj.insert("warning".into(), json!(warning));
        }
    }

    Ok(out)
}

/// 완료 알림 hook 의 command 문자열 — 등록 시점과 fire 후 정리 시점이 **정확히 같은
/// 값**을 만들어야 command 일치 정리가 성립한다. 형제(codex-idle/process-exit)는 모두
/// 이 동일 문자열을 command 로 갖는다.
fn notify_caller_command(caller_surface: u32, target_surface: u32, kind: &str) -> String {
    format!(
        "tasty codex notify-caller --caller {caller_surface} --target {target_surface} --kind {kind}"
    )
}

/// caller 에게 보여줄 완료 알림 문구 — "그 child 가 맡은 작업이 끝났다"를 앞세운다.
/// 과거 `"{kind} 완료: surface {target}"` 형태는 spawn/tell 자체가(호출이) 완료됐다는
/// 뜻으로 오독되기 쉬워, conductor 가 실제 작업 완료 알림을 "spawn 접수 확인" 정도로
/// 여기고 계속 무시하는 사고로 이어졌다. `kind`는 호출 방식(spawn/tell)일 뿐 완료의
/// 주어가 아니므로 괄호로 분리한다(tasty-plugin-claude 의 `notify_done_message`와 동형).
fn notify_caller_message(tr: &Translator, kind: &str, target: u32) -> String {
    tr.t("codex.notify.done_message")
        .replace("{target}", &target.to_string())
        .replace("{kind}", kind)
}

/// 샌드박스(bwrap) 초기화 실패 감지 마커 — 오탐 최소화를 위해 특이도가 가장 높은
/// 토큰만 본다. `"bwrap:"`만 쓰면 일반 대화 텍스트에 우연히 매치될 여지가 있지만,
/// `RTM_NEWADDR`는 사실상 이 실패 상황에서만 등장한다.
const SANDBOX_FAILURE_MARKER: &str = "RTM_NEWADDR";

/// 힌트 문구의 번역 키 — `docs/plugins/codex/index.md`(샌드박스 초기화 실패 패턴과
/// 수동 우회법 `--full-auto` 가 이미 문서화된 곳)의 안내를 완료 알림 채널에 요약해
/// 싣는다.
///
/// 이 문구가 나가는 곳은 `<parent_home>/notify/*.log` 이고 읽는 쪽은 **호출한
/// 에이전트**다. 사람이 보는 CLI stdout/stderr 표면은 아니지만, 그렇다고 언어를 코드에
/// 박아 둘 자리도 아니다 — 형제 plugin(claude)은 같은 채널의 같은 성격 문구를
/// `claude.notify.done_message` 로 번역해 내보낸다. 두 문구가 같은 파일에 섞이면
/// 읽는 쪽이 한 채널에서 두 언어를 받는다.
const SANDBOX_FAILURE_HINT_KEY: &str = "codex.notify.sandbox_hint";

/// `screen_text`(대상 surface 의 최근 출력)에서 샌드박스 초기화 실패 시그니처를
/// 찾으면 힌트 문구를 반환한다. best-effort 탐지 — 순수 함수라 단위 테스트 대상.
fn detect_sandbox_failure_hint(tr: &Translator, screen_text: &str) -> Option<String> {
    if screen_text.contains(SANDBOX_FAILURE_MARKER) {
        Some(tr.t(SANDBOX_FAILURE_HINT_KEY).to_string())
    } else {
        None
    }
}

/// 완료 알림 본문에 (있으면) 샌드박스 실패 힌트를 덧붙인다. `screen_text`가
/// `None`(조회 자체가 실패 — soft-fail)이거나 마커가 없으면 `base`를 그대로
/// 돌려준다(회귀 없음). 순수 함수 — 단위 테스트 대상.
fn append_sandbox_hint_if_detected(
    tr: &Translator,
    base: String,
    screen_text: Option<&str>,
) -> String {
    match screen_text.and_then(|s| detect_sandbox_failure_hint(tr, s)) {
        Some(hint) => format!("{base} {hint}"),
        None => base,
    }
}

/// 힌트 탐지용으로 스캔할 최근 화면 줄 수 — 실제 관찰된 사례에서 샌드박스 실패
/// 메시지가 대화 초반(수백 줄 전)에 찍히고 그 뒤로 긴 응답이 이어졌다. 완벽한
/// 탐지가 목표가 아니므로(놓치면 힌트가 안 붙을 뿐, 기존 동작에 위해 없음)
/// 넉넉한 값으로 시작한다.
const SCREEN_TEXT_SCAN_LINES: u64 = 800;

/// 힌트 탐지를 위해 대상 surface 의 최근 화면 출력을 조회한다. soft-fail —
/// `surface.screen_text` 호출 자체가 실패해도(예: 대상 surface 가 이미 사라짐)
/// `None`을 돌려줄 뿐 알림 전송에는 영향 없다(`compute_spawn_warning`과 동일
/// 패턴). `TerminalRead` 권한은 이미 codex 플러그인이 보유하고 있어 신규 권한
/// 부여가 필요 없다.
fn fetch_screen_text_for_hint<H: HostCall>(host: &H, target: u32) -> Option<String> {
    host.call(
        "surface.screen_text",
        json!({ "surface_id": target, "lines": SCREEN_TEXT_SCAN_LINES }),
    )
    .ok()
    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_string))
}

/// child(=target) 가 완료(codex-idle 또는 process-exit)되면 caller 에게 1 회성 알림을
/// 보내도록 hook 2개를 등록한다. 두 hook 의 command 는 완전히 동일한 `codex
/// notify-caller` 호출이며, fire 시점에 `hook.list` 를 command 문자열로 매칭해 자기
/// 그룹의 남은 형제를 정리한다 — 어느 이벤트가 먼저 fire 하는지에 무관하게 대칭적으로
/// 동작하고, 상태(단일 meta 슬롯)를 공유하지 않아 같은 surface 에 spawn/tell 이 겹쳐
/// 등록돼도 서로의 형제를 덮어써 좀비로 남기지 않는다. host 호출 실패는 경고만 하고
/// 넘어간다(soft — spawn/tell 성공을 막지 않음).
fn register_notify_hooks<H: HostCall>(
    host: &H,
    target_surface: u32,
    caller_surface: u32,
    kind: &str,
) {
    let cmd = notify_caller_command(caller_surface, target_surface, kind);
    for event in ["codex-idle", "process-exit"] {
        if let Err(e) = host.call(
            "hook.set",
            json!({ "surface_id": target_surface, "event": event, "command": cmd, "once": true }),
        ) {
            tracing::warn!("codex notify hook.set '{event}' failed: {e}");
        }
    }
}

/// `register_notify_hooks` 가 등록한 hook 이 fire 되면 실행되는 핸들러. caller
/// 에게 완료 알림을 보내고, 형제 once-hook(자신 포함)을 함께 정리한다. 자신은
/// once 시맨틱으로 이미 자동 제거된 뒤이므로 unset 이 no-op 이어도 무해하다 —
/// "누가 먼저 fire했는지" 판별이 전혀 필요 없다. 정리는 `hook.list`(surface 필터) +
/// command 문자열 일치로 하며, 상태(단일 meta 슬롯)를 공유하지 않아 같은 surface 에
/// spawn/tell 이 겹쳐 등록돼도 서로의 형제를 덮어써 좀비로 남기지 않는다.
pub fn handle_notify_caller<H: HostCall>(
    host: &H,
    tr: &Translator,
    params: Value,
) -> Result<Value, IpcMethodError> {
    let caller = require_u32(&params, "caller", tr)?;
    let target = require_u32(&params, "target", tr)?;
    let kind = optional_str(&params, "kind").unwrap_or_else(|| "tell".into());
    let message = notify_caller_message(tr, &kind, target);
    // 샌드박스 초기화 실패 힌트(docs/plugins/codex/index.md 의 샌드박스 초기화 실패 힌트
    // 절 참조) — soft-fail, 조회 실패/미탐지 시 message 그대로.
    let screen_text = fetch_screen_text_for_hint(host, target);
    let message = append_sandbox_hint_if_detected(tr, message, screen_text.as_deref());

    // 완료 로그 파일에 append — conductor 가 Monitor tool 로 tail 하면 busy/idle 여부와
    // 무관하게 다음 턴에 전달된다. 완료 알림의 유일한 경로다(과거엔 terminal.tell 도
    // 함께 발사했으나, 자동 이벤트가 실제 사용자 발화처럼 대화 트랜스크립트에 섞여
    // 들어가는 부작용 때문에 제거함). best-effort — 실패해도 hook 정리에 영향 없음.
    if let Err(e) = tasty_utils::notify::append_notify_line(caller, &message) {
        tracing::warn!("codex notify-caller completion-log append failed: {e}");
    }

    // 자기 그룹(같은 command)의 남은 형제 정리 — surface 필터 + command 일치.
    let expected_command = notify_caller_command(caller, target, &kind);
    cleanup_sibling_hooks(host, target, &expected_command);

    // target 이 아직 살아있다면(이번 fire 가 process-exit 가 아니었다면) 형제 hook 을
    // 다시 등록해 다음 idle 전환에도 알림이 오도록 자기재무장한다 — "spawn/tell 당
    // 알림 1회" 가 아니라 "child 가 살아있는 동안 상태 전환마다 알림"으로 바뀐다.
    rearm_if_still_alive(host, caller, target, &kind);

    Ok(json!({}))
}

/// `target` 이 host 트리에 여전히 존재하면(=이번 fire 가 process-exit 가 아니었다면)
/// 형제 hook(codex-idle/process-exit)을 재등록한다. `surface.locate` 로 생존을
/// 판별하는 이유: process-exit 로 fire 된 경우 host 는 hook 발화 직후 동기로 그
/// surface 를 이미 닫으므로(`close_surface_by_id_no_snapshot`), 이 시점에 조회하면
/// 사라져 있다 — 반대로 codex-idle 은 surface 가 살아있는 상태에서만 발생하는
/// 이벤트라 재등록이 안전하다. 조회 실패(best-effort)는 "죽었다"로 간주해 재등록을
/// 건너뛴다 — 좀비 hook 을 쌓는 것보다 드물게 재무장을 놓치는 쪽이 안전하다.
fn rearm_if_still_alive<H: HostCall>(host: &H, caller: u32, target: u32, kind: &str) {
    let alive = host
        .call("surface.locate", json!({ "surface_id": target }))
        .ok()
        .and_then(|r| r.get("exists").and_then(|v| v.as_bool()))
        .unwrap_or(false);
    if alive {
        // register_notify_hooks 는 (host, target_surface, caller_surface, kind) 순서다
        // (claude 쪽과 인자 순서가 다르므로 주의).
        register_notify_hooks(host, target, caller, kind);
    }
}

const DEFAULT_SPAWN_CHILD_WARN_THRESHOLD: f64 = 6.0;

/// spawn 직후 parent 의 현재 child 목록/상태를 재조회해 임계치 초과 여부를 판단한다.
/// host 호출 실패는 경고 생략으로 처리한다(soft 경고이므로 spawn 성공을 막지 않음).
fn compute_spawn_warning(
    host: &HostHandle,
    tr: &Translator,
    parent_surface_id: u32,
) -> Option<String> {
    let children_resp = host
        .call("terminal.children", json!({ "surface": parent_surface_id }))
        .ok()?;
    let children = children_resp.get("children")?.as_array()?;
    let total = children.len();
    let idle_indices = indices_with(children, |c| state_of(c) == Some("idle"));
    // `stale` 은 확정(`foreground_is_shell`)인 것만 센다 — `heuristic` stale 은
    // SIGSTOP·긴 추론·무출력 명령과 관측상 구별되지 않아, 그것까지 "respawn 후보"
    // 로 부르면 일하는 자식을 재시작하라고 권하게 된다. `docs/dev-guide/
    // api-conventions.md` 가 같은 이유로 `stale` 을 기본 terminal state 집합에서
    // 뺀 것과 동일한 판단이다.
    let stale_indices = indices_with(children, |c| {
        state_of(c) == Some("stale")
            && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
    });

    let threshold = host
        .call(
            "settings.get_plugin_setting",
            json!({ "storage_key": "spawn_child_warn_threshold" }),
        )
        .ok()
        .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_SPAWN_CHILD_WARN_THRESHOLD);

    build_spawn_warning(tr, total, &idle_indices, &stale_indices, threshold)
}

/// child 개수가 임계치를 넘으면 경고 문구를 만든다(순수 함수, 단위 테스트 대상).
///
/// 재사용 후보를 **두 목록으로 나눈다.** 둘 다 respawn 대상이지만 근거가 다르다:
/// `idle` 은 자식이 hook 으로 완료를 직접 보고한 값이고, 확정 `stale` 은 보고가 오지
/// 않은 채 호스트 관측이 "전경이 셸로 돌아왔다" 를 잡아낸 값이다(hook 유실 —
/// ADR-0072 가 겨냥한 시나리오). 후자에 "have already finished their work" 를 쓰면
/// 자식이 그렇게 보고한 적 없는데 보고한 것처럼 읽히므로 문구를 분리한다.
///
/// 문구 자체는 `lang/{en,ko,ja}.toml` 의 `codex.spawn_warning.*` 에 있다 — 이 문자열은
/// `tasty codex spawn` 응답에 실려 CLI stdout 으로 그대로 나가는 사람이 읽는 표면이라
/// `docs/dev-guide/i18n.md` 의 하드코딩 허용 예외 어디에도 해당하지 않는다. plugin
/// process 는 호스트 카탈로그에 접근할 수 없으므로 SDK `Translator`(자기 `lang/` 로드)를
/// 쓴다.
fn build_spawn_warning(
    tr: &Translator,
    total: usize,
    idle_indices: &[u64],
    stale_indices: &[u64],
    threshold: f64,
) -> Option<String> {
    if (total as f64) <= threshold {
        return None;
    }
    let mut msg = tr
        .t_replace("codex.spawn_warning.total", "{total}", &total.to_string())
        .replace("{threshold}", &threshold.to_string());
    if !idle_indices.is_empty() {
        msg.push_str(&tr.t_replace(
            "codex.spawn_warning.idle",
            "{indices}",
            &join_indices(idle_indices),
        ));
    }
    if !stale_indices.is_empty() {
        msg.push_str(&tr.t_replace(
            "codex.spawn_warning.stale",
            "{indices}",
            &join_indices(stale_indices),
        ));
    }
    Some(msg)
}

pub fn handle_children(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let mut cp = serde_json::Map::new();
    put_target_surface(&mut cp, &params, tr)?;
    host_call(host, "terminal.children", Value::Object(cp))
}

pub fn handle_broadcast(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_text")))?;
    let mut bp = forward(&params, &["role"]);
    put_target_surface(&mut bp, &params, tr)?;
    bp.insert("text".into(), json!(text));
    host_call(host, "terminal.broadcast", Value::Object(bp))
}

pub fn handle_kill(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let child = require_u32(&params, "child", tr)?;
    let mut kp = serde_json::Map::new();
    put_target_surface(&mut kp, &params, tr)?;
    kp.insert("child".into(), json!(child));
    host_call(host, "terminal.kill", Value::Object(kp))
}

pub fn handle_respawn(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let child = require_u32(&params, "child", tr)?;
    let prompt = optional_str(&params, "prompt");

    // 1) 호스트 registry 위임: cwd 있으면 PTY 교체, 없으면 Ctrl-C. role/nickname/cwd
    //    갱신 + idle 초기화까지 호스트가 수행하고 child_surface_id 를 돌려준다.
    //    codex 기동은 여기서 하지 않으므로 command 는 넘기지 않는다.
    let mut rp = forward(&params, &["cwd", "role", "nickname"]);
    put_target_surface(&mut rp, &params, tr)?;
    rp.insert("child".into(), json!(child));
    let resp = host_call(host, "terminal.respawn", Value::Object(rp))?;
    let child_sid = resp
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            IpcMethodError::new(tr.t_replace(
                "codex.respawn.missing_child_surface_id",
                "{resp}",
                &resp.to_string(),
            ))
        })?;

    // 2) codex 특화 기동 명령 재전송.
    let policy_args = resolve_policy_args(host, &params, tr)?;
    let cmd = make_codex_command(child_sid, prompt.as_deref(), &policy_args);
    host_call(
        host,
        "surface.send",
        json!({"surface_id": child_sid, "text": cmd}),
    )?;

    Ok(resp)
}

/// Codex CLI hook event 가 fire 됐을 때 호출. install 이 박은 `Stop` /
/// `UserPromptSubmit` / `SessionStart` 만 정상 처리한다. idle/active 신호를
/// 호스트 registry(`terminal.set_state`)에 주입한다 — 자체 state 는 없다.
///
/// **반환값**: 빈 객체 `{}`. CLI 의 stdout 으로 흘러나가 codex 가 직접 파싱하므로
/// codex 의 wire schema 와 호환되어야 한다. 모든 필드가 optional 이므로 empty
/// object 는 "no decision, continue normally" 의미.
pub fn handle_hook(
    host: &HostHandle,
    params: Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let event = params
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| IpcMethodError::invalid_params(tr.t("codex.params.missing_event")))?;
    let surface_id = require_target_surface(&params, tr)
        .map_err(|_| IpcMethodError::invalid_params(tr.t("codex.hook.requires_surface")))?;
    let new_state = hook_event_to_state(event, tr)?;
    // session-start 에 session id(stdin JSON `session_id` → CLI `--session`)가
    // 오면 reboot/복원용 세션 meta 를 기록한다. codex 에는 SessionEnd hook 이
    // 없어 unset 경로는 없다 — 다음 session-start 가 덮어쓴다. resume 기동도
    // source=resume 인 session-start 를 같은 session_id 로 다시 fire 한다(실측).
    if event == "session-start"
        && let Some(session) = params.get("session").and_then(|v| v.as_str())
        && !session.is_empty()
    {
        for (key, value) in [
            ("codex-session-id", session.to_string()),
            ("restore.command", format!("codex resume {session}")),
        ] {
            if let Err(e) = host.call(
                "surface.meta.set",
                json!({ "surface_id": surface_id, "key": key, "value": value }),
            ) {
                tracing::warn!("codex hook meta.set '{key}' failed: {e}");
            }
        }
    }
    host_call(
        host,
        "terminal.set_state",
        json!({ "surface": surface_id, "state": new_state }),
    )?;
    // stop → idle 은 완료 신호이기도 하므로 `codex-idle` surface hook 도 함께
    // 쏜다 — `register_notify_hooks` 로 등록된 1 회성 알림이 이걸 구독한다.
    if event == "stop"
        && let Err(e) = host.call(
            "surface.fire_hook",
            json!({ "surface_id": surface_id, "event": "codex-idle" }),
        )
    {
        tracing::warn!("codex hook fire_hook 'codex-idle' failed: {e}");
    }
    Ok(json!({}))
}

/// codex hook event → 호스트 registry state 매핑(순수 함수, 단위 테스트 가능).
fn hook_event_to_state(event: &str, tr: &Translator) -> Result<&'static str, IpcMethodError> {
    match event {
        "stop" => Ok("idle"),
        "prompt-submit" | "session-start" => Ok("active"),
        other => Err(IpcMethodError::invalid_params(&tr.t_replace(
            "codex.hook.unknown_event",
            "{event}",
            other,
        ))),
    }
}

pub fn handle_install(tr: &Translator) -> Result<Value, IpcMethodError> {
    let path = codex_config_toml_path(tr)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            IpcMethodError::new(tr.t_replace(
                "codex.install.mkdir_failed",
                "{detail}",
                &e.to_string(),
            ))
        })?;
    }
    let existing = read_toml_or_default(&path);
    let merged = merge_install(existing);
    write_toml(&path, &merged, tr)?;
    let trusted = codex_hooks_all_trusted();
    let mut resp = json!({
        "installed": true,
        "path": path.to_string_lossy(),
        "trust_status": if trusted { "trusted" } else { "needs_review" },
    });
    if !trusted {
        resp["note"] = Value::String(
            "Codex blocks newly-added hooks until trusted, but tasty starts every codex instance \
with `--dangerously-bypass-hook-trust` (spawn/launch/reboot), so hooks fire regardless of this \
status. Manual trust is only needed if you run `codex` yourself without that flag. To trust \
manually: run `codex` in any terminal, type `/hooks` + Enter, then for each of 3 hooks press \
Enter → t → Esc → Down. Trust persists per-machine."
                .into(),
        );
    }
    Ok(resp)
}

pub fn handle_uninstall(tr: &Translator) -> Result<Value, IpcMethodError> {
    let path = codex_config_toml_path(tr)?;
    if !path.exists() {
        return Ok(json!({ "uninstalled": true, "path": path.to_string_lossy(), "noop": true }));
    }
    let existing = read_toml_or_default(&path);
    let cleaned = remove_install(existing);
    write_toml(&path, &cleaned, tr)?;
    Ok(json!({ "uninstalled": true, "path": path.to_string_lossy() }))
}

// ───── install/uninstall helpers ─────
//
// Codex CLI 0.130 의 hook 설정은 `~/.codex/config.toml` 의 `[hooks]` 섹션에 박는다.
// 이전 구현은 `~/.codex/settings.json` 에 썼으나 codex 가 그 파일은 *external agent
// config migration* (Claude Code 호환용) 경로에서만 읽고 hook dispatch 에는 쓰지
// 않는다. 그래서 install 했어도 hook 이 한 번도 fire 되지 않았다.
//
// TOML 스키마 (binary strings + 실 동작 검증):
//
// ```toml
// [[hooks.Stop]]                   # MatcherGroup 배열 entry
// # matcher = "..."                # PreToolUse 등에서 tool name regex. Stop 은 omit.
//
// [[hooks.Stop.hooks]]             # HookHandlerConfig 배열
// type = "command"                 # internally tagged enum 의 discriminator
// command = "..."
// # timeout = 5                    # optional, 초 단위
// # async = false                  # optional
// ```
//
// Codex 가 지원하는 event: Stop, PreToolUse, PostToolUse, PermissionRequest,
// PreCompact, PostCompact, SessionStart, UserPromptSubmit. tasty 는 idle/active
// 트래킹에 필요한 3 개만 박는다 (Stop, UserPromptSubmit, SessionStart).
//
// Trust gate: codex 는 새 hook entry 를 *trust* 하기 전엔 fire 하지 않고 TUI 에
// "1 hook needs review" 표시 후 `/hooks` 명령 승인을 요구한다 (`HookStateToml`
// 의 `trusted_hash` 메커니즘). install 자체는 멱등하게 entry 를 박지만, 승인
// 없이는 hook 이 fire 되지 않는다 — **단, `--dangerously-bypass-hook-trust`
// CLI 플래그(codex 공식 옵션)를 기동 명령에 박으면 이 승인 절차를 우회할 수
// 있다**(`make_codex_command`/`reboot::resume_command` 가 항상 이 플래그를
// 붙인다). tasty 는 자기 hook 을 스스로 심으므로(hook source 를 스스로 vet함)
// 이 플래그의 정당한 사용 대상이다. `codex_hooks_all_trusted*` 는 이제 wait
// 경로가 아니라 `handle_install` 의 안내 문구(수동 승인 여부 표시)에만 쓰인다.

use std::path::{Path, PathBuf};

const HOOK_MARKER: &str = "tasty codex hook";

/// (camel for `[hooks.<Camel>]` table key, kebab for `tasty codex hook <kebab>` CLI
/// subcommand, snake for `[hooks.state."<path>:<snake>:0:0"]` trust state key).
///
/// 3 컬럼이 다른 케이스를 쓰는 이유: codex 가 같은 event 를 표면별로 다른 표기로
/// 인코딩한다. config table 키는 Rust enum variant 그대로 CamelCase, hook 명령에
/// 넘기는 우리 자체 event 이름은 kebab, codex 가 trust state 를 영속화할 때 쓰는
/// 키는 snake_case lowercase.
const HOOK_EVENTS: &[(&str, &str, &str)] = &[
    ("Stop", "stop", "stop"),
    ("UserPromptSubmit", "prompt-submit", "user_prompt_submit"),
    ("SessionStart", "session-start", "session_start"),
];

/// 경로만 계산한다 — 실패를 문구로 만들지 않으므로 `Translator` 가 없는 자리에서도
/// 쓸 수 있다(`codex_hooks_all_trusted` 는 실패를 그냥 `false` 로 접는다).
fn config_toml_path_opt() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

fn codex_config_toml_path(tr: &Translator) -> Result<PathBuf, IpcMethodError> {
    config_toml_path_opt().ok_or_else(|| IpcMethodError::new(tr.t("codex.install.no_home")))
}

fn read_toml_or_default(path: &Path) -> toml::Value {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str::<toml::Value>(&text)
            .unwrap_or_else(|_| toml::Value::Table(toml::map::Map::new())),
        Err(_) => toml::Value::Table(toml::map::Map::new()),
    }
}

fn write_toml(path: &Path, value: &toml::Value, tr: &Translator) -> Result<(), IpcMethodError> {
    let text = toml::to_string_pretty(value).map_err(|e| {
        IpcMethodError::new(tr.t_replace("codex.install.encode_failed", "{detail}", &e.to_string()))
    })?;
    std::fs::write(path, text).map_err(|e| {
        IpcMethodError::new(tr.t_replace("codex.install.write_failed", "{detail}", &e.to_string()))
    })
}

fn hook_command(event_kebab: &str) -> String {
    // TASTY_SURFACE_ID 가 비어있을 때 skip 하는 guard 포함. 가드 없으면 codex 가
    // 변수를 빈 문자열로 치환해 `tasty codex hook X --surface ` 가 실행되어
    // invalid_params 노이즈 발생.
    //
    // Windows: codex 는 hook 명령을 PowerShell 로 실행한다(실측 2026-07-12 —
    // 단일따옴표/`#` 주석이 PS 규칙으로 해석되고 순수 PS 구문 명령이 성공).
    // POSIX `[ -n ... ]` 가드는 PS 파서에서 항상 실패해 hook 이 한 번도 성공하지
    // 못하므로 PS 구문으로 발행한다. stdin 의 payload JSON 은 `$input` 으로 tasty
    // CLI 에 그대로 전달한다(session_id 추출용).
    #[cfg(windows)]
    {
        format!(
            "if ($env:TASTY_SURFACE_ID) {{ $input | tasty codex hook {event_kebab} --surface $env:TASTY_SURFACE_ID }}"
        )
    }
    // POSIX: 가드를 `if` 로 올려 "TASTY_SURFACE_ID 미설정" 과 "hook 명령 실패" 를
    // 분리한다. 옛 형태(`[ -n ... ] && ... || true`)는 둘을 한 `|| true` 로 함께
    // 삼켜서, 상태 push 가 유실돼도 아무 흔적이 남지 않았다. 안쪽 `|| true` 는
    // 이제 후자만 담당한다 — codex 턴을 방해하지 않기 위해 exit 0 은 유지하되,
    // 실패 자체는 CLI(`tasty_cli::hook_failure`)가 IPC 와 무관한 로컬 파일에
    // 기록한다. Windows(PS) 분기는 원래부터 `|| true` 가 없어 형태만 맞춘다.
    // `HOOK_MARKER`("tasty codex hook") substring 을 그대로 포함하므로 기존
    // entry 를 걷어내는 멱등 경로(`merge_install`)는 계속 발동한다.
    #[cfg(not(windows))]
    {
        format!(
            "if [ -n \"$TASTY_SURFACE_ID\" ]; then tasty codex hook {event_kebab} --surface $TASTY_SURFACE_ID || true; fi"
        )
    }
}

fn new_matcher_group(event_kebab: &str) -> toml::Value {
    let mut handler = toml::map::Map::new();
    handler.insert("type".into(), toml::Value::String("command".into()));
    handler.insert(
        "command".into(),
        toml::Value::String(hook_command(event_kebab)),
    );
    let mut group = toml::map::Map::new();
    group.insert(
        "hooks".into(),
        toml::Value::Array(vec![toml::Value::Table(handler)]),
    );
    toml::Value::Table(group)
}

fn matcher_group_has_marker(item: &toml::Value, marker: &str) -> bool {
    let Some(group) = item.as_table() else {
        return false;
    };
    let Some(hooks) = group.get("hooks").and_then(|v| v.as_array()) else {
        return false;
    };
    hooks.iter().any(|h| {
        h.as_table()
            .and_then(|t| t.get("command"))
            .and_then(|c| c.as_str())
            .map(|s| s.contains(marker))
            .unwrap_or(false)
    })
}

/// `[hooks]` 의 각 event 배열에 tasty MatcherGroup 을 멱등하게 박는다. 기존
/// non-tasty entry, 다른 키 (다른 hook event, [hooks] 외 섹션) 는 모두 보존.
fn merge_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let hooks_table = table
        .entry("hooks".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(hooks) = hooks_table.as_table_mut() else {
        return value;
    };
    for (event_key, kebab, _trust_snake) in HOOK_EVENTS {
        let event_array = hooks
            .entry((*event_key).to_string())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        let Some(arr) = event_array.as_array_mut() else {
            continue;
        };
        // 기존 tasty marker entry 제거 후 새 entry push — 멱등.
        arr.retain(|item| !matcher_group_has_marker(item, HOOK_MARKER));
        arr.push(new_matcher_group(kebab));
    }
    value
}

/// 우리가 install 한 3 개 hook 모두 trusted 상태인지 확인.
///
/// codex 는 user 가 `/hooks` 로 trust 한 hook 에 대해 `[hooks.state."<path>:<snake_event>:0:0"]`
/// 섹션에 `trusted_hash = "sha256:..."` 를 박는다. 우리 install entry 가 모두 그
/// 형식으로 등록되어있어야 hook 이 실제 fire 된다.
///
/// 주의: codex 는 부팅 시 stored hash 와 현재 hook command 의 fresh hash 를 비교해서
/// 다르면 invalidate 한다. 본 체크는 키 존재 + sha256: prefix 만 보므로, stale entry
/// 가 있고 codex 가 invalidate 한 케이스는 못 잡는다. 하지만 우리 install 은 멱등하고
/// `hook_command()` 가 static 이라 실제 stale 케이스는 사용자가 config.toml 을 직접
/// 편집한 경우 정도. `--dangerously-bypass-hook-trust`(기동 명령에 항상 포함)가
/// 이 여부와 무관하게 hook 을 fire 시키므로, 이 함수는 이제 `handle_install` 의
/// 안내 문구(수동 승인 상태 표시)에만 쓰인다 — 실제 hook 동작에는 영향 없음.
fn codex_hooks_all_trusted() -> bool {
    let Some(path) = config_toml_path_opt() else {
        return false;
    };
    let value = read_toml_or_default(&path);
    codex_hooks_all_trusted_in(&value, &path.to_string_lossy())
}

fn codex_hooks_all_trusted_in(value: &toml::Value, source_path: &str) -> bool {
    let Some(state_table) = value
        .get("hooks")
        .and_then(|v| v.get("state"))
        .and_then(|v| v.as_table())
    else {
        return false;
    };
    for (_, _, trust_snake) in HOOK_EVENTS {
        let key = format!("{source_path}:{trust_snake}:0:0");
        let trusted = state_table
            .get(&key)
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("trusted_hash"))
            .and_then(|h| h.as_str())
            .map(|s| s.starts_with("sha256:") && s.len() > "sha256:".len())
            .unwrap_or(false);
        if !trusted {
            return false;
        }
    }
    true
}

fn remove_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let Some(hooks_table) = table.get_mut("hooks").and_then(|v| v.as_table_mut()) else {
        return value;
    };
    // 각 event 의 array 에서 tasty marker 가진 MatcherGroup 만 제거. `toml::map::Map`
    // 는 values_mut 가 없어 (&Map iter 만 지원) 키 목록을 떠서 우회.
    let event_keys: Vec<String> = hooks_table.keys().cloned().collect();
    for key in event_keys {
        if let Some(arr) = hooks_table.get_mut(&key).and_then(|v| v.as_array_mut()) {
            arr.retain(|item| !matcher_group_has_marker(item, HOOK_MARKER));
        }
    }
    // 빈 array 가 된 event 키 정리.
    hooks_table.retain(|_, v| !v.as_array().map(|a| a.is_empty()).unwrap_or(false));
    // [hooks] 가 텅 비면 제거.
    if hooks_table.is_empty() {
        table.remove("hooks");
    }
    value
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    /// `require_u32` 의 **네 갈래**를 픽스처로 못박는다. 실재하는 surface id 를 쓰지
    /// 않는 이유: 그 id 가 사라지거나 바뀌면 이 회귀가 조용히 뜻을 잃는다.
    ///
    /// **문구가 아니라 키로 단정한다.** 이 메시지들은 번역되므로 영어 조각(`missing` ·
    /// `32 bits`)으로 갈래를 가르면 로케일이 바뀌는 순간 이 테스트가 뜻을 잃는다 —
    /// 그런데 그 실패는 그 언어에서만 나타나 한 언어만 보는 완주로는 안 잡힌다.
    fn absent_msg(tr: &Translator, key: &str) -> String {
        tr.t_replace("codex.params.missing", "{key}", key)
    }

    fn malformed_msg(tr: &Translator, key: &str, raw: &str) -> String {
        t_args(
            tr,
            "codex.params.not_a_number",
            &[("{key}", key), ("{raw}", raw)],
        )
    }

    #[test]
    fn require_u32_separates_absent_from_malformed_and_refuses_to_truncate() {
        let tr = test_translator();
        // ① 키 없음 — 호출자가 인자를 빠뜨렸다.
        let e = require_u32(&json!({}), "surface", &tr).unwrap_err();
        assert!(e.message.contains(&absent_msg(&tr, "surface")), "{e:?}");

        // ② 정상 — 경계값이 그대로 통과한다.
        assert_eq!(
            require_u32(&json!({ "surface": 0 }), "surface", &tr).unwrap(),
            0
        );
        assert_eq!(
            require_u32(&json!({ "surface": u32::MAX }), "surface", &tr).unwrap(),
            u32::MAX
        );

        // ③ 숫자가 아니다 — 거부하고, "없다" 라고 답하지 않는다.
        let e = require_u32(&json!({ "surface": "conductor" }), "surface", &tr).unwrap_err();
        let m = e.message.clone();
        assert!(
            m.contains(&malformed_msg(&tr, "surface", "\"conductor\"")),
            "{m}"
        );
        assert!(
            !m.contains(&absent_msg(&tr, "surface")),
            "값이 왔는데 없다고 답한다: {m}"
        );

        // ④ ★ 범위 초과 — 자르면 **다른 surface** 가 된다. u32::MAX + 2 는 1 로,
        //    5_000_000_000 은 705_032_704 로 잘린다. 둘 다 실재할 수 있는 id 다.
        for over in [
            u64::from(u32::MAX) + 1,
            u64::from(u32::MAX) + 2,
            5_000_000_000,
        ] {
            let e = require_u32(&json!({ "surface": over }), "surface", &tr).unwrap_err();
            assert!(
                e.message
                    .contains(&malformed_msg(&tr, "surface", &over.to_string())),
                "{over} 가 안 걸린다"
            );
        }

        // 음수도 같은 갈래다 — `as_u64()` 가 못 읽는다.
        assert!(require_u32(&json!({ "surface": -1 }), "surface", &tr).is_err());
    }

    /// 갈래를 가르는 두 문구가 **세 로케일 모두에서 서로 다르다.** 한 언어에서만
    /// 갈리면 다른 언어 사용자는 "인자를 빠뜨렸다" 와 "값이 틀렸다" 를 구분할 수 없다.
    #[test]
    fn the_two_branches_stay_distinguishable_in_every_locale() {
        for locale in ["en", "ko", "ja"] {
            let tr = test_translator_for(locale);
            let absent = absent_msg(&tr, "surface");
            let malformed = malformed_msg(&tr, "surface", "5000000000");
            assert_ne!(absent, malformed, "{locale}: 두 갈래의 문구가 같다");
            assert!(!absent.contains("{key}"), "{locale}: 토큰이 안 채워졌다");
            assert!(
                !malformed.contains("{key}") && !malformed.contains("{raw}"),
                "{locale}: 토큰이 안 채워졌다 — {malformed}"
            );
        }
    }

    /// `null` 은 **값이 왔다**가 아니라 **안 왔다**로 읽는다 — JSON 직렬화가 빈 슬롯을
    /// `null` 로 채우는 경우가 있어서, 이것을 오타로 취급하면 정상 경로가 막힌다.
    #[test]
    fn a_null_slot_reads_as_absent_not_as_a_malformed_value() {
        let tr = test_translator();
        let e = require_u32(&json!({ "surface": Value::Null }), "surface", &tr).unwrap_err();
        assert!(e.message.contains(&absent_msg(&tr, "surface")), "{e:?}");
    }

    /// 이 crate 의 `lang/` 를 en 으로 로드한 번역기 — 런타임에 호스트가 주입하는
    /// `plugin_dir` 없이도 같은 카탈로그를 본다(claude plugin 의 `test_translator` 와 동형).
    fn test_translator() -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, "en")
    }

    fn test_translator_for(code: &str) -> Translator {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        Translator::load(&lang_dir, code)
    }

    /// 이 완주만의 surface id. `make_codex_command` 가 쓰는 prompt 임시파일 경로는
    /// `{prefix}{surface_id}.txt` 로만 정해지므로, 같은 머신에서 이 크레이트를 **동시에
    /// 두 번** 완주하면 두 완주가 같은 파일을 쓴다. `prompt_file::write` 는 먼저 지우고
    /// 다시 만들기 때문에, 한쪽의 쓰기가 다른 쪽의 읽기 사이에 끼면 파일이 잠깐 없어져
    /// 확률적 red 가 난다 (실측: 동시 2 완주 × 40 회 = 80 완주에 1 회). 유니크하게 만들
    /// 수 있는 자리가 surface id 뿐이라 pid 를 섞는다 — `slot` 은 한 완주 **안에서**
    /// 테스트끼리 겹치지 않게 하는 번호이고, pid 가 Linux 기본 상한(2^22)보다 작으므로
    /// `pid * 8 + slot` 은 slot < 8 에서 (pid, slot) 을 유일하게 되돌릴 수 있다.
    fn unique_surface_id(slot: u32) -> u32 {
        std::process::id().wrapping_mul(8).wrapping_add(slot)
    }

    /// 이 테스트가 만든 prompt 임시파일 경로. 이름 규칙을 여기서 다시 적으면 프로덕션이
    /// 규칙을 바꿨을 때 정리가 **조용히** 빗나가므로 프로덕션과 같은 헬퍼로 만든다.
    fn prompt_path(surface_id: u32) -> std::path::PathBuf {
        prompt_file::path_for(&std::env::temp_dir(), PROMPT_FILE_PREFIX, surface_id)
    }

    #[test]
    fn make_codex_command_no_prompt() {
        assert_eq!(
            make_codex_command(42, None, ""),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust\r"
        );
        assert_eq!(
            make_codex_command(42, Some(""), ""),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust\r"
        );
    }

    #[test]
    fn make_codex_command_with_plain_prompt_uses_tempfile_cat() {
        let surface_id = unique_surface_id(1);
        let cmd = make_codex_command(surface_id, Some("hello"), "");
        assert!(
            cmd.starts_with(&format!(
                "TASTY_SURFACE_ID={surface_id} codex --dangerously-bypass-hook-trust \"$(cat '"
            )),
            "got {cmd}"
        );
        assert!(cmd.ends_with("')\"\r"), "got {cmd}");
        // 테스트 tempfile 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_file(prompt_path(surface_id));
    }

    #[test]
    fn make_codex_command_prompt_file_preserves_content_verbatim() {
        // zsh history expansion(`!`)이나 shell quoting 대상 문자가 섞여도 파일
        // 쓰기는 셸 파싱을 거치지 않으므로 그대로 보존돼야 한다.
        let prompt = "fix [!NOTE] \"bug\" in path\\to\\file";
        let surface_id = unique_surface_id(2);
        // 반환된 커맨드 문자열 자체는 이 테스트의 관심사가 아니다 — 아래에서 부작용으로
        // 쓰인 tempfile 내용만 검증한다.
        let _ = make_codex_command(surface_id, Some(prompt), "");
        let path = prompt_path(surface_id);
        let written = std::fs::read_to_string(&path).expect("prompt file should exist");
        assert_eq!(written, prompt);
        // 테스트 tempfile 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn make_codex_command_with_policy_args_no_prompt() {
        assert_eq!(
            make_codex_command(42, None, "-a never -s read-only"),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust -a never -s read-only\r"
        );
    }

    #[test]
    fn make_codex_command_with_policy_args_and_prompt() {
        let surface_id = unique_surface_id(3);
        let cmd = make_codex_command(surface_id, Some("hello"), "-a never");
        assert!(
            cmd.starts_with(&format!(
                "TASTY_SURFACE_ID={surface_id} codex --dangerously-bypass-hook-trust -a never \"$(cat '"
            )),
            "got {cmd}"
        );
        // 테스트 tempfile 정리 — 실패해도(OS 임시 디렉토리 정리 대상) 테스트 결과에 무해.
        let _ = std::fs::remove_file(prompt_path(surface_id));
    }

    #[test]
    fn make_codex_command_with_full_auto_bypass() {
        assert_eq!(
            make_codex_command(42, None, "--dangerously-bypass-approvals-and-sandbox"),
            "TASTY_SURFACE_ID=42 codex --dangerously-bypass-hook-trust --dangerously-bypass-approvals-and-sandbox\r"
        );
    }

    #[test]
    fn resolve_policy_args_defaults_to_never_approval_when_nothing_set() {
        // 승인 프롬프트가 무인 spawn 자식을 영구 정지시키는 문제(docs/plugins/codex/index.md
        // 의 승인/샌드박스 정책 플래그 절 참조) 재발 방지 — approval 은 아무것도 설정되지
        // 않아도 무조건 "never" 로 떨어져야 한다.
        // sandbox 는 그 자체로 정지를 유발하지 않으므로 기존대로 미설정 시 빈 채로 둔다.
        let host = MockHost::new();
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never"
        );
    }

    #[test]
    fn resolve_policy_args_uses_explicit_approval_and_sandbox() {
        let host = MockHost::new();
        let params = json!({ "approval": "never", "sandbox": "read-only" });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "-a never -s read-only"
        );
    }

    #[test]
    fn resolve_policy_args_rejects_invalid_approval_value() {
        let host = MockHost::new();
        let err = resolve_policy_args(&host, &json!({ "approval": "yolo" }), &test_translator())
            .unwrap_err();
        assert!(format!("{err:?}").contains("invalid 'approval'"));
    }

    #[test]
    fn resolve_policy_args_rejects_invalid_sandbox_value() {
        let host = MockHost::new();
        let err = resolve_policy_args(&host, &json!({ "sandbox": "yolo" }), &test_translator())
            .unwrap_err();
        assert!(format!("{err:?}").contains("invalid 'sandbox'"));
    }

    #[test]
    fn resolve_policy_args_full_auto_bypasses_both() {
        let host = MockHost::new();
        let params = json!({ "full_auto": true });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "--dangerously-bypass-approvals-and-sandbox"
        );
    }

    #[test]
    fn resolve_policy_args_full_auto_rejects_combination_with_approval() {
        let host = MockHost::new();
        let params = json!({ "full_auto": true, "approval": "never" });
        let err = resolve_policy_args(&host, &params, &test_translator()).unwrap_err();
        assert!(format!("{err:?}").contains("full_auto"));
    }

    #[test]
    fn resolve_policy_args_falls_back_to_global_defaults() {
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "never");
        host.set_setting("default_sandbox_mode", "workspace-write");
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never -s workspace-write"
        );
    }

    #[test]
    fn resolve_policy_args_per_call_override_wins_over_global_default() {
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "never");
        let params = json!({ "approval": "on-request" });
        assert_eq!(
            resolve_policy_args(&host, &params, &test_translator()).unwrap(),
            "-a on-request"
        );
    }

    #[test]
    fn resolve_policy_args_global_inherit_still_falls_back_to_never() {
        // 전역 설정을 명시적으로 "inherit" 로 둬도(=touch 안 한 것과 구분 불가) 하드코드
        // 기본값(never)이 적용된다 — "inherit" 을 골라도 더 이상 인터랙티브 승인으로
        // 되돌아가지 않는다(의도된 동작, 위 resolve_policy_args 문서 주석 참고).
        let host = MockHost::new();
        host.set_setting("default_approval_policy", "inherit");
        assert_eq!(
            resolve_policy_args(&host, &json!({}), &test_translator()).unwrap(),
            "-a never"
        );
    }

    #[test]
    fn hook_event_to_state_maps_known_events() {
        assert_eq!(
            hook_event_to_state("stop", &test_translator()).unwrap(),
            "idle"
        );
        assert_eq!(
            hook_event_to_state("prompt-submit", &test_translator()).unwrap(),
            "active"
        );
        assert_eq!(
            hook_event_to_state("session-start", &test_translator()).unwrap(),
            "active"
        );
    }

    #[test]
    fn hook_event_to_state_rejects_unsupported() {
        // notification / session-end / subagent-stop 은 codex 가 fire 하지 않으므로
        // 거부 (silent no-op 대신 invalid_params).
        let err = hook_event_to_state("notification", &test_translator()).unwrap_err();
        assert!(format!("{err:?}").contains("unknown hook event"));
    }

    #[test]
    fn build_spawn_warning_none_below_threshold() {
        assert_eq!(
            build_spawn_warning(&test_translator(), 3, &[], &[], 6.0),
            None
        );
    }

    #[test]
    fn build_spawn_warning_above_threshold_lists_idle_and_mentions_respawn() {
        let w = build_spawn_warning(&test_translator(), 7, &[2, 5], &[], 6.0).unwrap();
        assert!(w.contains("respawn"));
        assert!(w.contains('2') && w.contains('5'));
    }

    #[test]
    fn build_spawn_warning_above_threshold_no_idle_has_no_respawn_word() {
        let w = build_spawn_warning(&test_translator(), 7, &[], &[], 6.0).unwrap();
        assert!(!w.contains("respawn"));
    }

    #[test]
    fn build_spawn_warning_respects_custom_threshold() {
        // threshold=6 이면 안 뜨는 3개가, threshold=3 이면 뜬다(설정 override 시나리오).
        let tr = test_translator();
        assert_eq!(build_spawn_warning(&tr, 3, &[], &[], 6.0), None);
        assert!(build_spawn_warning(&tr, 4, &[], &[], 3.0).is_some());
    }

    /// 확정 stale 자식만 있어도 respawn 을 권해야 한다 — hook 유실로 idle 보고가
    /// 영영 오지 않는 자식이 정확히 이 경우다(ADR-0072 가 겨냥한 시나리오).
    #[test]
    fn build_spawn_warning_lists_stale_children_as_respawn_candidates() {
        let w = build_spawn_warning(&test_translator(), 7, &[], &[3], 6.0).unwrap();
        assert!(w.contains("respawn"), "{w}");
        assert!(w.contains('3'), "{w}");
    }

    /// stale 문구는 idle 문구와 분리된다 — 보고받지 않은 자식에 "have already
    /// finished their work" 를 쓰면 자식이 그렇게 보고한 것처럼 읽힌다.
    #[test]
    fn build_spawn_warning_separates_stale_wording_from_idle() {
        let tr = test_translator();
        let idle_only = build_spawn_warning(&tr, 7, &[2], &[], 6.0).unwrap();
        let stale_only = build_spawn_warning(&tr, 7, &[], &[3], 6.0).unwrap();
        assert!(idle_only.contains("have already finished their work"));
        assert!(
            !stale_only.contains("have already finished their work"),
            "{stale_only}"
        );
        assert!(
            stale_only.contains("never reported completion"),
            "{stale_only}"
        );

        let both = build_spawn_warning(&tr, 7, &[2], &[3], 6.0).unwrap();
        assert!(both.contains("have already finished their work"), "{both}");
        assert!(both.contains("never reported completion"), "{both}");
    }

    /// 완성 문구가 `lang/en.toml` 의 대응 키를 플레이스홀더만 치환한 결과와 정확히
    /// 같아야 한다 — 문구가 `t()` 를 실제로 거쳤다는 직접 증거(스펙 "확인 절차" 1).
    #[test]
    fn build_spawn_warning_matches_lang_catalog_after_substitution() {
        let tr = test_translator();
        let expected = tr
            .t("codex.spawn_warning.total")
            .replace("{total}", "7")
            .replace("{threshold}", "6")
            + &tr
                .t("codex.spawn_warning.idle")
                .replace("{indices}", "2, 5")
            + &tr.t("codex.spawn_warning.stale").replace("{indices}", "3");
        assert_eq!(
            build_spawn_warning(&tr, 7, &[2, 5], &[3], 6.0).unwrap(),
            expected
        );
    }

    /// 로케일을 바꾸면 완성 문구가 실제로 바뀐다 — 문구가 `t()` 를 거친다는 사실의
    /// 행동적 증거(스펙 "확인 절차" 2 의 자동화 대응). 치환된 값(인덱스)은 로케일과
    /// 무관하게 그대로 실린다.
    #[test]
    fn build_spawn_warning_follows_active_locale() {
        let lang_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang");
        let en =
            build_spawn_warning(&Translator::load(&lang_dir, "en"), 7, &[2], &[3], 6.0).unwrap();
        let ko =
            build_spawn_warning(&Translator::load(&lang_dir, "ko"), 7, &[2], &[3], 6.0).unwrap();
        let ja =
            build_spawn_warning(&Translator::load(&lang_dir, "ja"), 7, &[2], &[3], 6.0).unwrap();
        assert_ne!(en, ko);
        assert_ne!(en, ja);
        assert_ne!(ko, ja);
        // 어느 로케일이든 치환 자체는 성공해야 한다(플레이스홀더가 남지 않는다).
        for (locale, msg) in [("en", &en), ("ko", &ko), ("ja", &ja)] {
            assert!(!msg.contains("{total}"), "{locale}: {msg}");
            assert!(!msg.contains("{threshold}"), "{locale}: {msg}");
            assert!(!msg.contains("{indices}"), "{locale}: {msg}");
            assert!(
                msg.contains('7') && msg.contains('2') && msg.contains('3'),
                "{locale}: {msg}"
            );
        }
    }

    // 카탈로그 정합(세 로케일의 키 집합 · 플레이스홀더 이름 · 소스가 부르는 키의 실재)은
    // **레포 전역 가드 `tests/i18n_key_parity.rs` 가 이미 본다** — 번들 plugin 의
    // `lang/` 을 전부 훑으므로 여기 사본을 두지 않는다. 그 가드는 통합 타깃이라
    // 자동 실행이 헤드리스 조합에서만 일어난다(`docs/dev-guide/ci-gates.md`).

    /// `heuristic` stale 은 제외된다 — SIGSTOP·긴 추론과 구별되지 않아 일하는
    /// 자식을 respawn 하라고 권하게 된다. 확정(`foreground_is_shell`)만 센다.
    #[test]
    fn spawn_warning_counts_only_confirmed_stale() {
        let children = vec![
            json!({ "index": 1, "state": "stale", "confidence": "confirmed" }),
            json!({ "index": 2, "state": "stale", "confidence": "heuristic" }),
            json!({ "index": 3, "state": "active", "confidence": "confirmed" }),
        ];
        let stale = indices_with(&children, |c| {
            state_of(c) == Some("stale")
                && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
        });
        assert_eq!(stale, vec![1]);
    }

    /// `confidence` 를 싣지 않는 옛 호스트 응답에서는 stale 이 하나도 안 잡힌다 —
    /// 경고가 과하게 나가는 것보다 안전한 쪽으로 실패한다(기존 idle 경고는 그대로).
    #[test]
    fn spawn_warning_ignores_stale_without_confidence_field() {
        let children = vec![json!({ "index": 1, "state": "stale" })];
        let stale = indices_with(&children, |c| {
            state_of(c) == Some("stale")
                && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
        });
        assert!(stale.is_empty());
    }

    fn parse_toml(text: &str) -> toml::Value {
        toml::from_str(text).expect("valid toml")
    }

    #[test]
    fn merge_install_adds_three_events() {
        let result = merge_install(toml::Value::Table(toml::map::Map::new()));
        let hooks = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .unwrap();
        for (event_key, _, _) in HOOK_EVENTS {
            assert!(hooks.contains_key(*event_key), "missing {event_key}");
            // 각 event 는 marker 가진 MatcherGroup 한 개.
            let arr = hooks.get(*event_key).unwrap().as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert!(matcher_group_has_marker(&arr[0], HOOK_MARKER));
        }
    }

    /// POSIX hook 명령의 가드는 **`if` 블록**이어야 한다 — 옛 `A && B || true`
    /// 형태는 "TASTY_SURFACE_ID 미설정"(정당한 침묵)과 "hook 명령 실패"(관측해야 할
    /// 상태 push 유실)를 한 연산자로 함께 삼킨다. 안쪽 `|| true` 는 codex 턴을
    /// 막지 않기 위해 유지하되, 실패 기록은 CLI 쪽 `hook_failure` 가 담당한다.
    #[cfg(not(windows))]
    #[test]
    fn posix_hook_command_separates_guard_from_failure_handling() {
        for (_, kebab, _) in HOOK_EVENTS {
            let cmd = hook_command(kebab);
            assert!(
                cmd.starts_with("if [ -n \"$TASTY_SURFACE_ID\" ]; then "),
                "가드가 if 블록이 아니다: {cmd}"
            );
            assert!(cmd.ends_with("; fi"), "블록이 닫히지 않았다: {cmd}");
            assert!(
                !cmd.contains("] && "),
                "옛 `A && B || true` 형태로 회귀했다: {cmd}"
            );
            // marker 를 잃으면 멱등 경로가 깨져 옛 entry 가 남고 hook 이 두 번 발화한다.
            assert!(cmd.contains(HOOK_MARKER), "marker 를 잃었다: {cmd}");
        }
    }

    /// 옛 프로덕션 문자열이 박힌 config.toml 에 install 을 다시 걸어도 entry 수가
    /// 늘지 않고 명령만 새 형태로 교체된다(`retain` + push 멱등 경로가 계속 발동).
    #[cfg(not(windows))]
    #[test]
    fn merge_install_replaces_legacy_command_without_growing() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "[ -n \"$TASTY_SURFACE_ID\" ] && tasty codex hook stop --surface $TASTY_SURFACE_ID || true"
"#,
        );
        let result = merge_install(initial);
        let arr = result
            .get("hooks")
            .and_then(|v| v.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(arr.len(), 1, "중복 entry 가 생기면 hook 이 두 번 발화한다");
        let cmd = arr[0]
            .get("hooks")
            .and_then(|v| v.as_array())
            .and_then(|a| a[0].get("command"))
            .and_then(|c| c.as_str())
            .unwrap();
        assert_eq!(cmd, hook_command("stop"));
    }

    /// 가드 동작 회귀 — `$TASTY_SURFACE_ID` 가 없으면 조용히 exit 0(비-tasty 환경
    /// 무소음). 형태가 아니라 실제 `sh` 실행 결과로 고정한다.
    #[cfg(unix)]
    #[test]
    fn posix_guard_exits_silently_without_surface_id() {
        let out = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(hook_command("stop"))
            .env_remove("TASTY_SURFACE_ID")
            .env("PATH", "/nonexistent")
            .output()
            .expect("/bin/sh");
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(
            out.stderr.is_empty(),
            "stderr 무소음: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn merge_install_preserves_other_keys_and_other_hook_events() {
        let initial = parse_toml(
            r#"
model = "gpt-5.5"

[projects."/path"]
trust_level = "trusted"

[[hooks.PreToolUse]]
[[hooks.PreToolUse.hooks]]
type = "command"
command = "user's own hook"
"#,
        );
        let result = merge_install(initial);
        let table = result.as_table().unwrap();
        assert_eq!(table.get("model").and_then(|v| v.as_str()), Some("gpt-5.5"));
        assert!(table.get("projects").is_some());
        let hooks = table.get("hooks").and_then(|v| v.as_table()).unwrap();
        // 사용자의 PreToolUse 는 그대로.
        let pre = hooks.get("PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert!(!matcher_group_has_marker(&pre[0], HOOK_MARKER));
        // tasty 의 Stop / UserPromptSubmit / SessionStart 가 추가됨.
        for (key, _, _) in HOOK_EVENTS {
            let arr = hooks.get(*key).unwrap().as_array().unwrap();
            assert_eq!(arr.len(), 1);
            assert!(matcher_group_has_marker(&arr[0], HOOK_MARKER));
        }
    }

    #[test]
    fn merge_install_is_idempotent() {
        let empty = toml::Value::Table(toml::map::Map::new());
        let once = merge_install(empty);
        let twice = merge_install(once.clone());
        assert_eq!(
            toml::to_string(&once).unwrap(),
            toml::to_string(&twice).unwrap()
        );
    }

    #[test]
    fn merge_install_keeps_coexisting_non_tasty_stop_hook() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "user wrote this Stop hook themselves"
"#,
        );
        let result = merge_install(initial);
        let stop = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        // 사용자 hook + tasty hook = 2 entries.
        assert_eq!(stop.len(), 2);
        assert_eq!(
            stop.iter()
                .filter(|i| matcher_group_has_marker(i, HOOK_MARKER))
                .count(),
            1
        );
    }

    #[test]
    fn remove_install_removes_only_tasty_marker_entries() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "keep me — not tasty"

[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "tasty codex hook stop --surface $TASTY_SURFACE_ID"
"#,
        );
        let result = remove_install(initial);
        let stop = result
            .as_table()
            .and_then(|t| t.get("hooks"))
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("Stop"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(stop.len(), 1);
        assert!(!matcher_group_has_marker(&stop[0], HOOK_MARKER));
    }

    #[test]
    fn remove_install_drops_empty_hooks_block() {
        let initial = parse_toml(
            r#"
[[hooks.Stop]]
[[hooks.Stop.hooks]]
type = "command"
command = "tasty codex hook stop"
"#,
        );
        let result = remove_install(initial);
        // [hooks] 가 통째로 사라져야 함.
        assert!(result.as_table().unwrap().get("hooks").is_none());
    }

    #[test]
    fn codex_hooks_all_trusted_in_returns_true_when_all_three_present() {
        let path = "/Users/x/.codex/config.toml";
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc123"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = "sha256:def456"

[hooks.state."{path}:session_start:0:0"]
trusted_hash = "sha256:fff999"
"#
        );
        let value = parse_toml(&toml);
        assert!(codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_any_missing() {
        let path = "/Users/x/.codex/config.toml";
        // Stop + UserPromptSubmit 만, SessionStart 빠짐.
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = "sha256:def"
"#
        );
        let value = parse_toml(&toml);
        assert!(!codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_hash_value_invalid() {
        let path = "/Users/x/.codex/config.toml";
        // 3 개 entry 모두 있지만 trusted_hash 가 sha256: prefix 미충족.
        let toml = format!(
            r#"
[hooks.state."{path}:stop:0:0"]
trusted_hash = "sha256:abc"

[hooks.state."{path}:user_prompt_submit:0:0"]
trusted_hash = ""

[hooks.state."{path}:session_start:0:0"]
trusted_hash = "sha256:abc"
"#
        );
        let value = parse_toml(&toml);
        assert!(!codex_hooks_all_trusted_in(&value, path));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_no_state_section() {
        let value = parse_toml("model = \"gpt-5.5\"");
        assert!(!codex_hooks_all_trusted_in(&value, "/x"));
    }

    #[test]
    fn codex_hooks_all_trusted_in_false_when_only_other_paths_present() {
        // 다른 config 경로의 hook 만 있는 경우 → 우리 경로 기준 false.
        let toml = r#"
[hooks.state."/other/path:stop:0:0"]
trusted_hash = "sha256:xyz"

[hooks.state."/other/path:user_prompt_submit:0:0"]
trusted_hash = "sha256:xyz"

[hooks.state."/other/path:session_start:0:0"]
trusted_hash = "sha256:xyz"
"#;
        let value = parse_toml(toml);
        assert!(!codex_hooks_all_trusted_in(
            &value,
            "/Users/x/.codex/config.toml"
        ));
    }

    // ── 형제 once-hook 정리 재현 (docs/plugins/codex/index.md 의 notify-caller 형제 hook 정리 절 참조) ──

    use std::cell::RefCell;

    struct MockHook {
        id: u64,
        surface_id: u32,
        command: String,
        event: String,
    }

    /// hook.set/list/unset + terminal.tell 을 in-memory 로 시뮬레이션하는 mock 호스트.
    /// `alive` 는 `surface.locate` 응답을 시뮬레이션 — 기본은 아무도 살아있지 않은
    /// 것으로 취급하고(= surface.locate 조회 실패와 동일하게 안전 쪽으로 fallback),
    /// `mark_alive`/`mark_dead` 로 명시적으로 상태를 세팅한다.
    struct MockHost {
        hooks: RefCell<Vec<MockHook>>,
        next_id: RefCell<u64>,
        alive: RefCell<std::collections::HashSet<u32>>,
        settings: RefCell<std::collections::HashMap<String, String>>,
        /// `surface.screen_text` 응답 시뮬레이션(docs/plugins/codex/index.md 의 샌드박스
        /// 초기화 실패 힌트 절 참조) — `None`이면 조회 자체가
        /// 실패(soft-fail 경로 재현), `Some`이면 그 문자열을 `text`로 돌려준다.
        screen_text: RefCell<Option<String>>,
    }

    impl MockHost {
        fn new() -> Self {
            Self {
                hooks: RefCell::new(Vec::new()),
                next_id: RefCell::new(1),
                alive: RefCell::new(std::collections::HashSet::new()),
                settings: RefCell::new(std::collections::HashMap::new()),
                screen_text: RefCell::new(None),
            }
        }

        /// `surface.screen_text` 가 이 텍스트를 `text` 필드로 돌려주도록 세팅한다.
        fn set_screen_text(&self, text: &str) {
            *self.screen_text.borrow_mut() = Some(text.to_string());
        }

        /// `settings.get_plugin_setting` 응답을 시뮬레이션 — 전역 기본 정책
        /// (`default_approval_policy`/`default_sandbox_mode`) fallback 테스트용.
        fn set_setting(&self, storage_key: &str, value: &str) {
            self.settings
                .borrow_mut()
                .insert(storage_key.to_string(), value.to_string());
        }

        /// event 발화 시뮬레이션 — 매칭 once-hook 제거(호스트 `check_and_fire` retain 동일).
        fn fire(&self, surface_id: u32, event: &str) -> usize {
            let mut hooks = self.hooks.borrow_mut();
            let before = hooks.len();
            hooks.retain(|h| !(h.surface_id == surface_id && h.event == event));
            before - hooks.len()
        }

        fn commands_on(&self, surface_id: u32) -> Vec<String> {
            self.hooks
                .borrow()
                .iter()
                .filter(|h| h.surface_id == surface_id)
                .map(|h| h.command.clone())
                .collect()
        }

        /// `surface.locate` 가 `exists: true` 를 돌려주도록(= 아직 process 가 살아있음).
        fn mark_alive(&self, surface_id: u32) {
            self.alive.borrow_mut().insert(surface_id);
        }

        /// `surface.locate` 가 `exists: false` 를 돌려주도록(= process-exit 로 host 가
        /// 이미 surface 를 닫아버림을 재현).
        fn mark_dead(&self, surface_id: u32) {
            self.alive.borrow_mut().remove(&surface_id);
        }
    }

    impl HostCall for MockHost {
        fn call(
            &self,
            method: &str,
            params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            match method {
                "hook.set" => {
                    let mut id = self.next_id.borrow_mut();
                    let hid = *id;
                    *id += 1;
                    self.hooks.borrow_mut().push(MockHook {
                        id: hid,
                        surface_id: params["surface_id"].as_u64().unwrap() as u32,
                        command: params["command"].as_str().unwrap().to_string(),
                        event: params["event"].as_str().unwrap().to_string(),
                    });
                    Ok(json!({ "hook_id": hid }))
                }
                "hook.list" => {
                    let sid = params["surface_id"].as_u64().map(|v| v as u32);
                    let arr: Vec<Value> = self
                        .hooks
                        .borrow()
                        .iter()
                        .filter(|h| sid.is_none_or(|s| h.surface_id == s))
                        .map(|h| {
                            json!({ "id": h.id, "surface_id": h.surface_id, "command": h.command, "event": h.event })
                        })
                        .collect();
                    Ok(json!(arr))
                }
                "hook.unset" => {
                    let hid = params["hook_id"].as_u64().unwrap();
                    self.hooks.borrow_mut().retain(|h| h.id != hid);
                    Ok(json!({ "removed": true }))
                }
                "surface.locate" => {
                    let sid = params["surface_id"].as_u64().unwrap() as u32;
                    let exists = self.alive.borrow().contains(&sid);
                    Ok(json!({ "surface_id": sid, "exists": exists }))
                }
                "settings.get_plugin_setting" => {
                    let key = params["storage_key"].as_str().unwrap();
                    match self.settings.borrow().get(key) {
                        Some(v) => Ok(json!({ "value": v })),
                        None => Ok(json!({})),
                    }
                }
                "surface.screen_text" => match self.screen_text.borrow().as_ref() {
                    Some(text) => Ok(json!({ "text": text })),
                    None => Err(tasty_plugin_sdk::PluginError::HostCall {
                        method: method.to_string(),
                        message: "no screen_text set (mock soft-fail)".to_string(),
                    }),
                },
                _ => Ok(json!({})),
            }
        }
    }

    // ── 완료 알림 문구 — "spawn 완료" 오독 방지 ──

    #[test]
    fn notify_caller_message_leads_with_work_completion() {
        let msg = notify_caller_message(&test_translator_for("ko"), "spawn", 42);
        assert!(
            msg.contains("작업 완료"),
            "완료 대상이 '작업'임이 드러나야 함: {msg}"
        );
        assert!(msg.contains("42"), "target surface 번호 누락: {msg}");
        assert!(msg.contains("spawn"), "호출 방식 정보 누락: {msg}");
    }

    /// 어느 로케일에서든 **채워지는 값**(대상 번호 · 호출 방식)은 문구에 남는다.
    /// 자리표시자 이름을 바꾸면 치환이 조용히 안 먹고 `{target}` 이 그대로 나가는데,
    /// 그건 한 언어만 보면 안 걸린다.
    #[test]
    fn notify_caller_message_keeps_its_substitutions_in_every_locale() {
        for code in ["en", "ko", "ja"] {
            let msg = notify_caller_message(&test_translator_for(code), "tell", 42);
            assert!(msg.contains("42"), "[{code}] target 번호 누락: {msg}");
            assert!(msg.contains("tell"), "[{code}] 호출 방식 누락: {msg}");
            assert!(
                !msg.contains("{target}") && !msg.contains("{kind}"),
                "[{code}] 자리표시자가 치환되지 않았다: {msg}"
            );
        }
    }

    /// 문구가 실제로 `t()` 를 거친다 — 로케일을 바꾸면 완성 문구가 달라진다.
    /// 이 단정이 없으면 키만 만들어 두고 값을 코드에 도로 박아도 위 테스트는 통과한다.
    #[test]
    fn notify_caller_message_changes_with_the_locale() {
        let en = notify_caller_message(&test_translator_for("en"), "spawn", 42);
        let ko = notify_caller_message(&test_translator_for("ko"), "spawn", 42);
        assert_ne!(en, ko, "로케일이 달라도 같은 문구다 — t() 를 안 거친다");
    }

    #[test]
    fn notify_caller_message_does_not_read_as_command_itself_completing() {
        // 회귀 방지: 과거 "{kind} 완료: surface N" 형태는 "spawn 이라는 동작이
        // 완료됐다"로 오독되기 쉬웠다 — kind 가 더 이상 완료의 주어로 문장 맨 앞에
        // 오지 않아야 한다.
        let tr = test_translator_for("ko");
        for kind in ["spawn", "tell"] {
            let msg = notify_caller_message(&tr, kind, 7);
            assert!(
                !msg.starts_with(&format!("{kind} 완료")),
                "옛 오독 유발 포맷으로 회귀함: {msg}"
            );
        }
    }

    // ── 샌드박스 초기화 실패 힌트 (docs/plugins/codex/index.md 의 샌드박스 초기화 실패 힌트 절 참조) ──

    #[test]
    fn detect_sandbox_failure_hint_matches_rtm_newaddr() {
        let text = "...\nbwrap: loopback: Failed RTM_NEWADDR: Operation not permitted\n...";
        assert!(detect_sandbox_failure_hint(&test_translator(), text).is_some());
    }

    #[test]
    fn detect_sandbox_failure_hint_ignores_unrelated_text() {
        assert!(
            detect_sandbox_failure_hint(&test_translator(), "normal codex output, no errors here")
                .is_none()
        );
    }

    #[test]
    fn append_sandbox_hint_if_detected_appends_when_marker_present() {
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let text = "bwrap: loopback: Failed RTM_NEWADDR: Operation not permitted";
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), Some(text));
        assert!(
            msg.starts_with(&base),
            "기존 base 메시지는 그대로 유지: {msg}"
        );
        assert!(msg.contains("full-auto"), "우회법 안내 누락: {msg}");
        assert!(msg.contains("RTM_NEWADDR"), "탐지 근거 노출 누락: {msg}");
    }

    #[test]
    fn append_sandbox_hint_if_detected_unchanged_when_no_marker() {
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), Some("normal codex output"));
        assert_eq!(msg, base, "마커 없으면 회귀 없이 base 그대로여야 함");
    }

    #[test]
    fn append_sandbox_hint_if_detected_unchanged_when_query_failed() {
        // screen_text 조회 자체가 실패(soft-fail)한 경우 — None.
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), None);
        assert_eq!(
            msg, base,
            "조회 실패 시 알림 전송 자체는 회귀 없이 그대로여야 함"
        );
    }

    #[test]
    fn notify_caller_appends_sandbox_hint_when_rtm_newaddr_detected() {
        let host = MockHost::new();
        host.set_screen_text(
            "...\nbwrap: loopback: Failed RTM_NEWADDR: Operation not permitted\n...",
        );
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        let msg = append_sandbox_hint_if_detected(&tr, base, screen_text.as_deref());
        assert!(
            msg.contains("full-auto"),
            "힌트가 실제로 덧붙어야 함: {msg}"
        );
    }

    #[test]
    fn notify_caller_message_unchanged_when_no_sandbox_failure() {
        let host = MockHost::new();
        host.set_screen_text("normal codex output, no errors here");
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), screen_text.as_deref());
        assert_eq!(
            msg, base,
            "실패 시그니처가 없으면 회귀 없이 base 그대로여야 함"
        );
    }

    #[test]
    fn notify_caller_message_unchanged_when_screen_text_query_fails() {
        // MockHost::new() 는 screen_text 를 세팅하지 않은 상태 — surface.screen_text
        // 호출이 실패하는 경우를 재현(soft-fail).
        let host = MockHost::new();
        let tr = test_translator();
        let base = notify_caller_message(&tr, "spawn", 100);
        let screen_text = fetch_screen_text_for_hint(&host, 100);
        assert!(screen_text.is_none(), "조회 실패는 None 이어야 함");
        let msg = append_sandbox_hint_if_detected(&tr, base.clone(), screen_text.as_deref());
        assert_eq!(msg, base);
    }

    #[test]
    fn sibling_cleanup_removes_all_after_one_fires() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 2, "2 형제 등록");

        // codex-idle 이 fire(once 제거) → 나머지 형제(process-exit) 정리.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        let expected = notify_caller_command(caller, target, "tell");
        cleanup_sibling_hooks(&host, target, &expected);

        assert!(
            host.commands_on(target).is_empty(),
            "형제 hook 이 하나도 남지 않아야 함 — process-exit 좀비 없음: {:?}",
            host.commands_on(target)
        );
    }

    #[test]
    fn concurrent_registrations_leave_no_zombie() {
        // 같은 child(target) 에 spawn 완료 hook 과 tell 완료 hook 이 겹쳐 등록된 상태.
        // 옛 단일 meta 슬롯(`codex-notify-hooks`) 방식이면 tell 등록이 spawn 의 sibling
        // id 목록을 덮어써, spawn 그룹의 process-exit 이 정리되지 못하고 좀비로 남았다.
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        register_notify_hooks(&host, target, caller, "spawn");
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 4, "두 그룹 = 4 hook");

        // spawn 그룹의 codex-idle 이 먼저 fire → spawn 그룹만 정리.
        host.fire(target, "codex-idle");
        let spawn_cmd = notify_caller_command(caller, target, "spawn");
        cleanup_sibling_hooks(&host, target, &spawn_cmd);

        let remaining = host.commands_on(target);
        let tell_cmd = notify_caller_command(caller, target, "tell");
        assert!(
            remaining.iter().all(|c| c == &tell_cmd),
            "spawn 그룹 좀비 잔존: {remaining:?}"
        );
        assert!(
            !remaining.iter().any(|c| c == &spawn_cmd),
            "spawn 그룹 process-exit 좀비 남음"
        );

        // 이제 tell 그룹도 fire → 전부 정리.
        host.fire(target, "process-exit");
        cleanup_sibling_hooks(&host, target, &tell_cmd);
        assert!(
            host.commands_on(target).is_empty(),
            "최종적으로 형제 hook 이 전부 사라져야 함: {:?}",
            host.commands_on(target)
        );
    }

    // ── 자기재무장(self-rearm) — child 가 살아있는 동안 알림 반복 (docs/plugins/codex/index.md 의 자기재무장 절 참조) ──
    //
    // 배경: codex-idle 은 process-exit 와 달리 "child 가 아직 살아있는 상태 전환"일 수
    // 있다. 형제 hook 이 once=true 라 한 번 fire 하면 남은 형제도 정리돼 그 spawn/tell
    // 콜당 알림이 딱 1번만 오던 문제 — 진짜 완료 전에 codex-idle 을 한 번이라도 거치면
    // 그 뒤엔 재알림 경로가 없었다.

    #[test]
    fn handle_notify_caller_rearms_when_target_still_alive() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, target, caller, "tell");
        assert_eq!(host.commands_on(target).len(), 2, "최초 2 형제 등록");

        // 1번째 전환: codex-idle — child 는 여전히 살아있다.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            &test_translator(),
            json!({ "caller": caller, "target": target, "kind": "tell" }),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            2,
            "살아있으면 형제 hook 이 다시 2개로 재무장돼야 함"
        );

        // 2번째 전환에도 계속 재무장되는지 확인 — 'spawn/tell 당 1회' 로 되돌아가면 안 됨.
        assert_eq!(host.fire(target, "codex-idle"), 1);
        handle_notify_caller(
            &host,
            &test_translator(),
            json!({ "caller": caller, "target": target, "kind": "tell" }),
        )
        .unwrap();
        assert_eq!(
            host.commands_on(target).len(),
            2,
            "두 번째 전환에도 재무장돼야 함"
        );
    }

    #[test]
    fn handle_notify_caller_does_not_rearm_when_target_exited() {
        let host = MockHost::new();
        let (caller, target) = (7u32, 1650u32);
        host.mark_alive(target);
        register_notify_hooks(&host, target, caller, "spawn");

        // process-exit 로 fire — host 는 이 시점에 이미 동기로 surface 를 닫으므로
        // surface.locate 가 exists:false 를 돌려주는 상황을 재현.
        assert_eq!(host.fire(target, "process-exit"), 1);
        host.mark_dead(target);
        handle_notify_caller(
            &host,
            &test_translator(),
            json!({ "caller": caller, "target": target, "kind": "spawn" }),
        )
        .unwrap();

        assert!(
            host.commands_on(target).is_empty(),
            "죽은 surface 에 재무장하면 좀비 hook: {:?}",
            host.commands_on(target)
        );
    }
}
