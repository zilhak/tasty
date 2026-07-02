// IPC handler 트리는 JSON-RPC public API surface. gui 빌드는 app::ipc::routing
// 경유로 호출하지만, headless 빌드는 run_headless 의 IPC dispatch 와이어링이
// 미구현이라 호출자 없음. 본질적 library API 이므로 *headless 한정* dead_code/
// unused_imports 침묵 — gui 빌드에선 검사 그대로.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

#[cfg(all(debug_assertions, feature = "gui"))]
mod debug;
#[cfg(debug_assertions)]
pub(crate) mod debug_plugin;
mod file_handler;
mod hooks;
#[cfg(feature = "gui")]
mod image;
#[cfg(all(target_os = "macos", feature = "gui"))]
mod input_source;
#[cfg(feature = "gui")]
mod markdown;
mod memory;
mod message;
mod meta;
mod notification;
mod output;
pub(crate) mod pane;
mod passkey;
mod preset;
mod remote_profile;
pub(crate) mod surface;
mod tab;
mod telemetry;
#[cfg(all(debug_assertions, feature = "gui"))]
mod tool;
#[cfg(feature = "gui")]
mod webview;
pub(crate) mod workspace;
pub(crate) mod workspace_category;

pub mod agent;
pub mod approval;
pub(crate) mod attach;
pub mod audit;
#[cfg(feature = "gui")]
pub mod ime;
pub mod plugin;
#[cfg(all(debug_assertions, feature = "gui"))]
pub mod popup;
pub mod session;

use std::borrow::Cow;

use serde_json::json;

use crate::core::CoreState;
use crate::ipc::alias;
use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::state::AppState;

/// caller가 명시된 라우터 진입점. CLI/네트워크 IPC는 [`CallerContext::Local`],
/// plugin process가 호출한 명령은 [`CallerContext::Plugin`]을 전달한다.
///
/// 라우터 구조:
/// 1. **engine 핸들러** (`route_engine_handler`): AppState UI 필드를 만지지 않는
///    핸들러 60+개. `&mut AppState`를 받지만 본문이 `state.engine`만 접근하거나
///    AppState 메서드(현재는 engine-only)만 호출한다. 단계 07에서 plugin 권한
///    게이트가 이 진입점에서 동작한다.
/// 2. **GUI 의존 핸들러** (`route_gui_handler`): UI state(popups/dialogs/active_workspace)
///    를 만져야 하는 소수 핸들러. 권한 게이트 대상 외부.
/// 3. **debug 핸들러** (`route_debug_handler`): debug build 전용. release에서는 정의 안 됨.
///
/// 권한 게이트는 라우터의 가장 바깥에서 한 번만 실행된다. plugin이 호출한
/// 명령이 권한을 통과하지 못하면 `permission_denied` 에러로 즉시 회신.
pub fn handle_with_caller(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    request: &JsonRpcRequest,
    caller: &CallerContext,
) -> JsonRpcResponse {
    let _ = core; // Phase D 진행 중 — 본 인자는 향후 도메인 핸들러 마이그레이션
    // 에서 점진 사용. 현재는 *시그니처 통과* 만.
    let id = request.id.clone().unwrap_or(serde_json::Value::Null);

    let canonical = alias::canonicalize(&request.method);
    if alias::is_deprecated(&request.method) {
        tracing::warn!(
            "ipc method '{}' is deprecated; use '{canonical}' (will be removed at 1.0)",
            request.method
        );
    }

    let workspace_id = engine.workspaces.get(state.active_workspace).map(|w| w.id);

    if let Err(e) = caller.ensure_allowed(canonical) {
        tracing::warn!("ipc permission denied: {e}");
        let seq = engine.telemetry_seq.next();
        crate::ipc::audit::record(
            core,
            caller,
            canonical,
            crate::ipc::audit::AuditDecision::Deny,
            Some(&format!("{e}")),
            workspace_id,
            seq,
        );
        return JsonRpcResponse::error(id, -32001, format!("permission_denied: {e}"));
    }

    // Phase 4.3c 텔레메트리 cap 차단: triggered + (Stop|Pause) 인 cap 이 있는
    // plugin agent 는 모든 IPC 가 거부된다. CLI/Local 은 검사 대상이 아니므로
    // `telemetry.cap.reset` 으로 해제 가능.
    if let Some(reason) = telemetry::check_cap_block(core, caller, canonical) {
        tracing::warn!("ipc cap blocked: {reason}");
        let seq = engine.telemetry_seq.next();
        crate::ipc::audit::record(
            core,
            caller,
            canonical,
            crate::ipc::audit::AuditDecision::Deny,
            Some(&format!("cap_blocked: {reason}")),
            workspace_id,
            seq,
        );
        return JsonRpcResponse::error(id, -32007, format!("cap_blocked: {reason}"));
    }

    // Phase I.A.2 rate_limit 미들웨어: 등록된 (agent, "ipc_calls") 한도 초과 시
    // -32010 throttled 응답 + audit Deny. 자가 회복을 위해 agent.rate_limit_*
    // 자체는 제외 (영구 차단 방지). throttled 호출은 `record_ipc_call` 을 건너
    // 뛰므로 `ipc_calls` telemetry 이벤트로 카운트되지 않는다 — throttle 추적은
    // `RateLimit.throttled_count` 가 담당.
    if should_rate_limit(caller, canonical) {
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
                    crate::ipc::audit::AuditDecision::Deny,
                    Some(&reason),
                    workspace_id,
                    seq,
                );
                return JsonRpcResponse::error(id, -32010, reason);
            }
            Ok(_) => {}
            Err(e) => {
                // fail-open: rate_limit 인프라 자체 실패는 전체 IPC 차단보다 통과 + warn.
                tracing::warn!("rate_limit middleware error: {e}");
            }
        }
    }

    // Phase 4.2 텔레메트리 미들웨어: 비-host caller 의 IPC 호출을 자동 카운트.
    // `telemetry.*` 자체와 `_host` agent 는 카운트 제외 (재귀 폭주 / 자기-측정 방지).
    // 카운트는 cap_eval 직후 호출되며 record 시 cap 평가도 함께 일어난다 (Phase 4.3b).
    telemetry::record_ipc_call(core, state, engine, caller, canonical);

    // Phase 6.5a audit: allow 경로도 기록. cap_blocked 와 마찬가지로 host 자신은
    // 기록 의미가 적지만 일관성을 위해 전부 기록 (운영자가 query 시 filter).
    let seq = engine.telemetry_seq.next();
    crate::ipc::audit::record(
        core,
        caller,
        canonical,
        crate::ipc::audit::AuditDecision::Allow,
        None,
        workspace_id,
        seq,
    );

    // 옛 이름이면 method를 새 이름으로 교체한 임시 request를 라우터에 전달.
    let routed: Cow<JsonRpcRequest> = if canonical == request.method {
        Cow::Borrowed(request)
    } else {
        Cow::Owned(JsonRpcRequest {
            jsonrpc: request.jsonrpc.clone(),
            method: canonical.to_string(),
            params: request.params.clone(),
            id: request.id.clone(),
            session_token: request.session_token.clone(),
        })
    };
    let request = routed.as_ref();

    if let Some(resp) = route_engine_handler(core, state, engine, caller, request, id.clone()) {
        return resp;
    }

    #[cfg(debug_assertions)]
    if let Some(resp) = route_debug_handler(state, engine, request, id.clone()) {
        return resp;
    }

    JsonRpcResponse::method_not_found(id, &request.method)
}

/// IPC rate_limit 미들웨어가 적용되는 caller/method 조합인가? (Phase I.A.2)
///
/// 제외 정책:
/// - **Local**: 사용자가 직접 CLI/network 로 호출 — 무제한.
/// - **Agent `_host`**: 호스트 자기 호출 (telemetry.rs:103 의 record_ipc_call
///   제외와 일관). throttle 자체 무의미.
/// - **`telemetry.*` / `agent.rate_limit_*` / `system.info`**:
///   - `telemetry.*` — record_ipc_call 자체가 호출하므로 재귀 폭주 위험.
///   - `agent.rate_limit_*` — throttle 걸린 agent 의 *자가 회복 경로*. 이게
///     막히면 한 번 throttle 된 agent 가 영구 차단됨.
///   - `system.info` — 단순 상태 조회. throttle 대상 아님.
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

/// engine-substate handlers — UI에 의존하지 않음. 단계 07 권한 게이트 대상.
///
/// 현재는 시그니처가 `&mut AppState`이지만 본문이 GUI를 만지지 않는다. 향후
/// AppState 메서드들이 `CoreState`로 이전되면 시그니처를 `&mut CoreState`로
/// 좁힐 예정 (별도 작업).
fn route_engine_handler(
    core: &mut crate::core::Core,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "system.info" => handle_system_info(state, engine, id),
        // workspace
        "workspace.list" => workspace::handle_workspace_list(state, engine, id),
        "workspace.create" => {
            workspace::handle_workspace_create(core, state, engine, id, &request.params)
        }
        "workspace.update" => {
            workspace::handle_workspace_update(core, state, engine, id, &request.params)
        }
        "workspace.move" => {
            workspace::handle_workspace_move(core, state, engine, id, &request.params)
        }
        // workspace category (사이드바 폴더 CRUD — 원칙 1·3: active/포커스 불변)
        "workspace_category.list" => workspace_category::handle_list(state, engine, id),
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
        // pane / split
        "pane.list" => pane::handle_pane_list(state, engine, id),
        "pane.close" => pane::handle_pane_close(core, state, engine, id, &request.params),
        "split" => pane::handle_split(core, state, engine, id, &request.params),
        // tab
        "tab.list" => tab::handle_tab_list(state, engine, id, &request.params),
        "tab.create" => tab::handle_tab_create(core, state, engine, id, &request.params),
        "tab.close" => tab::handle_tab_close(core, state, engine, id, &request.params),
        "tab.move" => tab::handle_tab_move(core, state, engine, id, &request.params),
        // preset (layout preset CRUD + apply)
        "preset.list" => preset::handle_list(core, state, id, &request.params),
        "preset.get" => preset::handle_get(core, state, id, &request.params),
        "preset.save" => preset::handle_save(core, state, id, &request.params),
        "preset.delete" => preset::handle_delete(core, state, id, &request.params),
        "preset.rename" => preset::handle_rename(core, state, id, &request.params),
        "preset.capture" => preset::handle_capture(core, state, engine, id, &request.params),
        "preset.apply" => preset::handle_apply(core, state, engine, id, &request.params),
        // surface
        "surface.close" => surface::handle_surface_close(core, state, engine, id, &request.params),
        "surface.close_self" => {
            surface::handle_surface_close_self(core, state, engine, id, &request.params)
        }
        "surface.list" => surface::handle_surface_list(state, engine, id),
        "surface.send" => surface::handle_surface_send(core, state, engine, id, &request.params),
        "surface.send_key" => {
            surface::handle_surface_send_key(core, state, engine, id, &request.params)
        }
        "surface.send_combo" => {
            surface::handle_surface_send_combo(core, state, engine, id, &request.params)
        }
        "surface.send_to" => {
            surface::handle_surface_send_to(core, state, engine, id, &request.params)
        }
        "surface.wake" => surface::handle_surface_wake(state, engine, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, engine, id, &request.params),
        "surface.read_since_mark" => {
            surface::handle_read_since_mark(state, engine, id, &request.params)
        }
        "surface.parse_since_mark" => {
            surface::handle_parse_since_mark(state, engine, id, &request.params)
        }
        "surface.commands" => surface::handle_commands(core, state, engine, id, &request.params),
        "surface.last_command" => {
            surface::handle_last_command(core, state, engine, id, &request.params)
        }
        "surface.command_at" => {
            surface::handle_command_at(core, state, engine, id, &request.params)
        }
        "output.observe_start" => {
            output::handle_observe_start(core, state, engine, id, &request.params)
        }
        "output.observe_stop" => {
            output::handle_observe_stop(core, state, engine, id, &request.params)
        }
        "output.observe_list" => output::handle_observe_list(core, state, engine, id),
        "output.observe_info" => {
            output::handle_observe_info(core, state, engine, id, &request.params)
        }
        "surface.screen_text" => surface::handle_screen_text(state, engine, id, &request.params),
        "surface.cursor_position" => {
            surface::handle_cursor_position(state, engine, id, &request.params)
        }
        "surface.foreground_process" => {
            surface::handle_foreground_process(state, engine, id, &request.params)
        }
        "surface.locate" => surface::handle_surface_locate(state, engine, id, &request.params),
        "surface.respawn_terminal" => {
            surface::handle_surface_respawn_terminal(core, state, engine, id, &request.params)
        }
        "surface.is_typing" => handle_is_typing(state, engine, id, &request.params),
        "surface.send_wait_idle" => handle_send_wait_idle(state, engine, id, &request.params),
        "surface.fire_hook" => {
            hooks::handle_surface_fire_hook(core, state, engine, id, &request.params)
        }
        "surface.meta.set" => meta::handle_surface_meta_set(state, engine, id, &request.params),
        "surface.meta.get" => meta::handle_surface_meta_get(state, engine, id, &request.params),
        "surface.meta.unset" => meta::handle_surface_meta_unset(state, engine, id, &request.params),
        "surface.meta.list" => meta::handle_surface_meta_list(state, engine, id, &request.params),
        "surface.set_cwd" => surface::handle_set_cwd(state, engine, id, &request.params),
        // hooks
        "hook.set" => hooks::handle_hook_set(core, state, engine, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, engine, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(core, state, engine, id, &request.params),
        "global_hook.set" => {
            hooks::handle_global_hook_set(core, state, engine, id, &request.params)
        }
        "global_hook.list" => hooks::handle_global_hook_list(state, engine, id),
        "global_hook.unset" => {
            hooks::handle_global_hook_unset(core, state, engine, id, &request.params)
        }
        // webview (plugin 이 webview-enabled surface 의 URL/navigation 제어)
        #[cfg(feature = "gui")]
        "webview.set_url" => webview::handle_set_url(state, engine, id, &request.params),
        // tree
        "tree" => handle_tree(state, engine, id),
        // message
        "message.send" => message::handle_message_send(core, state, engine, id, &request.params),
        "message.read" => message::handle_message_read(core, state, engine, id, &request.params),
        "message.count" => message::handle_message_count(state, engine, id, &request.params),
        "message.clear" => message::handle_message_clear(core, state, engine, id, &request.params),
        // input source (macOS)
        #[cfg(all(target_os = "macos", feature = "gui"))]
        "surface.switch_input_source" => {
            input_source::handle_switch_input_source(id, &request.params)
        }
        #[cfg(all(target_os = "macos", feature = "gui"))]
        "surface.raw_key" => input_source::handle_raw_key(id, &request.params),
        // notification (focus-independent — workspace_id/surface_id로 라우팅)
        "notification.list" => notification::handle_notification_list(state, engine, id),
        "notification.create" => {
            notification::handle_notification_create(state, engine, id, &request.params)
        }
        // file handler: 사용자 설정 reload (host 전용 — plugin 비노출).
        "file_handler.reload" => file_handler::handle_reload(core, engine, id),
        // file handler: 임의 경로를 dispatch 흐름에 진입시킴. plugin (예: explorer)
        // 또는 CLI 가 호출. plugin 호출은 FsRead 권한 요구.
        "file_handler.dispatch" => file_handler::handle_dispatch(state, id, request.params.clone()),
        // markdown 제자리 이동 (04) — 주소창(03) 플러그인이 자기 surface 를 새 파일로 교체.
        #[cfg(feature = "gui")]
        "markdown.navigate" => markdown::handle_navigate(state, id, request.params.clone()),
        // image surface 조작 — com.tasty.image plugin namespace 의 호스트 어댑터.
        // host 는 open(ConvertSurface)/list(surface 순회)만 담당하고, 픽셀 편집 계열
        // (save/export_png/paste/next/prev)은 plugin 이 자기 namespace 에서 처리한다.
        #[cfg(feature = "gui")]
        "image.open" => image::handle_open(core, state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "image.list" => image::handle_list(state, engine, id),
        // memory: regular (공유 네임스페이스 + owner enforcement)
        "memory.put" => memory::handle_put(core, state, engine, caller, id, &request.params),
        "memory.get" => memory::handle_get(core, state, engine, caller, id, &request.params),
        "memory.delete" => memory::handle_delete(core, state, engine, caller, id, &request.params),
        "memory.list" => memory::handle_list(core, state, engine, caller, id, &request.params),
        "memory.exists" => memory::handle_exists(core, state, engine, caller, id, &request.params),
        "memory.count" => memory::handle_count(core, state, engine, caller, id, &request.params),
        "memory.scopes" => memory::handle_scopes(core, state, engine, caller, id, &request.params),
        "memory.stats" => memory::handle_stats(core, state, engine, caller, id, &request.params),
        "memory.query" => memory::handle_query(core, state, engine, caller, id, &request.params),
        "memory.export" => memory::handle_export(core, state, engine, caller, id, &request.params),
        "memory.import" => memory::handle_import(core, state, engine, caller, id, &request.params),
        // memory: secret (plugin 별 사전 분할)
        "memory.secret.put" => {
            memory::handle_secret_put(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.get" => {
            memory::handle_secret_get(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.delete" => {
            memory::handle_secret_delete(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.list" => {
            memory::handle_secret_list(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.exists" => {
            memory::handle_secret_exists(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.count" => {
            memory::handle_secret_count(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.scopes" => {
            memory::handle_secret_scopes(core, state, engine, caller, id, &request.params)
        }
        "memory.secret.stats" => {
            memory::handle_secret_stats(core, state, engine, caller, id, &request.params)
        }
        // memory: 유지 보수 (host 전용)
        "memory.gc" => memory::handle_gc(core, state, engine, caller, id, &request.params),
        // memory: blackboard (Phase 7.1 — workspace-scoped 키-값 컬렉션)
        "memory.bb_create" => {
            memory::handle_bb_create(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_put" => memory::handle_bb_put(core, state, engine, caller, id, &request.params),
        "memory.bb_get" => memory::handle_bb_get(core, state, engine, caller, id, &request.params),
        "memory.bb_get_all" => {
            memory::handle_bb_get_all(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_get_meta" => {
            memory::handle_bb_get_meta(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_delete_field" => {
            memory::handle_bb_delete_field(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_delete" => {
            memory::handle_bb_delete(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_list" => {
            memory::handle_bb_list(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_exists" => {
            memory::handle_bb_exists(core, state, engine, caller, id, &request.params)
        }
        // memory: bb snapshot (Phase 7.4)
        "memory.bb_snapshot" => {
            memory::handle_bb_snapshot(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_get" => {
            memory::handle_bb_snapshot_get(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_list" => {
            memory::handle_bb_snapshot_list(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_delete" => {
            memory::handle_bb_snapshot_delete(core, state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_restore" => {
            memory::handle_bb_snapshot_restore(core, state, engine, caller, id, &request.params)
        }
        // memory: plan (Phase 7.2 — workspace-scoped 선언적 work breakdown)
        "memory.plan_create" => {
            memory::handle_plan_create(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_get" => {
            memory::handle_plan_get(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_list" => {
            memory::handle_plan_list(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_delete" => {
            memory::handle_plan_delete(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_add_step" => {
            memory::handle_plan_add_step(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_remove_step" => {
            memory::handle_plan_remove_step(core, state, engine, caller, id, &request.params)
        }
        "memory.plan_update_step" => {
            memory::handle_plan_update_step(core, state, engine, caller, id, &request.params)
        }
        // memory: cache (Phase 7.3 — workspace-scoped TTL 캐시)
        "memory.cache_put" => {
            memory::handle_cache_put(core, state, engine, caller, id, &request.params)
        }
        "memory.cache_get" => {
            memory::handle_cache_get(core, state, engine, caller, id, &request.params)
        }
        "memory.cache_invalidate" => {
            memory::handle_cache_invalidate(core, state, engine, caller, id, &request.params)
        }
        "memory.cache_clear" => {
            memory::handle_cache_clear(core, state, engine, caller, id, &request.params)
        }
        "memory.cache_list" => {
            memory::handle_cache_list(core, state, engine, caller, id, &request.params)
        }
        // approval (휴먼 핸드오프) — await 는 process_ipc 에서 worker thread 로 분리 처리.
        "approval.request" => {
            approval::handle_request(core, state, engine, caller, id, &request.params)
        }
        "approval.respond" => {
            approval::handle_respond(core, state, engine, caller, id, &request.params)
        }
        "approval.cancel" => {
            approval::handle_cancel(core, state, engine, caller, id, &request.params)
        }
        "approval.get" => approval::handle_get(core, state, engine, caller, id, &request.params),
        "approval.list" => approval::handle_list(core, state, engine, caller, id, &request.params),
        "approval.history" => {
            approval::handle_history(core, state, engine, caller, id, &request.params)
        }
        "approval.summary.set" => {
            approval::handle_summary_set(core, state, engine, caller, id, &request.params)
        }
        "approval.summary.get" => {
            approval::handle_summary_get(core, state, engine, caller, id, &request.params)
        }
        // telemetry (관측 / 비용) — 단계 4.1
        "telemetry.record" => {
            telemetry::handle_record(core, state, engine, caller, id, &request.params)
        }
        "telemetry.record_batch" => {
            telemetry::handle_record_batch(core, state, engine, caller, id, &request.params)
        }
        "telemetry.summary" => {
            telemetry::handle_summary(core, state, engine, caller, id, &request.params)
        }
        "telemetry.timeseries" => {
            telemetry::handle_timeseries(core, state, engine, caller, id, &request.params)
        }
        "telemetry.top" => telemetry::handle_top(core, state, engine, caller, id, &request.params),
        // telemetry.cap — Phase 4.3 (CRUD; eval/action wiring 은 후속)
        "telemetry.cap.set" => {
            telemetry::handle_cap_set(core, state, engine, caller, id, &request.params)
        }
        "telemetry.cap.list" => {
            telemetry::handle_cap_list(core, state, engine, caller, id, &request.params)
        }
        "telemetry.cap.remove" => {
            telemetry::handle_cap_remove(core, state, engine, caller, id, &request.params)
        }
        "telemetry.cap.status" => {
            telemetry::handle_cap_status(core, state, engine, caller, id, &request.params)
        }
        "telemetry.cap.reset" => {
            telemetry::handle_cap_reset(core, state, engine, caller, id, &request.params)
        }
        // telemetry.anomaly — Phase 4.4 (영속 anomaly 조회만; 검출은 dispatcher 후크)
        "telemetry.anomaly.list" => {
            telemetry::handle_anomaly_list(core, state, engine, caller, id, &request.params)
        }
        // telemetry.session_summary — Phase 4.5 (메트릭/승인/이상 집계)
        "telemetry.session_summary" => {
            telemetry::handle_session_summary(core, state, engine, caller, id, &request.params)
        }
        // agent.task_* — Phase 5.1 (DAG + state 머신)
        "agent.task_create" => {
            agent::handle_task_create(core, state, engine, caller, id, &request.params)
        }
        "agent.task_list" => {
            agent::handle_task_list(core, state, engine, caller, id, &request.params)
        }
        "agent.task_get" => {
            agent::handle_task_get(core, state, engine, caller, id, &request.params)
        }
        "agent.task_await" => {
            agent::handle_task_await(core, state, engine, caller, id, &request.params)
        }
        "agent.task_cancel" => {
            agent::handle_task_cancel(core, state, engine, caller, id, &request.params)
        }
        "agent.task_retry" => {
            agent::handle_task_retry(core, state, engine, caller, id, &request.params)
        }
        "agent.task_graph" => {
            agent::handle_task_graph(core, state, engine, caller, id, &request.params)
        }
        // agent.task_set_result — Phase H.F (외부 task 완료 신호)
        "agent.task_set_result" => {
            agent::handle_task_set_result(core, state, engine, caller, id, &request.params)
        }
        // agent.task_run — Phase H.F (workspace runner thread 시작/중단/상태)
        "agent.task_run" => {
            agent::handle_task_run(core, state, engine, caller, id, &request.params)
        }
        // agent.barrier_* / semaphore_* — Phase 5.2 (poll-based 동기화 primitive)
        "agent.barrier_create" => {
            agent::handle_barrier_create(core, state, engine, caller, id, &request.params)
        }
        "agent.barrier_signal" => {
            agent::handle_barrier_signal(core, state, engine, caller, id, &request.params)
        }
        "agent.barrier_await" => {
            agent::handle_barrier_await(core, state, engine, caller, id, &request.params)
        }
        "agent.barrier_state" => {
            agent::handle_barrier_state(core, state, engine, caller, id, &request.params)
        }
        "agent.semaphore_create" => {
            agent::handle_semaphore_create(core, state, engine, caller, id, &request.params)
        }
        "agent.semaphore_acquire" => {
            agent::handle_semaphore_acquire(core, state, engine, caller, id, &request.params)
        }
        "agent.semaphore_release" => {
            agent::handle_semaphore_release(core, state, engine, caller, id, &request.params)
        }
        "agent.barrier_list" => {
            agent::handle_barrier_list(core, state, engine, caller, id, &request.params)
        }
        "agent.barrier_delete" => {
            agent::handle_barrier_delete(core, state, engine, caller, id, &request.params)
        }
        "agent.semaphore_list" => {
            agent::handle_semaphore_list(core, state, engine, caller, id, &request.params)
        }
        "agent.semaphore_delete" => {
            agent::handle_semaphore_delete(core, state, engine, caller, id, &request.params)
        }
        // agent.lease_* — Phase 5.3 (협조적 점유 마커 + TTL)
        "agent.lease_acquire" => {
            agent::handle_lease_acquire(core, state, engine, caller, id, &request.params)
        }
        "agent.lease_release" => {
            agent::handle_lease_release(core, state, engine, caller, id, &request.params)
        }
        "agent.lease_list" => {
            agent::handle_lease_list(core, state, engine, caller, id, &request.params)
        }
        // agent.task_reduce — Phase 5.4 (결과 합성: first_success / all / merge_json / concat_text / custom)
        "agent.task_reduce" => {
            agent::handle_task_reduce(core, state, engine, caller, id, &request.params)
        }
        // agent.rate_limit_* — Phase 5.5 (token bucket 시간당 비율 제한)
        "agent.rate_limit_set" => {
            agent::handle_rate_limit_set(core, state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_list" => {
            agent::handle_rate_limit_list(core, state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_remove" => {
            agent::handle_rate_limit_remove(core, state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_status" => {
            agent::handle_rate_limit_status(core, state, engine, caller, id, &request.params)
        }
        // session.* — Phase 6.2c (자식 agent 신원 토큰 관리)
        "session.issue" => session::handle_issue(core, caller, id, &request.params),
        "session.revoke" => session::handle_revoke(core, id, &request.params),
        "session.list" => session::handle_list(core, id),
        // attach.* — attach/detach 단계 3 (배타 점유 제어; session.* 와 별개)
        "attach.acquire" => attach::handle_acquire(engine, id, &request.params),
        "attach.release" => attach::handle_release(engine, id, &request.params),
        "attach.force_detach" => attach::handle_force_detach(engine, id, &request.params),
        "attach.force_detach_workspace" => {
            attach::handle_force_detach_workspace(engine, id, &request.params)
        }
        "attach.into_gui" => attach::handle_into_gui(engine, id, &request.params),
        "attach.list" => attach::handle_list(engine, id),
        // remote.profile.* — 원격 접속 프로필 CRUD (원칙 2). 로컬 파일 I/O.
        // (구 tool.ssh.* / ssh.profile.* 는 alias.rs 에서 정규화되어 여기로 도달.)
        "remote.profile.list" => remote_profile::handle_list(id),
        "remote.profile.get" => remote_profile::handle_get(id, &request.params),
        "remote.profile.add" => remote_profile::handle_add(id, &request.params),
        "remote.profile.detect" => remote_profile::handle_detect(id, &request.params),
        "remote.profile.remove" => remote_profile::handle_remove(id, &request.params),
        // remote.passkey.* — 자격증명 CRUD (값 마스킹 — 경로/내용 미반환).
        "remote.passkey.list" => passkey::handle_list(id),
        "remote.passkey.get" => passkey::handle_get(id, &request.params),
        "remote.passkey.add" => passkey::handle_add(id, &request.params),
        "remote.passkey.remove" => passkey::handle_remove(id, &request.params),
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
        "ui.state" => handle_ui_state(state, engine, id),
        // settings cascade 는 headless 에서도 유효 — gui 게이트 없이 둔다 (ui.state 선례).
        // 핸들러는 handler.rs 에 직접 둔다 (debug 서브모듈은 gui 게이트라 headless 에서 사라짐).
        "debug.settings.apply" => handle_debug_settings_apply(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.cell_info" => debug::handle_debug_cell_info(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.screen_attrs" => {
            debug::handle_debug_screen_attrs(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.glyph_color" => debug::handle_debug_glyph_color(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.feed_bytes" => debug::handle_debug_feed_bytes(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.inject_mouse" => {
            debug::handle_debug_inject_mouse(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.inject_key" => debug::handle_debug_inject_key(state, engine, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.switch_workspace" => {
            debug::handle_debug_switch_workspace(state, engine, id, &request.params)
        }
        #[cfg(feature = "gui")]
        "debug.switch_tab" => debug::handle_debug_switch_tab(state, engine, id, &request.params),
        // 도구 메뉴 — 사용자 클릭 자동화. release 미노출.
        #[cfg(feature = "gui")]
        "debug.tool.list" => tool::handle_list(state, engine, id),
        #[cfg(feature = "gui")]
        "debug.tool.invoke" => tool::handle_invoke(state, engine, id, &request.params),
        // 호스트 빌트인 popup 직접 open/close — 사용자 클릭 경로 없이 시각 검증용.
        // release 미노출. (plugin popup 은 debug.popup.* 가 담당.)
        #[cfg(feature = "gui")]
        "debug.host_popup.list" => debug::handle_debug_host_popup_list(state, id),
        #[cfg(feature = "gui")]
        "debug.host_popup.open" => debug::handle_debug_host_popup_open(state, id, &request.params),
        #[cfg(feature = "gui")]
        "debug.host_popup.close" => {
            debug::handle_debug_host_popup_close(state, id, &request.params)
        }
        // 배너 직접 발화/조회/닫기 — 사용자 조작 없이 시각 검증용. release 미노출.
        // 배너는 사용자 행동에서만 발사되므로(발화 정책 §불가침) 이 표면은 debug 전용.
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

/// Extract a required surface_id from params. Returns Err(JsonRpcResponse) if missing.
pub(super) fn require_surface_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'surface_id' parameter")
        })
}

/// Extract a required pane_id from params. Returns Err(JsonRpcResponse) if missing.
fn require_pane_id(
    params: &serde_json::Value,
    id: &serde_json::Value,
) -> Result<u32, JsonRpcResponse> {
    params
        .get("pane_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| {
            JsonRpcResponse::invalid_params(id.clone(), "Missing required 'pane_id' parameter")
        })
}

/// Extract optional caller_surface_id from params.
pub(super) fn caller_surface_id(params: &serde_json::Value) -> Option<u32> {
    params
        .get("caller_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
}

/// Check if a surface belongs to a pane (directly or in any tab).
fn surface_belongs_to_pane(engine: &CoreState, surface_id: u32, pane_id: u32) -> bool {
    engine.find_pane_for_surface(surface_id) == Some(pane_id)
}

/// Apply metadata key-value pairs to a surface.
pub(crate) fn apply_meta(
    state: &AppState,
    surface_id: u32,
    meta: Option<&serde_json::Map<String, serde_json::Value>>,
) {
    if let Some(map) = meta {
        for (key, value) in map {
            if let Some(v) = value.as_str() {
                let result = state.with_memory(|m| {
                    crate::surface_meta::SurfaceMetaStore::set(m, surface_id, key, v)
                });
                if let Err(e) = result {
                    tracing::warn!(
                        "surface_meta set failed for surface {surface_id} key '{key}': {e}"
                    );
                }
            }
        }
    }
}

fn handle_system_info(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "workspace_count": engine.workspaces.len(),
            "active_workspace": state.active_workspace,
        }),
    )
}

#[cfg(debug_assertions)]
fn handle_ui_state(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let ws = state.active_workspace(engine);
    let pane_count = ws.pane_layout().all_pane_ids().len();
    let focused_pane_id = ws.focused_pane;
    let tab_count = ws
        .pane_layout()
        .find_pane(focused_pane_id)
        .map(|p| p.tabs.len())
        .unwrap_or(0);
    #[cfg(feature = "gui")]
    let notification_panel_open = state.popups.is_open("notifications");
    #[cfg(not(feature = "gui"))]
    let notification_panel_open = false;
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": notification_panel_open,
            "active_workspace": state.active_workspace,
            "workspace_count": engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

/// `debug.settings.apply` — `{ settings }` 의 부분 JSON patch 를 라이브 settings
/// 직렬화 **복사본** 위에 재귀 deep-merge 한 뒤, 완성된 전체 `Settings` 로
/// `UpdateSettings` intent 를 dispatch 한다. 이후는 기존 파이프라인
/// (dispatch_pending_intents → Core::apply → SettingsUpdated → cascade)이
/// collapse / theme / config.toml save 까지 처리한다 — 모달 / proxy 불요.
///
/// 사용자의 "설정 모달에서 값 변경 후 저장" 을 재현하는 디버그 동작이므로
/// release 에 노출되지 않는다 (`#[cfg(debug_assertions)]`). gui feature 와
/// 무관하게 동작하므로 (settings cascade 는 headless 에서도 유효) gui 게이트
/// 없이 두었고, 그래서 핸들러를 gui 게이트된 `debug` 서브모듈이 아니라 여기
/// (`handle_ui_state` 와 같은 비-gui 선례 위치)에 둔다.
///
/// 주의:
/// - 라이브 `engine.settings` 를 dispatch 전에 직접 mutate 하지 않는다. cascade 가
///   prev(라이브)와 new 를 비교해 collapse 분기를 결정하므로, pre-mutate 시
///   prev==new 가 되어 collapse 가 죽는다. merge 는 직렬화 복사본 위에서만 한다.
/// - `Settings` 는 `deny_unknown_fields` 가 아니므로(`#[serde(default)]`) patch 의
///   오타/미지정 키는 조용히 무시된다(no-op). 타입 불일치는 `from_value` Err →
///   `invalid_params` 로 거부되고 라이브는 불변.
#[cfg(debug_assertions)]
fn handle_debug_settings_apply(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> JsonRpcResponse {
    let Some(patch) = params.get("settings") else {
        return JsonRpcResponse::invalid_params(id, "Missing required 'settings' parameter");
    };
    if !patch.is_object() {
        return JsonRpcResponse::invalid_params(id, "'settings' must be a JSON object");
    }

    // base = 라이브 settings 직렬화 복사본 (라이브 자체는 건드리지 않는다).
    let mut base = match serde_json::to_value(&engine.settings) {
        Ok(v) => v,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                -32603,
                format!("failed to serialize live settings: {e}"),
            );
        }
    };
    json_deep_merge(&mut base, patch);

    // base 가 이미 완전한 Settings 직렬화이므로 serde(default) 함정을 피한다.
    let new_settings: tasty_settings::Settings = match serde_json::from_value(base) {
        Ok(s) => s,
        Err(e) => {
            return JsonRpcResponse::invalid_params(id, format!("invalid settings patch: {e}"));
        }
    };

    state.dispatch_intent(
        crate::core::intent::DomainIntent::UpdateSettings(new_settings).from_agent_ipc(),
    );
    JsonRpcResponse::success(id, json!({ "applied": true }))
}

/// 표준 재귀 deep-merge. 양쪽이 object 면 키별로 재귀 병합하고, 그 외에는
/// `patch` 값으로 `target` 을 치환한다. 얕은 치환이 아니므로 nested 필드가
/// 유실되지 않는다 (예: `appearance` 의 일부 키만 patch 해도 나머지 보존).
#[cfg(debug_assertions)]
fn json_deep_merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(target_map), serde_json::Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                json_deep_merge(
                    target_map
                        .entry(k.clone())
                        .or_insert(serde_json::Value::Null),
                    v,
                );
            }
        }
        (target_slot, patch_val) => {
            *target_slot = patch_val.clone();
        }
    }
}

fn handle_tree(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    JsonRpcResponse::success(id, json!(build_engine_tree(state, engine)))
}

/// 한 (state, engine) 쌍의 워크스페이스 트리를 JSON 배열로 빌드한다.
///
/// IPC `list tree`(단일 라우팅 engine) 와 Lua 스냅샷(전 View/parked 통합, ADR-0031)이
/// **같은 구조**를 내도록 공유하는 빌더 — 노드 필드(active/busy_count/busy, panes/tabs/surface)가
/// 드리프트하지 않게 단일 소스로 유지한다.
pub(crate) fn build_engine_tree(
    state: &AppState,
    engine: &crate::core::CoreState,
) -> Vec<serde_json::Value> {
    engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let mut t = ws.to_tree_json();
            t["active"] = json!(i == state.active_workspace);
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

        // Recurse into children.
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

        // After children are annotated, sum descendant busy counts.
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
    _state: &AppState,
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
    _state: &mut AppState,
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
