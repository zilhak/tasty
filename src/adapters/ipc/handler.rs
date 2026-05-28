mod clipboard;
#[cfg(debug_assertions)]
mod debug;
#[cfg(debug_assertions)]
pub(crate) mod debug_plugin;
mod file_handler;
mod hooks;
mod image;
#[cfg(target_os = "macos")]
mod input_source;
mod memory;
mod message;
mod meta;
mod notification;
mod output;
pub(crate) mod pane;
mod preset;
pub(crate) mod surface;
mod tab;
mod telemetry;
#[cfg(debug_assertions)]
mod tool;
mod webview;
pub(crate) mod workspace;

pub mod agent;
pub mod approval;
pub mod audit;
pub mod ime;
pub mod plugin;
#[cfg(debug_assertions)]
pub mod popup;
pub mod session;

use std::borrow::Cow;

use serde_json::json;

use crate::engine_state::CoreState;
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
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    request: &JsonRpcRequest,
    caller: &CallerContext,
) -> JsonRpcResponse {
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
            caller,
            canonical,
            crate::ipc::audit::AuditDecision::Deny,
            Some(&format!("{e}")),
            workspace_id,
            seq,
        );
        return JsonRpcResponse::error(id, -32001, &format!("permission_denied: {e}"));
    }

    // Phase 4.3c 텔레메트리 cap 차단: triggered + (Stop|Pause) 인 cap 이 있는
    // plugin agent 는 모든 IPC 가 거부된다. CLI/Local 은 검사 대상이 아니므로
    // `telemetry.cap.reset` 으로 해제 가능.
    if let Some(reason) = telemetry::check_cap_block(caller, canonical) {
        tracing::warn!("ipc cap blocked: {reason}");
        let seq = engine.telemetry_seq.next();
        crate::ipc::audit::record(
            caller,
            canonical,
            crate::ipc::audit::AuditDecision::Deny,
            Some(&format!("cap_blocked: {reason}")),
            workspace_id,
            seq,
        );
        return JsonRpcResponse::error(id, -32007, &format!("cap_blocked: {reason}"));
    }

    // Phase 4.2 텔레메트리 미들웨어: 비-host caller 의 IPC 호출을 자동 카운트.
    // `telemetry.*` 자체와 `_host` agent 는 카운트 제외 (재귀 폭주 / 자기-측정 방지).
    // 카운트는 cap_eval 직후 호출되며 record 시 cap 평가도 함께 일어난다 (Phase 4.3b).
    telemetry::record_ipc_call(state, engine, caller, canonical);

    // Phase 6.5a audit: allow 경로도 기록. cap_blocked 와 마찬가지로 host 자신은
    // 기록 의미가 적지만 일관성을 위해 전부 기록 (운영자가 query 시 filter).
    let seq = engine.telemetry_seq.next();
    crate::ipc::audit::record(
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

    if let Some(resp) = route_engine_handler(state, engine, caller, request, id.clone()) {
        return resp;
    }

    #[cfg(debug_assertions)]
    if let Some(resp) = route_debug_handler(state, engine, request, id.clone()) {
        return resp;
    }

    JsonRpcResponse::method_not_found(id, &request.method)
}

/// engine-substate handlers — UI에 의존하지 않음. 단계 07 권한 게이트 대상.
///
/// 현재는 시그니처가 `&mut AppState`이지만 본문이 GUI를 만지지 않는다. 향후
/// AppState 메서드들이 `CoreState`로 이전되면 시그니처를 `&mut CoreState`로
/// 좁힐 예정 (별도 작업).
fn route_engine_handler(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    caller: &CallerContext,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "system.info" => handle_system_info(state, engine, id),
        // workspace
        "workspace.list" => workspace::handle_workspace_list(state, engine, id),
        "workspace.create" => {
            workspace::handle_workspace_create(state, engine, id, &request.params)
        }
        "workspace.update" => {
            workspace::handle_workspace_update(state, engine, id, &request.params)
        }
        "workspace.move" => workspace::handle_workspace_move(state, engine, id, &request.params),
        // pane / split
        "pane.list" => pane::handle_pane_list(state, engine, id),
        "pane.close" => pane::handle_pane_close(state, engine, id, &request.params),
        "split" => pane::handle_split(state, engine, id, &request.params),
        // tab
        "tab.list" => tab::handle_tab_list(state, engine, id, &request.params),
        "tab.create" => tab::handle_tab_create(state, engine, id, &request.params),
        "tab.close" => tab::handle_tab_close(state, engine, id, &request.params),
        "tab.move" => tab::handle_tab_move(state, engine, id, &request.params),
        // preset (layout preset CRUD + apply)
        "preset.list" => preset::handle_list(state, engine, id, &request.params),
        "preset.get" => preset::handle_get(state, engine, id, &request.params),
        "preset.save" => preset::handle_save(state, engine, id, &request.params),
        "preset.delete" => preset::handle_delete(state, engine, id, &request.params),
        "preset.rename" => preset::handle_rename(state, engine, id, &request.params),
        "preset.capture" => preset::handle_capture(state, engine, id, &request.params),
        "preset.apply" => preset::handle_apply(state, engine, id, &request.params),
        // surface
        "surface.close" => surface::handle_surface_close(state, engine, id, &request.params),
        "surface.close_self" => {
            surface::handle_surface_close_self(state, engine, id, &request.params)
        }
        "surface.list" => surface::handle_surface_list(state, engine, id),
        "surface.send" => surface::handle_surface_send(state, engine, id, &request.params),
        "surface.send_key" => surface::handle_surface_send_key(state, engine, id, &request.params),
        "surface.send_combo" => {
            surface::handle_surface_send_combo(state, engine, id, &request.params)
        }
        "surface.send_to" => surface::handle_surface_send_to(state, engine, id, &request.params),
        "surface.wake" => surface::handle_surface_wake(state, engine, id, &request.params),
        "surface.set_mark" => surface::handle_set_mark(state, engine, id, &request.params),
        "surface.read_since_mark" => {
            surface::handle_read_since_mark(state, engine, id, &request.params)
        }
        "surface.parse_since_mark" => {
            surface::handle_parse_since_mark(state, engine, id, &request.params)
        }
        "surface.commands" => surface::handle_commands(state, engine, id, &request.params),
        "surface.last_command" => surface::handle_last_command(state, engine, id, &request.params),
        "surface.command_at" => surface::handle_command_at(state, engine, id, &request.params),
        "output.observe_start" => output::handle_observe_start(state, engine, id, &request.params),
        "output.observe_stop" => output::handle_observe_stop(state, engine, id, &request.params),
        "output.observe_list" => output::handle_observe_list(state, engine, id),
        "output.observe_info" => output::handle_observe_info(state, engine, id, &request.params),
        "surface.screen_text" => surface::handle_screen_text(state, engine, id, &request.params),
        "surface.cursor_position" => {
            surface::handle_cursor_position(state, engine, id, &request.params)
        }
        "surface.foreground_process" => {
            surface::handle_foreground_process(state, engine, id, &request.params)
        }
        "surface.locate" => surface::handle_surface_locate(state, engine, id, &request.params),
        "surface.respawn_terminal" => {
            surface::handle_surface_respawn_terminal(state, engine, id, &request.params)
        }
        "surface.is_typing" => handle_is_typing(state, engine, id, &request.params),
        "surface.send_wait_idle" => handle_send_wait_idle(state, engine, id, &request.params),
        "surface.fire_hook" => hooks::handle_surface_fire_hook(state, engine, id, &request.params),
        "surface.meta.set" => meta::handle_surface_meta_set(state, engine, id, &request.params),
        "surface.meta.get" => meta::handle_surface_meta_get(state, engine, id, &request.params),
        "surface.meta.unset" => meta::handle_surface_meta_unset(state, engine, id, &request.params),
        "surface.meta.list" => meta::handle_surface_meta_list(state, engine, id, &request.params),
        // hooks
        "hook.set" => hooks::handle_hook_set(state, engine, id, &request.params),
        "hook.list" => hooks::handle_hook_list(state, engine, id, &request.params),
        "hook.unset" => hooks::handle_hook_unset(state, engine, id, &request.params),
        "global_hook.set" => hooks::handle_global_hook_set(state, engine, id, &request.params),
        "global_hook.list" => hooks::handle_global_hook_list(state, engine, id),
        "global_hook.unset" => hooks::handle_global_hook_unset(state, engine, id, &request.params),
        // webview (plugin 이 webview-enabled surface 의 URL/navigation 제어)
        "webview.set_url" => webview::handle_set_url(state, engine, id, &request.params),
        // tree
        "tree" => handle_tree(state, engine, id),
        // message
        "message.send" => message::handle_message_send(state, engine, id, &request.params),
        "message.read" => message::handle_message_read(state, engine, id, &request.params),
        "message.count" => message::handle_message_count(state, engine, id, &request.params),
        "message.clear" => message::handle_message_clear(state, engine, id, &request.params),
        // tool.clipboard (read/write only — viewer_open is GUI)
        "tool.clipboard.list" => clipboard::handle_list(engine, id, &request.params),
        "tool.clipboard.get" => clipboard::handle_get(engine, id, &request.params),
        "tool.clipboard.paste" => clipboard::handle_paste(engine, id, &request.params),
        "tool.clipboard.remove" => clipboard::handle_remove(engine, id, &request.params),
        "tool.clipboard.clear" => clipboard::handle_clear(engine, id),
        // input source (macOS)
        #[cfg(target_os = "macos")]
        "surface.switch_input_source" => {
            input_source::handle_switch_input_source(id, &request.params)
        }
        #[cfg(target_os = "macos")]
        "surface.raw_key" => input_source::handle_raw_key(id, &request.params),
        // notification (focus-independent — workspace_id/surface_id로 라우팅)
        "notification.list" => notification::handle_notification_list(state, engine, id),
        "notification.create" => {
            notification::handle_notification_create(state, engine, id, &request.params)
        }
        // file handler: 사용자 설정 reload (host 전용 — plugin 비노출).
        "file_handler.reload" => file_handler::handle_reload(state, engine, id),
        // file handler: 임의 경로를 dispatch 흐름에 진입시킴. plugin (예: explorer)
        // 또는 CLI 가 호출. plugin 호출은 FsRead 권한 요구.
        "file_handler.dispatch" => {
            file_handler::handle_dispatch(state, engine, id, request.params.clone())
        }
        // image surface 조작 — com.tasty.image plugin이 외부에 노출하는 namespace의
        // 호스트 어댑터. plugin 비활성 상태에서도 CLI/직접 IPC로 호출 가능.
        "image.open" => image::handle_open(state, engine, id, &request.params),
        "image.save" => image::handle_save(state, engine, id, &request.params),
        "image.export_png" => image::handle_export_png(state, engine, id, &request.params),
        "image.next" => image::handle_next(state, engine, id, &request.params),
        "image.prev" => image::handle_prev(state, engine, id, &request.params),
        "image.paste" => image::handle_paste(state, engine, id, &request.params),
        "image.list" => image::handle_list(state, engine, id),
        // memory: regular (공유 네임스페이스 + owner enforcement)
        "memory.put" => memory::handle_put(state, engine, caller, id, &request.params),
        "memory.get" => memory::handle_get(state, engine, caller, id, &request.params),
        "memory.delete" => memory::handle_delete(state, engine, caller, id, &request.params),
        "memory.list" => memory::handle_list(state, engine, caller, id, &request.params),
        "memory.exists" => memory::handle_exists(state, engine, caller, id, &request.params),
        "memory.count" => memory::handle_count(state, engine, caller, id, &request.params),
        "memory.scopes" => memory::handle_scopes(state, engine, caller, id, &request.params),
        "memory.stats" => memory::handle_stats(state, engine, caller, id, &request.params),
        "memory.query" => memory::handle_query(state, engine, caller, id, &request.params),
        "memory.export" => memory::handle_export(state, engine, caller, id, &request.params),
        "memory.import" => memory::handle_import(state, engine, caller, id, &request.params),
        // memory: secret (plugin 별 사전 분할)
        "memory.secret.put" => {
            memory::handle_secret_put(state, engine, caller, id, &request.params)
        }
        "memory.secret.get" => {
            memory::handle_secret_get(state, engine, caller, id, &request.params)
        }
        "memory.secret.delete" => {
            memory::handle_secret_delete(state, engine, caller, id, &request.params)
        }
        "memory.secret.list" => {
            memory::handle_secret_list(state, engine, caller, id, &request.params)
        }
        "memory.secret.exists" => {
            memory::handle_secret_exists(state, engine, caller, id, &request.params)
        }
        "memory.secret.count" => {
            memory::handle_secret_count(state, engine, caller, id, &request.params)
        }
        "memory.secret.scopes" => {
            memory::handle_secret_scopes(state, engine, caller, id, &request.params)
        }
        "memory.secret.stats" => {
            memory::handle_secret_stats(state, engine, caller, id, &request.params)
        }
        // memory: 유지 보수 (host 전용)
        "memory.gc" => memory::handle_gc(state, engine, caller, id, &request.params),
        // memory: blackboard (Phase 7.1 — workspace-scoped 키-값 컬렉션)
        "memory.bb_create" => memory::handle_bb_create(state, engine, caller, id, &request.params),
        "memory.bb_put" => memory::handle_bb_put(state, engine, caller, id, &request.params),
        "memory.bb_get" => memory::handle_bb_get(state, engine, caller, id, &request.params),
        "memory.bb_get_all" => {
            memory::handle_bb_get_all(state, engine, caller, id, &request.params)
        }
        "memory.bb_get_meta" => {
            memory::handle_bb_get_meta(state, engine, caller, id, &request.params)
        }
        "memory.bb_delete_field" => {
            memory::handle_bb_delete_field(state, engine, caller, id, &request.params)
        }
        "memory.bb_delete" => memory::handle_bb_delete(state, engine, caller, id, &request.params),
        "memory.bb_list" => memory::handle_bb_list(state, engine, caller, id, &request.params),
        "memory.bb_exists" => memory::handle_bb_exists(state, engine, caller, id, &request.params),
        // memory: bb snapshot (Phase 7.4)
        "memory.bb_snapshot" => {
            memory::handle_bb_snapshot(state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_get" => {
            memory::handle_bb_snapshot_get(state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_list" => {
            memory::handle_bb_snapshot_list(state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_delete" => {
            memory::handle_bb_snapshot_delete(state, engine, caller, id, &request.params)
        }
        "memory.bb_snapshot_restore" => {
            memory::handle_bb_snapshot_restore(state, engine, caller, id, &request.params)
        }
        // memory: plan (Phase 7.2 — workspace-scoped 선언적 work breakdown)
        "memory.plan_create" => {
            memory::handle_plan_create(state, engine, caller, id, &request.params)
        }
        "memory.plan_get" => memory::handle_plan_get(state, engine, caller, id, &request.params),
        "memory.plan_list" => memory::handle_plan_list(state, engine, caller, id, &request.params),
        "memory.plan_delete" => {
            memory::handle_plan_delete(state, engine, caller, id, &request.params)
        }
        "memory.plan_add_step" => {
            memory::handle_plan_add_step(state, engine, caller, id, &request.params)
        }
        "memory.plan_remove_step" => {
            memory::handle_plan_remove_step(state, engine, caller, id, &request.params)
        }
        "memory.plan_update_step" => {
            memory::handle_plan_update_step(state, engine, caller, id, &request.params)
        }
        // memory: cache (Phase 7.3 — workspace-scoped TTL 캐시)
        "memory.cache_put" => memory::handle_cache_put(state, engine, caller, id, &request.params),
        "memory.cache_get" => memory::handle_cache_get(state, engine, caller, id, &request.params),
        "memory.cache_invalidate" => {
            memory::handle_cache_invalidate(state, engine, caller, id, &request.params)
        }
        "memory.cache_clear" => {
            memory::handle_cache_clear(state, engine, caller, id, &request.params)
        }
        "memory.cache_list" => {
            memory::handle_cache_list(state, engine, caller, id, &request.params)
        }
        // approval (휴먼 핸드오프) — await 는 process_ipc 에서 worker thread 로 분리 처리.
        "approval.request" => approval::handle_request(state, engine, caller, id, &request.params),
        "approval.respond" => approval::handle_respond(state, engine, caller, id, &request.params),
        "approval.cancel" => approval::handle_cancel(state, engine, caller, id, &request.params),
        "approval.get" => approval::handle_get(state, engine, caller, id, &request.params),
        "approval.list" => approval::handle_list(state, engine, caller, id, &request.params),
        "approval.history" => approval::handle_history(state, engine, caller, id, &request.params),
        "approval.summary.set" => {
            approval::handle_summary_set(state, engine, caller, id, &request.params)
        }
        "approval.summary.get" => {
            approval::handle_summary_get(state, engine, caller, id, &request.params)
        }
        // telemetry (관측 / 비용) — 단계 4.1
        "telemetry.record" => telemetry::handle_record(state, engine, caller, id, &request.params),
        "telemetry.record_batch" => {
            telemetry::handle_record_batch(state, engine, caller, id, &request.params)
        }
        "telemetry.summary" => {
            telemetry::handle_summary(state, engine, caller, id, &request.params)
        }
        "telemetry.timeseries" => {
            telemetry::handle_timeseries(state, engine, caller, id, &request.params)
        }
        "telemetry.top" => telemetry::handle_top(state, engine, caller, id, &request.params),
        // telemetry.cap — Phase 4.3 (CRUD; eval/action wiring 은 후속)
        "telemetry.cap.set" => {
            telemetry::handle_cap_set(state, engine, caller, id, &request.params)
        }
        "telemetry.cap.list" => {
            telemetry::handle_cap_list(state, engine, caller, id, &request.params)
        }
        "telemetry.cap.remove" => {
            telemetry::handle_cap_remove(state, engine, caller, id, &request.params)
        }
        "telemetry.cap.status" => {
            telemetry::handle_cap_status(state, engine, caller, id, &request.params)
        }
        "telemetry.cap.reset" => {
            telemetry::handle_cap_reset(state, engine, caller, id, &request.params)
        }
        // telemetry.anomaly — Phase 4.4 (영속 anomaly 조회만; 검출은 dispatcher 후크)
        "telemetry.anomaly.list" => {
            telemetry::handle_anomaly_list(state, engine, caller, id, &request.params)
        }
        // telemetry.session_summary — Phase 4.5 (메트릭/승인/이상 집계)
        "telemetry.session_summary" => {
            telemetry::handle_session_summary(state, engine, caller, id, &request.params)
        }
        // agent.task_* — Phase 5.1 (DAG + state 머신)
        "agent.task_create" => {
            agent::handle_task_create(state, engine, caller, id, &request.params)
        }
        "agent.task_list" => agent::handle_task_list(state, engine, caller, id, &request.params),
        "agent.task_get" => agent::handle_task_get(state, engine, caller, id, &request.params),
        "agent.task_await" => agent::handle_task_await(state, engine, caller, id, &request.params),
        "agent.task_cancel" => {
            agent::handle_task_cancel(state, engine, caller, id, &request.params)
        }
        "agent.task_retry" => agent::handle_task_retry(state, engine, caller, id, &request.params),
        "agent.task_graph" => agent::handle_task_graph(state, engine, caller, id, &request.params),
        // agent.barrier_* / semaphore_* — Phase 5.2 (poll-based 동기화 primitive)
        "agent.barrier_create" => {
            agent::handle_barrier_create(state, engine, caller, id, &request.params)
        }
        "agent.barrier_signal" => {
            agent::handle_barrier_signal(state, engine, caller, id, &request.params)
        }
        "agent.barrier_await" => {
            agent::handle_barrier_await(state, engine, caller, id, &request.params)
        }
        "agent.barrier_state" => {
            agent::handle_barrier_state(state, engine, caller, id, &request.params)
        }
        "agent.semaphore_create" => {
            agent::handle_semaphore_create(state, engine, caller, id, &request.params)
        }
        "agent.semaphore_acquire" => {
            agent::handle_semaphore_acquire(state, engine, caller, id, &request.params)
        }
        "agent.semaphore_release" => {
            agent::handle_semaphore_release(state, engine, caller, id, &request.params)
        }
        // agent.lease_* — Phase 5.3 (협조적 점유 마커 + TTL)
        "agent.lease_acquire" => {
            agent::handle_lease_acquire(state, engine, caller, id, &request.params)
        }
        "agent.lease_release" => {
            agent::handle_lease_release(state, engine, caller, id, &request.params)
        }
        "agent.lease_list" => agent::handle_lease_list(state, engine, caller, id, &request.params),
        // agent.task_reduce — Phase 5.4 (결과 합성: first_success / all / merge_json / concat_text / custom)
        "agent.task_reduce" => {
            agent::handle_task_reduce(state, engine, caller, id, &request.params)
        }
        // agent.rate_limit_* — Phase 5.5 (token bucket 시간당 비율 제한)
        "agent.rate_limit_set" => {
            agent::handle_rate_limit_set(state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_list" => {
            agent::handle_rate_limit_list(state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_remove" => {
            agent::handle_rate_limit_remove(state, engine, caller, id, &request.params)
        }
        "agent.rate_limit_status" => {
            agent::handle_rate_limit_status(state, engine, caller, id, &request.params)
        }
        // session.* — Phase 6.2c (자식 agent 신원 토큰 관리)
        "session.issue" => session::handle_issue(caller, id, &request.params),
        "session.revoke" => session::handle_revoke(id, &request.params),
        "session.list" => session::handle_list(id),
        _ => return None,
    })
}

#[cfg(debug_assertions)]
fn route_debug_handler(
    state: &mut AppState,
    engine: &mut crate::engine_state::CoreState,
    request: &JsonRpcRequest,
    id: serde_json::Value,
) -> Option<JsonRpcResponse> {
    Some(match request.method.as_str() {
        "ui.state" => handle_ui_state(state, engine, id),
        "debug.cell_info" => debug::handle_debug_cell_info(state, engine, id, &request.params),
        "debug.screen_attrs" => {
            debug::handle_debug_screen_attrs(state, engine, id, &request.params)
        }
        "debug.glyph_color" => debug::handle_debug_glyph_color(state, engine, id, &request.params),
        "debug.feed_bytes" => debug::handle_debug_feed_bytes(state, engine, id, &request.params),
        "debug.inject_mouse" => {
            debug::handle_debug_inject_mouse(state, engine, id, &request.params)
        }
        "debug.inject_key" => debug::handle_debug_inject_key(state, engine, id, &request.params),
        // 도구 메뉴 — 사용자 클릭 자동화. release 미노출.
        "debug.tool.list" => tool::handle_list(state, engine, id),
        "debug.tool.invoke" => tool::handle_invoke(state, engine, id, &request.params),
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
fn apply_meta(surface_id: u32, meta: Option<&serde_json::Map<String, serde_json::Value>>) {
    if let Some(map) = meta {
        for (key, value) in map {
            if let Some(v) = value.as_str() {
                if let Err(e) = crate::surface_meta::SurfaceMetaStore::set(surface_id, key, v) {
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
    engine: &crate::engine_state::CoreState,
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
    engine: &crate::engine_state::CoreState,
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
    JsonRpcResponse::success(
        id,
        json!({
            "settings_open": state.settings_open,
            "notification_panel_open": state.popups.is_open("notifications"),
            "active_workspace": state.active_workspace,
            "workspace_count": engine.workspaces.len(),
            "pane_count": pane_count,
            "tab_count": tab_count,
        }),
    )
}

fn handle_tree(
    state: &AppState,
    engine: &crate::engine_state::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let tree: Vec<_> = engine
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
        .collect();
    JsonRpcResponse::success(id, json!(tree))
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
