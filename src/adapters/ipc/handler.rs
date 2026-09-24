mod checked;
#[cfg(all(test, debug_assertions))]
mod cli_entry_debug_tests;
#[cfg(test)]
pub(crate) mod cli_entry_tests;
mod completion_strategy;
#[cfg(all(debug_assertions, feature = "gui"))]
mod debug;
#[cfg(debug_assertions)]
mod debug_nav;
#[cfg(debug_assertions)]
pub(crate) mod debug_plugin;
#[cfg(debug_assertions)]
mod debug_state;
#[cfg(debug_assertions)]
mod debug_terminal;
mod entry_window;
mod file_handler;
#[cfg(feature = "gui")]
mod file_picker;
#[cfg(feature = "gui")]
mod git_viewer;
mod hook_handler;
pub(crate) mod idempotency;
#[cfg(test)]
mod intent_order_tests;
// 창 라우터 호출자 검사는 해당 라우터와 같은 GUI 조건에서 실행한다.
#[cfg(all(test, feature = "gui"))]
mod window_router_caller_tests;
// `list_global` 이 두 hook 목록을 합산하므로 크레이트 안에서 보여야 한다.
pub(crate) mod hooks;
// image.list는 호스트의 전 창 합산 조회에서 사용한다.
#[cfg(feature = "gui")]
pub(crate) mod image;
#[cfg(all(debug_assertions, target_os = "macos", feature = "gui"))]
mod input_source;
#[cfg(feature = "gui")]
mod markdown;
#[cfg(feature = "gui")]
mod markdown_mirror;
mod memory;
mod message;
mod meta;
pub(crate) mod notification;
// 창별 목록을 호스트에서 합산할 수 있도록 공개한다.
pub(crate) mod output;
pub(crate) mod pane;
pub(crate) mod params;
mod passkey;
mod preset;
mod pressure;
pub(crate) mod pty;
mod recent;
mod remote_profile;
mod settings;
pub(crate) mod surface;
pub(crate) mod tab;
mod telemetry;
mod terminal;
mod terminal_input;
pub(crate) mod theme;
#[cfg(all(debug_assertions, feature = "gui"))]
mod tool;
mod webhook;
#[cfg(feature = "gui")]
pub(crate) mod webview;
pub(crate) mod workspace;
pub(crate) mod workspace_category;

pub mod agent;
pub mod approval;
pub(crate) mod attach;
pub mod audit;
pub mod events;
#[cfg(all(debug_assertions, feature = "gui"))]
pub mod ime;
pub mod plugin;
// PluginManager 조회는 창 없이도 가능하다. debug 조건은 모듈 안에 있다.
#[cfg(debug_assertions)]
pub mod popup;
pub mod session;

#[cfg(feature = "gui")]
pub(crate) use checked::check_without_engine;
pub(crate) use checked::{CheckedRequest, check_request};

use std::borrow::Cow;

use serde_json::json;

use crate::core::CoreState;
use crate::ipc::alias;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

/// 이름은 등록돼 있지만 macOS GUI 조건이 맞지 않아 실행할 수 없음을 알린다.
/// 사용처가 debug 라우터이므로 상수도 같은 debug·플랫폼 조건으로 제한한다.
#[cfg(all(debug_assertions, not(all(target_os = "macos", feature = "gui"))))]
const PLATFORM_ONLY_MACOS_GUI: &str = "input reproduction over the OS event stream is macOS-only and needs the gui build \
     (CGEventPost / TISSelectInputSource have no equivalent here)";
use crate::ipc::window_port::IpcWindow;
use crate::state::AppState;

/// 호출자를 인증된 종류로 전달하고 공통 권한·cap·rate 검사를 수행한다.
/// 이미 검사한 요청은 handle_checked_request를 사용한다(ADR-0012).
///
/// 엔진 핸들러는 IpcWindow와 IntentOutbox로 창에 접근한다.
/// 창 자체를 조작하는 GUI·debug 핸들러만 AppState를 받는다(ADR-0002).
#[cfg(test)]
pub fn handle_with_caller(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    request: &JsonRpcRequest,
    caller: &CallerContext,
) -> JsonRpcResponse {
    match check_request(core, state, engine, request, caller) {
        Ok(checked) => handle_checked_request(core, state, engine, &checked),
        Err(response) => response,
    }
}

/// 검사한 요청을 실행하고 소요 시간을 기록한다. 예산과 사용량은 다시 집계하지 않는다.
/// AppState는 EntryWindow로 감싸고 요청의 intent를 해당 창 큐로 전달한다(ADR-0002).
pub(crate) fn handle_checked_request(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut CoreState,
    checked: &CheckedRequest<'_>,
) -> JsonRpcResponse {
    let started = core.now_instant();
    let mut window = entry_window::EntryWindow::new(state);
    let response = route_checked_request(core, &mut window, engine, checked);
    // 시작과 끝을 같은 Clock으로 재야 주입한 시계와 벽시계가 섞이지 않는다.
    let elapsed = core.now_instant().duration_since(started);
    core.pressure().record_handler(elapsed);
    response
}

/// 조기 반환도 계측되도록 실행 시간은 이 함수 밖에서 기록한다.
fn route_checked_request(
    core: &mut crate::core::Core,
    window: &mut entry_window::EntryWindow<'_>,
    engine: &mut CoreState,
    checked: &CheckedRequest<'_>,
) -> JsonRpcResponse {
    let request = checked.request();
    let caller = checked.caller();
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);
    let (_, routed) = canonicalize_and_route(request);
    let request = routed.as_ref();

    // alias 정규화 뒤 같은 멱등 키를 확인해 재시도는 handler를 다시 실행하지 않는다.
    let pending = match idempotency::begin(core.now_instant(), caller, request, &id) {
        Ok(pending) => pending,
        Err(answer) => return answer,
    };
    let response = dispatch_routed(core, window, engine, caller, request, id);
    idempotency::finish(core.now_instant(), pending, &response);
    response
}

/// 모든 반환값을 기록할 수 있도록 멱등 저장소 처리는 이 함수 밖에 둔다.
fn dispatch_routed(
    core: &mut crate::core::Core,
    window: &mut entry_window::EntryWindow<'_>,
    engine: &mut CoreState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> JsonRpcResponse {
    // 요청에서 생성한 intent를 순서대로 해당 창 큐에 옮긴다.
    let mut out = crate::ipc::window_port::IntentOutbox::default();
    let routed = route_engine_handler(
        core,
        window.port(),
        &mut out,
        engine,
        caller,
        request,
        id.clone(),
    );
    window.port().enqueue_intents(out);
    if let Some(resp) = routed {
        return resp;
    }

    #[cfg(feature = "gui")]
    if let Some(resp) = window.route_window(engine, caller, request, id.clone()) {
        return resp;
    }

    #[cfg(debug_assertions)]
    if let Some(resp) = window.route_debug(engine, request, id.clone()) {
        return resp;
    }

    JsonRpcResponse::unrouted_for_external_caller(id, &request.method)
}

/// alias를 정규화하고 deprecated 이름을 사용한 호출에 경고를 남긴다.
fn canonicalize_and_route(request: &JsonRpcRequest) -> (&str, Cow<'_, JsonRpcRequest>) {
    let canonical = alias::canonicalize(&request.method);
    if alias::is_deprecated(&request.method) {
        tracing::warn!(
            "ipc method '{}' is deprecated; use '{canonical}' (will be removed at 1.0)",
            request.method
        );
    }

    let routed: Cow<JsonRpcRequest> = if canonical == request.method {
        Cow::Borrowed(request)
    } else {
        Cow::Owned(JsonRpcRequest {
            response_timeout_ms: None,
            // alias로 호출해도 멱등 키는 유지한다.
            idempotency_key: request.idempotency_key.clone(),
            jsonrpc: request.jsonrpc.clone(),
            method: canonical.to_string(),
            params: request.params.clone(),
            id: request.id.clone(),
            session_token: request.session_token.clone(),
        })
    };
    (canonical, routed)
}

/// 권한 부족을 감사 기록과 오류로 반환한다.
/// Agent의 권한 부족은 공유 approval store에 승인 요청도 만든다.
pub(crate) fn check_permission_gate(
    core: &mut crate::core::Core,
    window: &mut dyn IpcWindow,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    canonical: &str,
    workspace_id: Option<u32>,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    if let Err(e) = caller.ensure_allowed(canonical) {
        tracing::warn!("ipc permission denied: {e}");
        let seq = engine.telemetry_seq.next();
        crate::ipc::audit::record(
            core,
            caller,
            canonical,
            crate::store::audit::AuditDecision::Deny,
            Some(&format!("{e}")),
            workspace_id,
            seq,
        );
        let mut response =
            JsonRpcResponse::error(id.clone(), -32001, format!("permission_denied: {e}"));
        // 알 수 없는 메서드·plugin 호출 금지는 권한을 추가해도 해결되지 않는다.
        // Plugin 권한은 매니페스트·grant로 관리하므로 Agent 승인 요청을 만들지 않는다.
        if let (
            tasty_ipc::caller::CallerError::MissingPermission { permission, .. },
            CallerContext::Agent { agent_id, .. },
        ) = (&e, caller)
        {
            let perm_token = permission.as_token();
            if let Some(record) = approval::publish_capability_elevation(
                core,
                window,
                engine,
                agent_id,
                canonical,
                &perm_token,
                None,
            ) && let Some(err) = response.error.as_mut()
            {
                err.data = Some(approval::elevation_error_data(
                    &record,
                    &perm_token,
                    canonical,
                ));
            }
        }
        return Some(response);
    }
    None
}

/// Pause·RequireApproval cap이 적용된 agent를 차단한다.
/// Local은 제외하므로 telemetry.cap.reset으로 해제할 수 있다.
pub(crate) fn check_cap_gate(
    core: &mut crate::core::Core,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    canonical: &str,
    workspace_id: Option<u32>,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    if let Some(reason) = telemetry::check_cap_block(core, caller, canonical) {
        tracing::warn!("ipc cap blocked: {reason}");
        let seq = engine.telemetry_seq.next();
        crate::ipc::audit::record(
            core,
            caller,
            canonical,
            crate::store::audit::AuditDecision::Deny,
            Some(&format!("cap_blocked: {reason}")),
            workspace_id,
            seq,
        );
        return Some(JsonRpcResponse::error(
            id.clone(),
            -32007,
            format!("cap_blocked: {reason}"),
        ));
    }
    None
}

/// ipc_calls 한도 초과를 -32010과 Deny로 반환한다. 복구 메서드는 제외한다.
/// 거절된 호출은 ipc_calls 대신 RateLimit.throttled_count에 집계한다.
pub(crate) fn check_rate_limit_gate(
    core: &mut crate::core::Core,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    canonical: &str,
    workspace_id: Option<u32>,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    if !should_rate_limit(caller, canonical) {
        return None;
    }
    let agent_id = caller.agent_id();
    let agent = agent_id.as_str();
    match core.rate_limit_try_consume(agent, "ipc_calls", 1, telemetry::now_ms()) {
        Ok(outcome) if !outcome.allowed => {
            let reason = format!("throttled: tokens_left={:.2}", outcome.tokens_left);
            tracing::warn!("ipc rate_limited: {reason}");
            let seq = engine.telemetry_seq.next();
            crate::ipc::audit::record(
                core,
                caller,
                canonical,
                crate::store::audit::AuditDecision::Deny,
                Some(&reason),
                workspace_id,
                seq,
            );
            Some(JsonRpcResponse::error(id.clone(), -32010, reason))
        }
        Ok(_) => None,
        Err(e) => {
            // fail-open: rate_limit 인프라 자체 실패는 전체 IPC 차단보다 통과 + warn.
            tracing::warn!("rate_limit middleware error: {e}");
            None
        }
    }
}

/// 허용된 비-host 호출을 집계한다. telemetry 자체 호출은 재귀 집계를 막기 위해 제외한다.
/// Allow 감사 기록의 보존 여부는 audit 모듈이 결정한다(ADR-0009).
fn record_telemetry_and_audit(
    core: &mut crate::core::Core,
    window: &mut dyn IpcWindow,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    canonical: &str,
    params: &serde_json::Value,
    workspace_id: Option<u32>,
) {
    // 게이트에서 만든 intent가 같은 요청의 handler intent보다 먼저 큐에 들어간다.
    let mut out = crate::ipc::window_port::IntentOutbox::default();
    telemetry::record_ipc_call(core, window, &mut out, engine, caller, canonical, params);
    window.enqueue_intents(out);

    let seq = engine.telemetry_seq.next();
    crate::ipc::audit::record(
        core,
        caller,
        canonical,
        crate::store::audit::AuditDecision::Allow,
        None,
        workspace_id,
        seq,
    );
}

/// Local과 호스트 호출은 제한하지 않는다.
/// telemetry는 재귀 집계를 피하고 agent.rate_limit은 자가 복구를 위해 제외한다.
/// system.info도 제한 없이 조회할 수 있다.
fn should_rate_limit(caller: &CallerContext, method: &str) -> bool {
    use crate::ipc::caller::CallerContext as C;
    match caller {
        C::Local => return false,
        C::Agent { .. } if caller.agent_id().is_host() => return false,
        _ => {}
    }
    if method.starts_with("telemetry.") {
        return false;
    }
    if method.starts_with("agent.rate_limit_") {
        return false;
    }
    if method == "system.info" {
        return false;
    }
    true
}

/// PluginManager가 측정한 plugin RSS를 이상 탐지에 전달한다.
/// Agent의 자체 보고는 telemetry.record에서 별도로 처리한다.
#[cfg(feature = "gui")]
pub fn record_plugin_rss_samples(
    core: &crate::core::Core,
    window: &mut dyn IpcWindow,
    engine: &mut crate::core::CoreState,
    samples: &[(String, u64)],
) {
    let ts = telemetry::now_ms();
    let mut out = crate::ipc::window_port::IntentOutbox::default();
    for (plugin_id, rss_bytes) in samples {
        telemetry::record_rss_sample(core, window, &mut out, engine, plugin_id, *rss_bytes, ts);
    }
    window.enqueue_intents(out);
}

/// 점유자가 아닌 호출자의 workspace 구조 변경을 IPC 라우터에서 거절한다.
/// holder의 forward는 structural_exec을 직접 호출하므로 이 검사를 거치지 않는다.
/// 공통 도메인 실행부에 검사하면 정당한 forward까지 막힌다(ADR-0021).
///
/// terminal.spawn은 pane 재지정 이후의 spawn_target_guard에서 검사한다.
/// convert는 아래 열거한 메서드만 검사하며 GUI의 직접 intent나 새 kind의 진입점은 포함하지 않는다.
/// 대상이 없거나 파라미터가 잘못되면 실제 핸들러가 오류를 반환하도록 넘긴다.
fn hard_occupied_structural_guard(
    core: &crate::core::Core,
    engine: &crate::core::CoreState,
    method: &str,
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    // 여기서는 소속만 찾고 잘못된 값은 handler의 require_*가 거절하게 한다.
    // 정수를 잘라 변환하면 다른 대상의 ID가 될 수 있으므로 범위를 검사한다.
    let ws_idx: usize = match method {
        "split" => {
            let target_pane = params::read_int::<u32>(params, "target_pane")
                .ok()
                .flatten();
            let target_surface = pane::resolve_surface_target(core, params);
            target_pane
                .and_then(|pid| engine.find_workspace_index_for_pane(pid))
                .or_else(|| {
                    target_surface
                        .and_then(|sid| engine.find_workspace_index_for_surface(sid))
                        .map(|(i, _)| i)
                })?
        }
        "workspace.close" => {
            if let Some(ws_id) = params::read_int::<u32>(params, "id").ok().flatten() {
                engine.workspaces.iter().position(|w| w.id == ws_id)?
            } else {
                params::read_int::<usize>(params, "index").ok().flatten()?
            }
        }
        "tab.create" | "pane.close" | "tab.move" => {
            let pane_id = params::read_int::<u32>(params, "pane_id").ok().flatten()?;
            engine.find_workspace_index_for_pane(pane_id)?
        }
        "tab.close" => {
            let tab_id = params::read_int::<u32>(params, "tab_id").ok().flatten()?;
            let pane_id = engine.find_pane_for_tab(tab_id)?;
            engine.find_workspace_index_for_pane(pane_id)?
        }
        "surface.close" | "markdown.navigate" | "image.open" => {
            let surface_id = params::read_int::<u32>(params, "surface_id")
                .ok()
                .flatten()?;
            engine
                .find_workspace_index_for_surface(surface_id)
                .map(|(i, _)| i)?
        }
        _ => return None,
    };
    let ws_id = engine.workspaces.get(ws_idx)?.id;
    if engine.attach.workspace_holder(ws_id).is_some() {
        return Some(hard_occupied_denial(ws_id, id));
    }
    None
}

/// 라우터와 spawn 가드가 같은 점유 거절 응답을 사용한다.
fn hard_occupied_denial(ws_id: u32, id: &serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::invalid_params(
        id.clone(),
        format!(
            "Workspace {ws_id} is occupied by a remote attach session (hard-occupied) — \
             structural changes (new tab/split/close/move) must come from that session. \
             Use a different workspace."
        ),
    )
}

/// pane 재지정까지 끝난 실제 대상의 mirror·hard 점유를 생성 전에 검사한다.
/// workspace 인자만 검사하면 다른 workspace의 pane을 지정해 우회할 수 있다.
///
/// spawn은 holder가 forward하는 구조 변경에 포함되지 않아 이 위치에서 거절해도 된다.
/// 나머지 mirror 구조 변경은 원격 전달을 허용하므로 mirror 검사를 공통 라우터에 넣지 않는다(ADR-0021).
fn spawn_target_guard(
    engine: &crate::core::CoreState,
    pane_id: u32,
    id: &serde_json::Value,
) -> Option<JsonRpcResponse> {
    let ws_idx = engine.find_workspace_index_for_pane(pane_id)?;
    let ws = engine.workspaces.get(ws_idx)?;
    let ws_id = ws.id;
    if ws.mirror {
        return Some(JsonRpcResponse::invalid_params(
            id.clone(),
            format!(
                "Workspace {ws_id} is a mirror of a remote attach session — child terminals \
                 cannot be spawned into it. Structural changes there are forwarded to the \
                 remote instance and complete asynchronously, so no local child is created. \
                 Use a different workspace, or spawn from the remote instance directly."
            ),
        ));
    }
    if engine.attach.workspace_holder(ws_id).is_some() {
        return Some(hard_occupied_denial(ws_id, id));
    }
    None
}

fn route_engine_handler(
    core: &mut crate::core::Core,
    window: &mut dyn IpcWindow,
    out: &mut crate::ipc::window_port::IntentOutbox,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    if let Some(resp) =
        hard_occupied_structural_guard(core, engine, &request.method, &request.params, &id)
    {
        return Some(resp);
    }
    Some(match request.method.as_str() {
        "system.info" => handle_system_info(window, engine, id),
        "system.pressure" => pressure::handle_system_pressure(&*core, engine, id),
        "workspace.list" => workspace::handle_workspace_list(window, engine, id),
        "workspace.create" => {
            workspace::handle_workspace_create(core, window, engine, id, &request.params)
        }
        "workspace.update" => {
            workspace::handle_workspace_update(core, window, engine, id, &request.params)
        }
        "workspace.move" => {
            workspace::handle_workspace_move(core, window, engine, id, &request.params)
        }
        "workspace.close" => workspace::handle_workspace_close(window, engine, id, &request.params),
        "workspace_category.list" => workspace_category::handle_list(engine, id),
        "workspace_category.create" => {
            workspace_category::handle_create(engine, id, &request.params)
        }
        "workspace_category.rename" => {
            workspace_category::handle_rename(engine, id, &request.params)
        }
        "workspace_category.delete" => {
            workspace_category::handle_delete(engine, id, &request.params)
        }
        "workspace_category.move" => workspace_category::handle_move(engine, id, &request.params),
        "pane.list" => pane::handle_pane_list(engine, id),
        "pane.close" => pane::handle_pane_close(core, window, engine, id, &request.params),
        "split" => pane::handle_split(core, window, engine, id, &request.params),
        "tab.list" => tab::handle_tab_list(engine, id, &request.params),
        "tab.create" => tab::handle_tab_create(core, window, engine, id, &request.params),
        "tab.close" => tab::handle_tab_close(core, window, engine, id, &request.params),
        "tab.move" => tab::handle_tab_move(core, engine, id, &request.params),
        // terminal: child-terminal 관리와 점유 검사 (ADR-0021)
        "terminal.spawn" => terminal::handle_spawn(core, window, engine, id, &request.params),
        "terminal.tell" => terminal::handle_tell(core, engine, id, &request.params),
        "terminal.children" => terminal::handle_children(engine, id, &request.params),
        "terminal.parent" => terminal::handle_parent(engine, id, &request.params),
        "terminal.state" => terminal::handle_state(engine, id, &request.params),
        "terminal.kill" => terminal::handle_kill(core, window, engine, id, &request.params),
        "terminal.respawn" => terminal::handle_respawn(core, engine, id, &request.params),
        "terminal.broadcast" => terminal::handle_broadcast(core, engine, id, &request.params),
        "terminal.set_state" => terminal::handle_set_state(engine, id, &request.params),
        "terminal.adopt" => terminal::handle_adopt(engine, id, &request.params),
        "terminal.release" => terminal::handle_release(engine, id, &request.params),
        // Surface에 연결하지 않은 백그라운드 PTY.
        "pty.spawn" => pty::handle_spawn(core, engine, caller, id, &request.params),
        "pty.write" => pty::handle_write(engine, id, &request.params),
        "pty.read" => pty::handle_read(engine, id, &request.params),
        "pty.wait" => pty::handle_wait(engine, id, &request.params),
        "pty.kill" => pty::handle_kill(engine, id, &request.params),
        "pty.list" => pty::handle_list(engine, id),
        "pty.attach_surface" => {
            pty::handle_attach_surface(core, window, engine, id, &request.params)
        }
        // preset (layout preset CRUD + apply)
        "preset.list" => preset::handle_list(core, id, &request.params),
        "preset.get" => preset::handle_get(core, id, &request.params),
        "preset.save" => preset::handle_save(core, id, &request.params),
        "preset.delete" => preset::handle_delete(core, id, &request.params),
        "preset.rename" => preset::handle_rename(core, id, &request.params),
        "preset.capture" => preset::handle_capture(core, engine, id, &request.params),
        "preset.apply" => preset::handle_apply(core, window, engine, id, &request.params),
        "surface.close" => surface::handle_surface_close(core, window, engine, id, &request.params),
        "surface.close_self" => {
            surface::handle_surface_close_self(core, window, engine, id, &request.params)
        }
        "surface.list" => surface::handle_surface_list(engine, id),
        "surface.kinds" => surface::handle_surface_kinds(engine, id),
        "surface.send" => surface::handle_surface_send(core, engine, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(core, engine, id, &request.params),
        "surface.send_combo" => {
            surface::handle_surface_send_combo(core, engine, id, &request.params)
        }
        "surface.send_to" => surface::handle_surface_send_to(core, engine, id, &request.params),
        "surface.wake" => surface::handle_surface_wake(engine, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(out, engine, id, &request.params),
        "surface.completion" => surface::handle_completion(out, engine, id, &request.params),
        "surface.attention.get" => surface::handle_attention_get(engine, id, &request.params),
        "surface.attention.clear" => {
            surface::handle_attention_clear(out, engine, id, &request.params)
        }
        "surface.read_since_mark" => surface::handle_read_since_mark(engine, id, &request.params),
        "surface.read_since_scan_mark" => {
            surface::handle_read_since_scan_mark(engine, id, &request.params)
        }
        "surface.parse_since_mark" => surface::handle_parse_since_mark(engine, id, &request.params),
        "surface.commands" => surface::handle_commands(core, engine, id, &request.params),
        "surface.last_command" => surface::handle_last_command(core, engine, id, &request.params),
        "surface.command_at" => surface::handle_command_at(core, engine, id, &request.params),
        "output.observe_start" => output::handle_observe_start(core, engine, id, &request.params),
        "output.observe_stop" => output::handle_observe_stop(core, engine, id, &request.params),
        "output.observe_list" => output::handle_observe_list(core, engine, id),
        "output.observe_info" => output::handle_observe_info(core, engine, id, &request.params),
        "surface.screen_text" => surface::handle_screen_text(engine, id, &request.params),
        "surface.cursor_position" => surface::handle_cursor_position(engine, id, &request.params),
        "surface.mouse_tracking" => surface::handle_mouse_tracking(engine, id, &request.params),
        "surface.foreground_process" => {
            surface::handle_foreground_process(engine, id, &request.params)
        }
        "surface.locate" => surface::handle_surface_locate(engine, id, &request.params),
        "surface.respawn_terminal" => {
            surface::handle_surface_respawn_terminal(core, engine, id, &request.params)
        }
        "surface.is_typing" => handle_is_typing(engine, id, &request.params),
        "surface.send_wait_idle" => handle_send_wait_idle(engine, id, &request.params),
        "surface.fire_hook" => {
            hooks::handle_surface_fire_hook(core, window, engine, id, &request.params)
        }
        "surface.meta.set" => meta::handle_surface_meta_set(core, engine, id, &request.params),
        "surface.meta.get" => meta::handle_surface_meta_get(core, engine, id, &request.params),
        "surface.meta.unset" => meta::handle_surface_meta_unset(core, engine, id, &request.params),
        "surface.meta.list" => meta::handle_surface_meta_list(core, engine, id, &request.params),
        "surface.set_cwd" => surface::handle_set_cwd(engine, id, &request.params),
        "hook.set" => hooks::handle_hook_set(core, engine, id, &request.params),
        "hook.list" => hooks::handle_hook_list(engine, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(core, engine, id, &request.params),
        "global_hook.set" => hooks::handle_global_hook_set(core, engine, id, &request.params),
        "global_hook.list" => hooks::handle_global_hook_list(engine, id),
        "global_hook.unset" => hooks::handle_global_hook_unset(core, engine, id, &request.params),
        "webhook.register" => webhook::handle_register(caller, id, &request.params),
        "webhook.list" => webhook::handle_list(id),
        "webhook.info" => webhook::handle_info(id, &request.params),
        "webhook.unregister" => webhook::handle_unregister(id, &request.params),
        "webhook.sweep" => webhook::handle_sweep(id),
        "webhook.config" => webhook::handle_config(id, &request.params),
        #[cfg(feature = "gui")]
        "webview.set_url" => webview::handle_set_url(engine, caller, id, &request.params),
        // WebView는 surface.set_context를 받지 않아 이 조회로 Theme를 읽는다.
        "theme.query" => theme::handle_query(engine, id),
        "tree" => handle_tree(window, engine, id),
        "message.send" => message::handle_message_send(core, engine, id, &request.params),
        "message.read" => message::handle_message_read(core, engine, id, &request.params),
        "message.count" => message::handle_message_count(engine, id, &request.params),
        "message.clear" => message::handle_message_clear(core, engine, id, &request.params),
        "notification.list" => notification::handle_notification_list(engine, id),
        "notification.create" => {
            notification::handle_notification_create(out, engine, id, &request.params)
        }
        "file_handler.reload" => file_handler::handle_reload(core, engine, id),
        "file_handler.detectors" => file_handler::handle_detectors(engine, id),
        // identify worker와 결과를 여는 창이 GUI에만 있다.
        // 헤드리스에서는 예약 성공 뒤 요청을 버리지 않도록 라우팅하지 않는다(ADR-0031).
        #[cfg(feature = "gui")]
        "file_handler.dispatch" => {
            file_handler::handle_dispatch(out, window, engine, caller, id, request.params.clone())
        }
        "hook_handler.list" => hook_handler::handle_list(id),
        "hook_handler.get" => hook_handler::handle_get(id, &request.params),
        "hook_handler.upsert" => hook_handler::handle_upsert(id, &request.params),
        "hook_handler.remove" => hook_handler::handle_remove(id, &request.params),
        "hook_handler.reload" => hook_handler::handle_reload(id),
        "hook_handler.dispatch" => hook_handler::handle_dispatch(core, id, &request.params),
        "completion_strategy.list" => completion_strategy::handle_list(id),
        #[cfg(feature = "gui")]
        "markdown.navigate" => markdown::handle_navigate(out, id, request.params.clone()),
        // kind에 상관없이 최근 목록만 조회하므로 GUI가 필요 없다.
        "recent.query" => recent::handle_query(window, id, request.params.clone()),
        // 결과를 전달하는 App::dispatch_pending_git_query_forwards가 GUI 전용이다(ADR-0022).
        #[cfg(feature = "gui")]
        "git_viewer.query" => git_viewer::handle_query(engine, id, &request.params),
        // mirror 원문도 GUI의 전달 큐에서 처리한다(ADR-0022).
        #[cfg(feature = "gui")]
        "markdown_mirror.content_request" => {
            markdown_mirror::handle_content_request(engine, id, &request.params)
        }
        // host는 surface 변환·목록만 처리하고 픽셀 편집은 plugin이 처리한다.
        #[cfg(feature = "gui")]
        "image.open" => image::handle_open(core, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "image.list" => image::handle_list(engine, id),
        "memory.put" => memory::handle_put(core, engine, caller, id, &request.params),
        "memory.get" => memory::handle_get(core, engine, caller, id, &request.params),
        "memory.delete" => memory::handle_delete(core, engine, caller, id, &request.params),
        "memory.list" => memory::handle_list(core, engine, caller, id, &request.params),
        "memory.exists" => memory::handle_exists(core, engine, caller, id, &request.params),
        "memory.count" => memory::handle_count(core, engine, caller, id, &request.params),
        "memory.scopes" => memory::handle_scopes(core, engine, caller, id, &request.params),
        "memory.stats" => memory::handle_stats(core, engine, caller, id, &request.params),
        "memory.query" => memory::handle_query(core, engine, caller, id, &request.params),
        "memory.export" => memory::handle_export(core, engine, caller, id, &request.params),
        "memory.import" => memory::handle_import(core, engine, caller, id, &request.params),
        "memory.secret.put" => memory::handle_secret_put(core, engine, caller, id, &request.params),
        "memory.secret.get" => memory::handle_secret_get(core, engine, caller, id, &request.params),
        "memory.secret.delete" => {
            memory::handle_secret_delete(core, engine, caller, id, &request.params)
        }
        "memory.secret.list" => {
            memory::handle_secret_list(core, engine, caller, id, &request.params)
        }
        "memory.secret.exists" => {
            memory::handle_secret_exists(core, engine, caller, id, &request.params)
        }
        "memory.secret.count" => {
            memory::handle_secret_count(core, engine, caller, id, &request.params)
        }
        "memory.secret.scopes" => {
            memory::handle_secret_scopes(core, engine, caller, id, &request.params)
        }
        "memory.secret.stats" => {
            memory::handle_secret_stats(core, engine, caller, id, &request.params)
        }
        "memory.gc" => memory::handle_gc(core, engine, caller, id, &request.params),
        "memory.bb_create" => memory::handle_bb_create(core, engine, caller, id, &request.params),
        "memory.bb_put" => memory::handle_bb_put(core, engine, caller, id, &request.params),
        "memory.bb_get" => memory::handle_bb_get(core, engine, caller, id, &request.params),
        "memory.bb_get_all" => memory::handle_bb_get_all(core, engine, caller, id, &request.params),
        "memory.bb_get_meta" => {
            memory::handle_bb_get_meta(core, engine, caller, id, &request.params)
        }
        "memory.bb_delete_field" => {
            memory::handle_bb_delete_field(core, engine, caller, id, &request.params)
        }
        "memory.bb_delete" => memory::handle_bb_delete(core, engine, caller, id, &request.params),
        "memory.bb_list" => memory::handle_bb_list(core, engine, caller, id, &request.params),
        "memory.bb_exists" => memory::handle_bb_exists(core, engine, caller, id, &request.params),
        "memory.bb_snapshot" => {
            memory::handle_bb_snapshot(core, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_get" => {
            memory::handle_bb_snapshot_get(core, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_list" => {
            memory::handle_bb_snapshot_list(core, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_delete" => {
            memory::handle_bb_snapshot_delete(core, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_restore" => {
            memory::handle_bb_snapshot_restore(core, engine, caller, id, &request.params)
        }
        "memory.plan_create" => {
            memory::handle_plan_create(core, engine, caller, id, &request.params)
        }
        "memory.plan_get" => memory::handle_plan_get(core, engine, caller, id, &request.params),
        "memory.plan_list" => memory::handle_plan_list(core, engine, caller, id, &request.params),
        "memory.plan_delete" => {
            memory::handle_plan_delete(core, engine, caller, id, &request.params)
        }
        "memory.plan_add_step" => {
            memory::handle_plan_add_step(core, engine, caller, id, &request.params)
        }
        "memory.plan_remove_step" => {
            memory::handle_plan_remove_step(core, engine, caller, id, &request.params)
        }
        "memory.plan_update_step" => {
            memory::handle_plan_update_step(core, engine, caller, id, &request.params)
        }
        "memory.cache_put" => memory::handle_cache_put(core, engine, caller, id, &request.params),
        "memory.cache_get" => memory::handle_cache_get(core, engine, caller, id, &request.params),
        "memory.cache_invalidate" => {
            memory::handle_cache_invalidate(core, engine, caller, id, &request.params)
        }
        "memory.cache_clear" => {
            memory::handle_cache_clear(core, engine, caller, id, &request.params)
        }
        "memory.cache_list" => memory::handle_cache_list(core, engine, caller, id, &request.params),
        "memory.goal_set" => memory::handle_goal_set(core, engine, caller, id, &request.params),
        "memory.goal_get" => memory::handle_goal_get(core, engine, caller, id, &request.params),
        "memory.goal_clear" => memory::handle_goal_clear(core, engine, caller, id, &request.params),
        "settings.get_plugin_setting" => {
            settings::handle_get_plugin_setting(engine, caller, id, &request.params)
        }
        "settings.get_remote_transfer" => settings::handle_get_remote_transfer(engine, id),
        "settings.get_input_rules" => terminal_input::get(engine, id),
        "settings.set_input_rule"
        | "settings.remove_input_rule"
        | "settings.initialize_input_rule" => terminal_input::handle_input_rule_update(
            out,
            engine,
            caller,
            id,
            &request.params,
            &request.method,
        ),
        "settings.set_remote_transfer" => {
            settings::handle_set_remote_transfer(out, engine, id, &request.params)
        }
        // approval.await는 별도 워커에서 대기한다.
        "approval.request" => {
            approval::handle_request(core, window, engine, caller, id, &request.params)
        }
        "approval.respond" => approval::handle_respond(core, engine, caller, id, &request.params),
        "approval.cancel" => approval::handle_cancel(core, engine, caller, id, &request.params),
        "approval.get" => approval::handle_get(core, engine, caller, id, &request.params),
        "approval.list" => approval::handle_list(core, engine, caller, id, &request.params),
        "approval.history" => approval::handle_history(core, engine, caller, id, &request.params),
        "approval.summary.set" => {
            approval::handle_summary_set(core, engine, caller, id, &request.params)
        }
        "approval.summary.get" => {
            approval::handle_summary_get(core, engine, caller, id, &request.params)
        }
        "telemetry.record" => {
            telemetry::handle_record(core, window, out, engine, caller, id, &request.params)
        }
        "telemetry.record_batch" => {
            telemetry::handle_record_batch(core, window, out, engine, caller, id, &request.params)
        }
        "telemetry.summary" => telemetry::handle_summary(core, engine, caller, id, &request.params),
        "telemetry.timeseries" => {
            telemetry::handle_timeseries(core, engine, caller, id, &request.params)
        }
        "telemetry.top" => telemetry::handle_top(core, engine, caller, id, &request.params),
        "telemetry.cap.set" => telemetry::handle_cap_set(core, engine, caller, id, &request.params),
        "telemetry.cap.list" => {
            telemetry::handle_cap_list(core, engine, caller, id, &request.params)
        }
        "telemetry.cap.remove" => {
            telemetry::handle_cap_remove(core, engine, caller, id, &request.params)
        }
        "telemetry.cap.status" => {
            telemetry::handle_cap_status(core, engine, caller, id, &request.params)
        }
        "telemetry.cap.reset" => {
            telemetry::handle_cap_reset(core, engine, caller, id, &request.params)
        }
        "telemetry.anomaly.list" => {
            telemetry::handle_anomaly_list(core, engine, caller, id, &request.params)
        }
        "telemetry.session_summary" => {
            telemetry::handle_session_summary(core, engine, caller, id, &request.params)
        }
        "agent.task_create" => agent::handle_task_create(core, engine, caller, id, &request.params),
        "agent.task_list" => agent::handle_task_list(core, engine, caller, id, &request.params),
        "agent.task_get" => agent::handle_task_get(core, engine, caller, id, &request.params),
        // agent.task_await는 GUI·헤드리스 모두 상위 라우터가 별도 워커로 처리한다.
        "agent.task_cancel" => agent::handle_task_cancel(core, engine, caller, id, &request.params),
        "agent.task_retry" => agent::handle_task_retry(core, engine, caller, id, &request.params),
        "agent.task_graph" => agent::handle_task_graph(core, engine, caller, id, &request.params),
        "agent.dag_list" => agent::handle_dag_list(core, engine, caller, id, &request.params),
        "agent.dag_get" => agent::handle_dag_get(core, engine, caller, id, &request.params),
        "agent.task_set_result" => {
            agent::handle_task_set_result(core, engine, caller, id, &request.params)
        }
        "agent.task_run" => agent::handle_task_run(core, engine, caller, id, &request.params),
        "agent.task_delete" => agent::handle_task_delete(core, engine, caller, id, &request.params),
        "agent.task_purge" => agent::handle_task_purge(core, engine, caller, id, &request.params),
        "agent.barrier_create" => {
            agent::handle_barrier_create(core, engine, caller, id, &request.params)
        }
        "agent.barrier_signal" => {
            agent::handle_barrier_signal(core, engine, caller, id, &request.params)
        }
        "agent.barrier_await" => {
            agent::handle_barrier_await(core, engine, caller, id, &request.params)
        }
        "agent.barrier_state" => {
            agent::handle_barrier_state(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_create" => {
            agent::handle_semaphore_create(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_set_permits" => {
            agent::handle_semaphore_set_permits(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_acquire" => {
            agent::handle_semaphore_acquire(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_release" => {
            agent::handle_semaphore_release(core, engine, caller, id, &request.params)
        }
        "agent.barrier_list" => {
            agent::handle_barrier_list(core, engine, caller, id, &request.params)
        }
        "agent.barrier_delete" => {
            agent::handle_barrier_delete(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_list" => {
            agent::handle_semaphore_list(core, engine, caller, id, &request.params)
        }
        "agent.semaphore_delete" => {
            agent::handle_semaphore_delete(core, engine, caller, id, &request.params)
        }
        "agent.lease_acquire" => {
            agent::handle_lease_acquire(core, engine, caller, id, &request.params)
        }
        "agent.lease_release" => {
            agent::handle_lease_release(core, engine, caller, id, &request.params)
        }
        "agent.lease_list" => agent::handle_lease_list(core, engine, caller, id, &request.params),
        "agent.task_reduce" => agent::handle_task_reduce(core, engine, caller, id, &request.params),
        "agent.rate_limit_set" => {
            agent::handle_rate_limit_set(core, engine, caller, id, &request.params)
        }
        "agent.rate_limit_list" => {
            agent::handle_rate_limit_list(core, engine, caller, id, &request.params)
        }
        "agent.rate_limit_remove" => {
            agent::handle_rate_limit_remove(core, engine, caller, id, &request.params)
        }
        "agent.rate_limit_status" => {
            agent::handle_rate_limit_status(core, engine, caller, id, &request.params)
        }
        "session.issue" => session::handle_issue(core, caller, id, &request.params),
        "session.revoke" => session::handle_revoke(core, id, &request.params),
        "session.list" => session::handle_list(core, id),
        "attach.acquire" => attach::handle_acquire(engine, id, &request.params),
        "attach.release" => attach::handle_release(engine, id, &request.params),
        "attach.force_detach" => attach::handle_force_detach(engine, id, &request.params),
        "attach.force_detach_workspace" => {
            attach::handle_force_detach_workspace(engine, id, &request.params)
        }
        "attach.into_gui" => attach::handle_into_gui(engine, id, &request.params),
        "attach.list" => attach::handle_list(engine, id),
        "remote.profile.list" => remote_profile::handle_list(id),
        "remote.profile.get" => remote_profile::handle_get(id, &request.params),
        "remote.profile.add" => remote_profile::handle_add(id, &request.params),
        "remote.profile.detect" => remote_profile::handle_detect(id, &request.params),
        "remote.profile.remove" => remote_profile::handle_remove(id, &request.params),
        // SSH를 실행하지 않고 로컬 설정 파일을 읽는다.
        "remote.profile.list_local" => remote_profile::handle_list_local(id),
        "remote.profile.import" => remote_profile::handle_import(id, &request.params),
        // 자격증명 조회는 비밀 값과 파일 경로를 반환하지 않는다.
        "remote.passkey.list" => passkey::handle_list(id),
        "remote.passkey.get" => passkey::handle_get(id, &request.params),
        "remote.passkey.add" => passkey::handle_add(id, &request.params),
        "remote.passkey.remove" => passkey::handle_remove(id, &request.params),
        _ => return None,
    })
}

/// 창 상태를 조작하는 GUI 핸들러는 EntryWindow를 통해 AppState를 받는다(ADR-0002).
/// window_router_caller_tests는 아래 match 팔과 호출자 명부를 대조한다.
/// match 밖의 분기는 검사에서 빠질 수 있으므로 새 진입점의 호출자 제한도 직접 확인한다.
#[cfg(feature = "gui")]
fn route_window_handler(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "file_picker.trigger" => {
            file_picker::handle_trigger(state, engine, caller, id, &request.params)
        }
        _ => return None,
    })
}

#[cfg(debug_assertions)]
fn route_debug_handler(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "ui.state" => debug_state::handle_ui_state(state, engine, id),
        // 설정 적용은 창이 없어도 동작하므로 debug_state에 둔다.
        "debug.settings.apply" => {
            debug_state::handle_debug_settings_apply(state, engine, id, &request.params)
        }
        // present를 막아 이벤트 루프 정지와 watchdog 진단을 재현한다.
        #[cfg(feature = "gui")]
        "debug.gpu.stall" => debug::handle_debug_gpu_stall(id, &request.params),
        // 터미널 그리드 조회·수정은 헤드리스 debug에서도 제공한다.
        "debug.cell_info" => debug_terminal::handle_debug_cell_info(engine, id, &request.params),
        "debug.screen_attrs" => {
            debug_terminal::handle_debug_screen_attrs(engine, id, &request.params)
        }
        "debug.glyph_color" => {
            debug_terminal::handle_debug_glyph_color(engine, id, &request.params)
        }
        "debug.feed_bytes" => debug_terminal::handle_debug_feed_bytes(engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.inject_mouse" => debug::handle_debug_inject_mouse(engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.inject_key" => debug::handle_debug_inject_key(engine, id, &request.params),
        // surface 이름을 쓰지만 CGEvent/TIS는 OS 전역에 작용한다. debug로 제한한다(ADR-0012).
        #[cfg(all(target_os = "macos", feature = "gui"))]
        "surface.switch_input_source" => {
            input_source::handle_switch_input_source(state, engine, id, &request.params)
        }
        #[cfg(all(target_os = "macos", feature = "gui"))]
        "surface.raw_key" => input_source::handle_raw_key(state, engine, id, &request.params),
        // CLI와 메서드 표에는 같은 이름이 있으므로 오타 대신 플랫폼 미지원 오류를 반환한다.
        #[cfg(not(all(target_os = "macos", feature = "gui")))]
        "surface.switch_input_source" | "surface.raw_key" => {
            JsonRpcResponse::error(id.clone(), -32015, PLATFORM_ONLY_MACOS_GUI)
        }
        "debug.close_workspace" => {
            debug_nav::handle_debug_close_workspace(state, engine, id, &request.params)
        }
        "debug.switch_workspace" => {
            debug_nav::handle_debug_switch_workspace(state, engine, id, &request.params)
        }
        "debug.switch_tab" => {
            debug_nav::handle_debug_switch_tab(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.tool.list" => tool::handle_list(state, engine, id),
        #[cfg(feature = "gui")]
        "debug.tool.invoke" => tool::handle_invoke(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.host_popup.list" => debug::handle_debug_host_popup_list(state, id),
        #[cfg(feature = "gui")]
        "debug.host_popup.open" => {
            debug::handle_debug_host_popup_open(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.host_popup.close" => {
            debug::handle_debug_host_popup_close(state, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.modifier_hint.hold" => {
            debug::handle_debug_modhint_hold(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.modifier_hint.state" => debug::handle_debug_modhint_state(state, engine, id),
        #[cfg(feature = "gui")]
        "debug.banner.list" => debug::handle_debug_banner_list(state, id),
        #[cfg(feature = "gui")]
        "debug.banner.show" => debug::handle_debug_banner_show(state, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.banner.close" => debug::handle_debug_banner_close(state, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.banner.set_countdown" => {
            debug::handle_debug_banner_set_countdown(state, id, &request.params)
        }
        _ => return None,
    })
}

/// 필수 surface_id의 타입·u32 범위·Surface ID 공간을 검사한다.
/// PTY_ID_BASE 이상의 값이 memory scope에 들어가면 다음 부팅의 surface 카운터를
/// PTY 공간으로 올릴 수 있으므로 거절한다(ADR-0017).
pub(super) fn require_surface_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    let raw = match params::require_u32(params, "surface_id", id) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };
    if !crate::core::pty_registry::is_surface_id_space(raw) {
        return Err(JsonRpcResponse::invalid_params(
            id.clone(),
            format!("'surface_id' {raw} is inside the headless PTY id space"),
        ));
    }
    Ok(raw)
}

fn require_pane_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params::require_u32(params, "pane_id", id)
}

/// 알림을 돌려줄 caller_surface_id는 부가 정보다.
/// 범위는 검사하되 잘못된 값 때문에 본 요청을 거절하지는 않는다.
pub(super) fn caller_surface_id(params: &serde_json::Value) -> Option<u32> {
    params::read_int::<u32>(params, "caller_surface_id")
        .ok()
        .flatten()
}

fn surface_belongs_to_pane(engine: &CoreState, surface_id: u32, pane_id: u32) -> bool {
    engine.find_pane_for_surface(surface_id) == Some(pane_id)
}

/// 원격 큐에 넣은 구조 변경은 forwarded:true로 답한다. 원격 완료를 보장하는 응답은 아니다.
/// 원격 결과는 이후 delta로 확인하며 전달 불가·일반 오류는 internal_error로 반환한다.
/// 헤드리스에는 전달 큐 소비자가 없어 mirror 구조 변경을 거절한다(ADR-0003).
pub(super) fn structural_apply_error(id: serde_json::Value, e: &anyhow::Error) -> JsonRpcResponse {
    if let Some(blocked) = e.downcast_ref::<crate::core::MirrorStructuralBlocked>()
        && blocked.forwarded
    {
        return JsonRpcResponse::success(
            id,
            json!({
                "forwarded": true,
                "workspace_index": blocked.workspace_index,
            }),
        );
    }
    JsonRpcResponse::internal_error(id, e.to_string())
}

/// 도메인 오류 종류를 JSON-RPC 코드로 바꾸고 forward 경로와 같은 사유를 보존한다.
pub(super) fn structural_failure_response(
    id: serde_json::Value,
    failure: crate::core::structural_exec::StructuralFailure,
) -> JsonRpcResponse {
    use crate::core::structural_exec::StructuralFailure;
    match failure {
        StructuralFailure::Rejected(msg) => JsonRpcResponse::invalid_params(id, msg),
        StructuralFailure::MissingEvent(msg) => JsonRpcResponse::internal_error(id, msg),
        StructuralFailure::Apply(e) => structural_apply_error(id, &e),
    }
}

/// 서버 capability는 창별로 재사용하는 system_info_fields가 아닌 이 응답에만 추가한다.
fn handle_system_info(
    window: &dyn IpcWindow,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut info = system_info_fields(window, engine);
    info["capabilities"] = tasty_ipc::capability::capabilities_json();
    // 재시도 가능 여부를 판단할 수 있도록 보존 시간·개수·응답 크기도 제공한다.
    info["idempotency"] = idempotency::declaration();
    JsonRpcResponse::success(id, info)
}

/// Version is process-wide; the legacy count/index describe this engine. Include
/// its workspace IDs so an observation never silently looks like a global count.
/// window.list reuses the same fields beside the OS window ID.
pub(crate) fn system_info_fields(window: &dyn IpcWindow, engine: &CoreState) -> serde_json::Value {
    let active_workspace = window.active_workspace_index();
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "scope": "engine",
        "layout_slot": engine.layout_slot,
        "workspace_count": engine.workspaces.len(),
        "workspace_ids": engine.workspaces.iter().map(|ws| ws.id).collect::<Vec<_>>(),
        "active_workspace": active_workspace,
        "active_workspace_id": engine.workspaces.get(active_workspace).map(|ws| ws.id),
    })
}

fn handle_tree(
    window: &dyn IpcWindow,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    JsonRpcResponse::success(id, json!(build_engine_tree(window, engine)))
}

/// IPC와 Lua 스냅샷이 같은 트리 필드를 사용하도록 공통 JSON을 만든다.
pub(crate) fn build_engine_tree(
    window: &dyn IpcWindow,
    engine: &crate::core::CoreState,
) -> Vec<serde_json::Value> {
    engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let mut t = ws.to_tree_json();
            t["active"] = json!(i == window.active_workspace_index());
            t["busy_count"] = json!(engine.busy_count(&ws.all_surface_ids()));
            annotate_tree_busy(&mut t, engine);
            t
        })
        .collect()
}

/// Walk a workspace tree JSON value and annotate every node that owns surface
/// ids with a `busy_count` field. Surface-leaf nodes also get a `busy` boolean.
fn annotate_tree_busy(node: &mut serde_json::Value, engine: &CoreState) {
    if let Some(obj) = node.as_object_mut() {
        // Surface leaf: has "id" but no "tabs"/"panes"/"first"/"second"
        let is_leaf = !obj.contains_key("tabs")
            && !obj.contains_key("panes")
            && !obj.contains_key("first")
            && !obj.contains_key("second")
            && obj.get("id").is_some();
        if is_leaf {
            if let Some(sid) = obj.get("id").and_then(|v| v.as_u64()) {
                obj.insert("busy".into(), json!(engine.is_surface_busy(sid as u32)));
            }
            return;
        }

        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for child in arr.iter_mut() {
                    annotate_tree_busy(child, engine);
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get_mut(key) {
                annotate_tree_busy(child, engine);
            }
        }

        let mut count: u64 = 0;
        for key in ["panes", "tabs"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                for child in arr {
                    count += child
                        .get("busy_count")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                        && child.get("busy_count").is_none()
                    {
                        count += 1;
                    }
                }
            }
        }
        for key in ["first", "second", "surface"] {
            if let Some(child) = obj.get(key) {
                count += child
                    .get("busy_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if child.get("busy").and_then(|v| v.as_bool()).unwrap_or(false)
                    && child.get("busy_count").is_none()
                {
                    count += 1;
                }
            }
        }
        // Workspaces already had busy_count set by the caller; only insert if missing.
        if !obj.contains_key("busy_count") {
            obj.insert("busy_count".into(), json!(count));
        }
    }
}

fn handle_is_typing(
    engine: &CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let typing = engine.is_typing(surface_id);
    let idle_seconds = if let Some(last) = engine.last_key_input.get(&surface_id) {
        last.elapsed().as_secs_f64()
    } else {
        f64::MAX
    };
    let idle_seconds_capped = if idle_seconds == f64::MAX {
        -1.0
    } else {
        idle_seconds
    };
    JsonRpcResponse::success(
        id,
        json!({
            "typing": typing,
            "idle_seconds": idle_seconds_capped,
        }),
    )
}

fn handle_send_wait_idle(
    engine: &mut CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let surface_id = match require_surface_id(params, &id) {
        Ok(sid) => sid,
        Err(e) => return e,
    };
    let text = match params.get("text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing 'text' parameter"),
    };
    if engine.is_typing(surface_id) {
        return JsonRpcResponse::success(id, json!({ "sent": false, "reason": "typing" }));
    }
    engine.ensure_surface_initialized(surface_id);
    if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
        terminal.send_key(&text);
        JsonRpcResponse::success(id, json!({ "sent": true }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {} not found", surface_id))
    }
}

#[cfg(test)]
mod structural_apply_error_tests {
    //! mirror 워크스페이스 구조 op forward 시 IPC 응답 정합성 회귀 방지.
    //! `forwarded:true`(원격으로 큐잉됨)를 실패로 오보하지 않고 success 로 회신한다.
    use super::structural_apply_error;

    #[test]
    fn forwarded_op_returns_success_not_error() {
        let err = anyhow::Error::new(crate::core::MirrorStructuralBlocked {
            workspace_index: 3,
            forwarded: true,
        });
        let resp = structural_apply_error(serde_json::json!(1), &err);
        assert!(
            resp.error.is_none(),
            "forward 로 큐잉된 op 는 에러로 회신하면 안 된다(원격 실행됨)"
        );
        let result = resp
            .result
            .expect("forwarded op 는 success result 를 가진다");
        assert_eq!(result["forwarded"], true);
        assert_eq!(result["workspace_index"], 3);
    }

    #[test]
    fn non_forwarded_mirror_block_stays_internal_error() {
        let err = anyhow::Error::new(crate::core::MirrorStructuralBlocked {
            workspace_index: 0,
            forwarded: false,
        });
        let resp = structural_apply_error(serde_json::json!(1), &err);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.expect("internal_error").code, -32603);
    }

    #[test]
    fn plain_error_stays_internal_error() {
        let err = anyhow::anyhow!("some unrelated failure");
        let resp = structural_apply_error(serde_json::json!(1), &err);
        assert!(resp.result.is_none());
        assert_eq!(resp.error.expect("internal_error").code, -32603);
    }
}

#[cfg(test)]
mod require_surface_id_tests {
    use super::require_surface_id;
    use crate::core::pty_registry::PTY_ID_BASE;
    use serde_json::json;

    #[test]
    fn accepts_surface_space_ids() {
        let id = json!(1);
        assert_eq!(
            require_surface_id(&json!({ "surface_id": 7 }), &id).unwrap(),
            7
        );
        assert_eq!(
            require_surface_id(&json!({ "surface_id": PTY_ID_BASE - 1 }), &id).unwrap(),
            PTY_ID_BASE - 1
        );
    }

    #[test]
    fn rejects_missing_wrong_type_and_out_of_u32_range() {
        let id = json!(1);
        assert!(require_surface_id(&json!({}), &id).is_err());
        assert!(require_surface_id(&json!({ "surface_id": "3" }), &id).is_err());
        assert!(require_surface_id(&json!({ "surface_id": -1 }), &id).is_err());
        assert!(
            require_surface_id(&json!({ "surface_id": u64::from(u32::MAX) + 1 }), &id).is_err()
        );
    }

    #[test]
    fn rejects_pty_id_space() {
        let id = json!(1);
        assert!(require_surface_id(&json!({ "surface_id": PTY_ID_BASE }), &id).is_err());
        // 실사용에서 관측된 오염 id.
        assert!(require_surface_id(&json!({ "surface_id": 2147484147u64 }), &id).is_err());
        assert!(require_surface_id(&json!({ "surface_id": u32::MAX }), &id).is_err());
    }
}

#[cfg(test)]
mod system_info_tests {
    use super::{handle_system_info, system_info_fields};

    #[test]
    fn system_info_identifies_the_engine_and_the_active_workspace_by_id() {
        let (state, engine) = crate::state::tests::test_state();
        let info = system_info_fields(&state, &engine);
        assert_eq!(info["scope"], "engine");
        assert_eq!(info["workspace_count"], engine.workspaces.len());
        assert_eq!(info["active_workspace"], 0);
        assert_eq!(info["active_workspace_id"], engine.workspaces[0].id);
        assert_eq!(
            info["workspace_ids"],
            serde_json::json!([engine.workspaces[0].id])
        );
    }

    #[test]
    fn system_info_does_not_invent_an_active_workspace_for_an_empty_engine() {
        let (state, mut engine) = crate::state::tests::test_state();
        engine.workspaces.clear();
        let info = system_info_fields(&state, &engine);
        assert_eq!(info["workspace_count"], 0);
        assert_eq!(info["active_workspace"], state.active_workspace);
        assert!(info["active_workspace_id"].is_null());
        assert_eq!(info["workspace_ids"], serde_json::json!([]));
    }

    /// 패키지 버전과 별도로 서버 capability를 제공한다.
    #[test]
    fn system_info_declares_what_this_server_can_negotiate() {
        let (state, engine) = crate::state::tests::test_state();
        let resp = handle_system_info(&state, &engine, serde_json::json!(1));
        let result = resp.result.expect("성공 응답이어야 한다");
        let caps = result["capabilities"]
            .as_array()
            .expect("capabilities 가 배열로 실려야 한다");
        assert!(!caps.is_empty());
        for c in caps {
            assert!(c["name"].is_string(), "{c}");
            assert!(c["version"].is_u64(), "{c}");
        }
        assert_eq!(result["scope"], "engine");
        assert!(result["version"].is_string());
    }

    /// client가 요구하는 capability가 응답에 실제로 포함되는지 확인한다.
    #[test]
    fn system_info_declares_the_capability_name_the_client_asks_for() {
        let (state, engine) = crate::state::tests::test_state();
        let resp = handle_system_info(&state, &engine, serde_json::json!(1));
        let result = resp.result.expect("성공 응답이어야 한다");
        let names: Vec<&str> = result["capabilities"]
            .as_array()
            .expect("배열")
            .iter()
            .filter_map(|c| c["name"].as_str())
            .collect();
        assert!(
            names.contains(&tasty_ipc::client::IDEMPOTENCY_CAPABILITY),
            "client 가 요구하는 이름이 선언에 없다: {names:?}"
        );
    }

    #[test]
    fn system_info_declares_the_bounds_of_the_idempotency_guarantee() {
        let (state, engine) = crate::state::tests::test_state();
        let resp = handle_system_info(&state, &engine, serde_json::json!(1));
        let result = resp.result.expect("성공 응답이어야 한다");
        assert_eq!(result["idempotency"], super::idempotency::declaration());
        assert_eq!(result["idempotency"]["survives_restart"], false);
    }

    /// capability를 창별 공통 필드에 중복하지 않는다.
    #[test]
    fn the_per_window_fields_do_not_repeat_the_server_capabilities() {
        let (state, engine) = crate::state::tests::test_state();
        let shared = system_info_fields(&state, &engine);
        assert!(
            shared.get("capabilities").is_none(),
            "창마다 재사용되는 필드에 capability 가 실렸다: {shared}"
        );
    }
}
