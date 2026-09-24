//! 헤드리스 IPC를 단일 engine에 전달한다. 창 선택은 없지만 공용 권한·감사·승인 검사는 유지한다.
//! App 소유 조회를 처리하고, 매니페스트 namespace에 속한 요청은 engine handler보다 먼저 플러그인에 전달한다.

#![cfg(not(feature = "gui"))]

use crate::app::App;
use crate::core::CoreState;
use crate::ipc::caller::resolve_caller_from_envelope;
use crate::ipc::server::send_response;
use crate::state::AppState;

/// GUI와 같은 IPC 회차 예산을 사용한다. 요청이 만든 Intent는 응답 전에 적용한다.
pub(crate) fn pump_ipc(
    app: &mut App,
    state: &mut AppState,
    engine: &mut CoreState,
) -> std::ops::ControlFlow<()> {
    let mut round = crate::app::ipc_round::IpcRound::begin();
    while let Some(cmd) = round.next(app.hub.ipc_server.as_deref()) {
        let observed = crate::app::ipc_round::CommandObservation::begin(app.core.pressure(), &cmd);
        let flow = dispatch_command(app, state, engine, cmd);
        observed.finish(app.core.slow_requests());
        if flow.is_break() {
            round.finish(app.core.pressure(), app.core.dispatch());
            return std::ops::ControlFlow::Break(());
        }
    }
    // 남은 명령을 보고 다시 깨우는 일은 호출자가 맡는다.
    round.finish(app.core.pressure(), app.core.dispatch());
    std::ops::ControlFlow::Continue(())
}

fn dispatch_command(
    app: &mut App,
    state: &mut AppState,
    engine: &mut CoreState,
    cmd: crate::ipc::server::IpcCommand,
) -> std::ops::ControlFlow<()> {
    // 큐 대기는 이미 계측했다. 실행 기한이 지났으면 권한·rate limit을 소비하기 전에 응답한다.
    if !crate::app::ipc_round::claim_or_answer(&cmd, app.core.dispatch()) {
        return std::ops::ControlFlow::Continue(());
    }
    let caller = match resolve_caller_from_envelope(&app.core, &cmd.request) {
        Ok(c) => c,
        Err(resp) => {
            send_response(&cmd.response_tx, resp);
            return std::ops::ControlFlow::Continue(());
        }
    };
    // App 전용 응답도 공용 검사 뒤에 처리한다. checked 요청은 다시 검사하지 않는다.
    let checked = match crate::ipc::handler::check_request(
        &mut app.core,
        state,
        engine,
        &cmd.request,
        &caller,
    ) {
        Ok(checked) => checked,
        Err(response) => {
            send_response(&cmd.response_tx, response);
            return std::ops::ControlFlow::Continue(());
        }
    };
    match intercept_app_layer(app, state, engine, &caller, &cmd) {
        Some(Intercepted::Answered) => return std::ops::ControlFlow::Continue(()),
        #[cfg(debug_assertions)]
        Some(Intercepted::Shutdown) => return std::ops::ControlFlow::Break(()),
        None => {}
    }
    // 다른 engine으로 보낼 수 없어 잘못 지정한 자원은 거절한다.
    // 플러그인 namespace의 요청까지 차단하지 않도록 호스트 예약 prefix에만 적용한다.
    if let Some(rid) =
        crate::core::request_target::request_resource_id(&cmd.request.method, &cmd.request.params)
        && crate::core::request_target::prefix_is_host_reserved(&cmd.request.method)
        && !crate::core::request_target::engine_has_resource(engine, rid)
    {
        let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        send_response(
            &cmd.response_tx,
            crate::ipc::protocol::JsonRpcResponse::invalid_params(
                id,
                crate::core::request_target::unowned_target_message(rid, &cmd.request.method),
            ),
        );
        return std::ops::ControlFlow::Continue(());
    }
    // engine 응답 오류를 라우팅 신호로 사용하지 않고 namespace 소유자로 먼저 결정한다.
    if forward_to_plugin_namespace(app, engine, &caller, &cmd) {
        return std::ops::ControlFlow::Continue(());
    }
    // kind 소유자만 준비한다. namespace 전달과 달리 IPC hook extension은 여기서 시작하지 않는다.
    super::headless_plugins::ensure_plugin_for_surface_kind(app, state, engine, &cmd.request);
    let resp = crate::ipc::handler::handle_checked_request(&mut app.core, state, engine, &checked);
    // 응답 전에 요청의 Intent와 후속 이벤트를 적용한다.
    crate::intent::headless::drain_pending_intents(&mut app.core, state, engine);
    crate::intent::headless::drain_pending_host_events(&app.core, state, engine);
    send_response(&cmd.response_tx, resp);
    std::ops::ControlFlow::Continue(())
}

enum Intercepted {
    Answered,
    /// debug 전용 system.shutdown 요청은 응답 후 호스트 루프를 끝낸다.
    #[cfg(debug_assertions)]
    Shutdown,
}

/// TimerHub·PluginManager·Lua처럼 App 소유 상태가 필요한 요청을 처리한다.
fn intercept_app_layer(
    app: &mut App,
    window: &mut dyn crate::ipc::window_port::IpcWindow,
    engine: &mut CoreState,
    caller: &crate::ipc::caller::CallerContext,
    cmd: &crate::ipc::server::IpcCommand,
) -> Option<Intercepted> {
    // 표에 등록된 Mutate 요청은 GUI와 같은 멱등 키 처리를 거친다.
    if let Some(hit) = crate::ipc::handler::idempotency::run_app_layer(
        caller,
        cmd,
        Some(Intercepted::Answered),
        Option::is_some,
        |relayed| intercept_app_layer(app, window, engine, caller, relayed),
    ) {
        return hit;
    }
    if cmd.request.method == "timer.list" {
        let resp = crate::ipc::protocol::JsonRpcResponse::success(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            app.timer_list_json(std::time::Instant::now()),
        );
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }
    // 조회에는 매니저 메타데이터만 필요하다. 플러그인 설치·권한 부여·실행을 하지 않는다.
    if crate::ipc::handler::plugin::is_readonly_method(&cmd.request.method) {
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        let surface_registry = engine.surface_registry.clone();
        if let Some(resp) = crate::ipc::handler::plugin::dispatch_readonly(
            &app.core,
            app.plugin_manager.as_ref(),
            &surface_registry,
            &cmd.request.method,
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            &cmd.request.params,
        ) {
            send_response(&cmd.response_tx, resp);
            return Some(Intercepted::Answered);
        }
    }
    // 지정한 플러그인만 켜야 하므로 전체 discover_and_start 대신 공용 enable/disable을 사용한다.
    if crate::ipc::handler::plugin::is_lifecycle_toggle_method(&cmd.request.method) {
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        let hook_events = engine.plugin_hook_events.clone();
        let surface_registry = engine.surface_registry.clone();
        if let Some((resp, events)) = crate::ipc::handler::plugin::dispatch_lifecycle_toggle(
            app.plugin_manager.as_mut(),
            &surface_registry,
            &cmd.request.method,
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            &cmd.request.params,
        ) {
            if let Some(mgr) = app.plugin_manager.as_mut() {
                crate::ipc::handler::plugin::cascade_toggle_events_headless(
                    mgr,
                    &hook_events,
                    events,
                );
            }
            send_response(&cmd.response_tx, resp);
            return Some(Intercepted::Answered);
        }
    }
    // 승인 popup이 없어도 approval 레코드를 만들고 await·list·respond로 처리할 수 있다.
    if cmd.request.method == "plugin.request_permission" {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let resp = crate::ipc::handler::session::handle_request_permission(
            &mut app.core,
            window,
            engine,
            &caller,
            rpc_id,
            &cmd.request.params,
        );
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }

    // 사건 조회도 메타데이터만 준비하며 플러그인 프로세스를 실행하지 않는다.
    if cmd.request.method == "events.fetch" {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        let args =
            match crate::ipc::handler::events::FetchParams::parse(&cmd.request.params, &rpc_id) {
                Ok(a) => a,
                Err(resp) => {
                    send_response(&cmd.response_tx, resp);
                    return Some(Intercepted::Answered);
                }
            };
        let Some(mgr) = app.plugin_manager.as_ref() else {
            send_response(
                &cmd.response_tx,
                crate::ipc::handler::events::no_bus(rpc_id),
            );
            return Some(Intercepted::Answered);
        };
        let bus = mgr.event_bus.clone();
        if args.wait.is_zero() {
            send_response(
                &cmd.response_tx,
                crate::ipc::handler::events::fetch(&bus, &args, rpc_id),
            );
            return Some(Intercepted::Answered);
        }
        // 긴 조회 대기가 다른 IPC 처리를 막지 않도록 워커에서 기다린다.
        let response_tx = cmd.response_tx.clone();
        std::thread::spawn(move || {
            let resp = crate::ipc::handler::events::fetch(&bus, &args, rpc_id);
            send_response(&response_tx, resp);
        });
        return Some(Intercepted::Answered);
    }

    #[cfg(debug_assertions)]
    if let Some(hit) = intercept_debug_app_layer(app, engine, cmd) {
        return Some(hit);
    }
    {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        match cmd.request.method.as_str() {
            "clipboard.set_text" => {
                let resp = crate::core::app_surface::clipboard_set_text(
                    &app.core,
                    rpc_id,
                    &cmd.request.params,
                );
                send_response(&cmd.response_tx, resp);
                return Some(Intercepted::Answered);
            }
            "remote.workspaces" => {
                crate::core::app_surface::spawn_remote_workspaces(
                    rpc_id,
                    &cmd.request.params,
                    &cmd.response_tx,
                );
                return Some(Intercepted::Answered);
            }
            // 헤드리스의 실제 engine은 App.core_state에 없고 이 함수 인자로 전달된다.
            "agent.task_await" => {
                crate::ipc::handler::agent::task::spawn_task_await(
                    engine.task_waker_hub.clone(),
                    app.core.memory_arc(),
                    engine.agent_seq.clone(),
                    rpc_id,
                    cmd.request.params.clone(),
                    &cmd.response_tx,
                );
                return Some(Intercepted::Answered);
            }
            "approval.await" => {
                crate::ipc::handler::approval::spawn_approval_await(
                    engine.approval_store.clone(),
                    app.core.memory_arc(),
                    rpc_id,
                    cmd.request.params.clone(),
                    &cmd.response_tx,
                );
                return Some(Intercepted::Answered);
            }
            // winit proxy가 없어 성공 응답 뒤 호출자에게 루프 종료를 반환한다.
            #[cfg(debug_assertions)]
            "system.shutdown" => {
                send_response(
                    &cmd.response_tx,
                    crate::ipc::protocol::JsonRpcResponse::success(
                        rpc_id,
                        serde_json::json!({"shutdown": true}),
                    ),
                );
                return Some(Intercepted::Shutdown);
            }
            _ => {}
        }
    }
    None
}

/// 창 없이 처리 가능한 debug 메서드. 지원 범위는 docs/dev-guide/headless-ipc-surface.md를 따른다.
#[cfg(debug_assertions)]
fn intercept_debug_app_layer(
    app: &mut App,
    engine: &CoreState,
    cmd: &crate::ipc::server::IpcCommand,
) -> Option<Intercepted> {
    let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
    if cmd.request.method == "debug.lua.eval" {
        let resp = crate::core::app_surface_debug::lua_eval(
            app.lua_engine.as_ref(),
            rpc_id,
            &cmd.request.params,
        );
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }
    // 아직 실행한 플러그인이 없어도 매니저를 준비해 빈 조회 결과를 반환한다.
    if cmd.request.method.starts_with("debug.event_bus.") {
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        let resp = crate::ipc::handler::debug_plugin::handle_event_bus(
            app.plugin_manager.as_mut(),
            &cmd.request.method,
            &cmd.request.params,
            rpc_id,
        );
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }
    // popup은 조회만 제공한다. 헤드리스에 닫기 처리가 없어 open을 허용하면 정리할 수 없다.
    if cmd.request.method == "debug.popup.list" {
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        let resp = crate::ipc::handler::popup::handle_list(app.plugin_manager.as_ref(), rpc_id);
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }
    // 전체화면 무대 선언은 조회할 수 있지만 창이 필요한 open·close·state는 처리하지 않는다.
    if cmd.request.method == "debug.fullscreen.list" {
        let resp = crate::core::app_surface_debug::fullscreen_list(rpc_id);
        send_response(&cmd.response_tx, resp);
        return Some(Intercepted::Answered);
    }
    if cmd.request.method == "debug.extension.invoke_hook" {
        super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
        crate::ipc::handler::debug_plugin::handle_extension_invoke_hook(
            app.plugin_manager.as_mut(),
            &cmd.request.params,
            rpc_id,
            cmd.response_tx.clone(),
        );
        return Some(Intercepted::Answered);
    }
    None
}

/// 메타데이터에서 namespace 소유자를 찾은 뒤 활성 owner와 일치하는 IPC hook extension만 준비한다.
/// 미등록 prefix는 시작하지 않는다. 등록된 prefix 안의 메서드 유효성은 owner가 판단한다.
/// 표에 등록된 Mutate만 멱등 키를 처리하며 플러그인 고유 메서드는 개입하지 않는다.
#[cfg(not(feature = "gui"))]
fn forward_to_plugin_namespace(
    app: &mut App,
    engine: &CoreState,
    caller: &crate::ipc::caller::CallerContext,
    cmd: &crate::ipc::server::IpcCommand,
) -> bool {
    super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
    let owns = app
        .plugin_manager
        .as_ref()
        .is_some_and(|mgr| mgr.owns_namespace(&cmd.request.method));
    if !owns {
        return false;
    }
    let Some(mgr) = app.plugin_manager.as_mut() else {
        return false;
    };
    crate::ipc::handler::idempotency::forward_keeping_the_key(caller, cmd, |c| {
        let id = c.request.id.clone().unwrap_or(serde_json::Value::Null);
        mgr.forward_namespace_call(
            &c.request.method,
            c.request.params.clone(),
            None, // CLI/사용자 호출 — plugin → plugin 호출은 별도 경로(gui 와 같다).
            id,
            c.response_tx.clone(),
            Some(c.request_seq()),
        );
    });
    true
}
