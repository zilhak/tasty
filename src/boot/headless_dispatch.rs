//! Headless 빌드 전용 IPC dispatch.
//!
//! gui 의 `App::process_ipc` (`src/app/ipc.rs`, `#[cfg(feature="gui")]`) 는 view /
//! parked_states / plugin_manager 의존이 큰 5-step 라우터다. headless 는 engine 이
//! 단 하나뿐이라 그 전부가 불필요하므로, caller 해석 → `handle_with_caller` 직결로
//! 간소화한다.
//!
//! 생략(gui 대비):
//! - caller elevation / audit-on-deny (view 의존; deny 자체는 handle_with_caller 가 회신)
//! - app_methods / window_required / debug step (창/스크린샷/system.shutdown 등).
//!   예외는 `timer.list` 하나 — 읽는 대상(TimerHub)이 `App` 에 있어 engine handler
//!   로는 답할 수 없고, 관측이 gui 에서만 되면 headless 인스턴스의 wakeup 원인을
//!   물어볼 방법이 사라진다.
//! - dispatch_list_global / find_request_owner / parked fallback (engine 1 개)
//!
//! plugin namespace forward 는 **생략하지 않는다** — plugin 이 contribute 한
//! namespace(`markdown.*` 등)는 CLI 로 노출된 에이전트 표면이라 headless 에서도
//! 답해야 한다(`docs/identity.md` 원칙 2). 다만 배치가 gui 와 다르다: gui 는
//! namespace forward 를 engine handler **앞**에 두지만(`app/ipc/routing.rs` step 5),
//! 여기서는 engine handler 가 `-32601` 을 돌려준 **뒤**의 fallback 이다. 그래야
//! 호스트가 답할 수 있는 메서드의 경로가 한 줄도 안 바뀌고, plugin 기동을
//! "실제로 plugin 메서드가 불렸을 때" 로 미룰 수 있다 — namespace 표는 plugin 이
//! spawn 돼야 채워지므로(`manager/lifecycle.rs` 의 `on_plugin_spawn_success`),
//! 부르기 전에 아는 방법이 없다.

#![cfg(not(feature = "gui"))]

use crate::app::App;
use crate::core::CoreState;
use crate::ipc::caller::resolve_caller_from_envelope;
use crate::ipc::server::send_response;
use crate::state::AppState;

/// IPC 큐를 비차단으로 비우고 각 명령을 단일 engine 으로 dispatch 한다.
/// `IpcReady` 수신 시 headless 메인 루프가 호출.
pub(crate) fn pump_ipc(
    app: &mut App,
    state: &mut AppState,
    engine: &mut CoreState,
) -> std::ops::ControlFlow<()> {
    // 큐를 한 번에 drain (try_recv 결과는 owned 라 borrow 가 따라가지 않는다).
    let mut pending = Vec::new();
    if let Some(ipc) = app.hub.ipc_server.as_ref() {
        while let Ok(cmd) = ipc.try_recv() {
            pending.push(cmd);
        }
    }

    for cmd in pending {
        // 1) caller 해석 (Local / Agent / 세션 토큰 검증). 실패 시 에러를 그대로 회신.
        let caller = match resolve_caller_from_envelope(&app.core, &cmd.request) {
            Ok(c) => c,
            Err(resp) => {
                send_response(&cmd.response_tx, resp);
                continue;
            }
        };
        // 2) 허브 관측만 App 층에서 가로챈다 — `timer.list` 가 읽는 TimerHub 는
        //    `App` 필드(+ plugin manager 자기 허브)라 `CoreState` 만 받는 engine
        //    handler 에서는 닿지 않는다. gui 의 app_methods step 과 같은 함수를 쓴다.
        if cmd.request.method == "timer.list" {
            let resp = crate::ipc::protocol::JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                app.timer_list_json(std::time::Instant::now()),
            );
            send_response(&cmd.response_tx, resp);
            continue;
        }
        // 2b) 읽기 전용 `plugin.*` 조회도 App 층에서 답한다 — `plugin_manager` 는 `App`
        //     필드라 engine handler 가 닿지 않는다. gui 라우터와 **같은 함수**를 쓴다
        //     (`handler::plugin::dispatch_readonly`), 두 벌로 두면 갈라지기 때문이다.
        //
        //     매니저는 여기서 **메타데이터 층까지만** 세운다. plugin 프로세스를 띄우거나
        //     번들을 설치·권한 grant 하는 것은 조회의 부수효과일 수 없다
        //     (`headless_plugins::ensure_plugin_manager_metadata` 주석).
        if crate::ipc::handler::plugin::is_readonly_method(&cmd.request.method) {
            super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
            if let Some(resp) = crate::ipc::handler::plugin::dispatch_readonly(
                &app.core,
                app.plugin_manager.as_ref(),
                &cmd.request.method,
                cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
                &cmd.request.params,
            ) {
                send_response(&cmd.response_tx, resp);
                continue;
            }
        }
        // 2c-debug) debug 빌드의 app 층 표면 중 **창이 없어도 답이 정의되는 것.**
        //
        //     gui 는 이것들을 라우터의 debug step(`src/app/ipc/debug_methods.rs`)에서
        //     처리하는데 그 step 자체가 헤드리스에 없다. 읽는 것은 `App` 의
        //     `lua_engine` / `plugin_manager` 이고 둘 다 feature 게이트가 없는 필드다 —
        //     창을 안 보는데 자리가 없어서 `-32601` 이던 것이라, 에이전트 검증 표면이
        //     헤드리스에서만 사라지는 형태였다(`docs/identity.md` 원칙 2).
        //
        //     여기 **없는** debug 메서드(popup · banner · fullscreen · modifier_hint ·
        //     tool · inject_* · selection · pending_menu · focused_surface · info ·
        //     gpu.stall)는 창·렌더러·egui 입력 큐를 읽는다. 판정은 메서드별로
        //     `docs/dev-guide/headless-ipc-surface.md` 에 있다.
        #[cfg(debug_assertions)]
        {
            let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
            if cmd.request.method == "debug.lua.eval" {
                let resp = crate::core::app_surface_debug::lua_eval(
                    app.lua_engine.as_ref(),
                    rpc_id,
                    &cmd.request.params,
                );
                send_response(&cmd.response_tx, resp);
                continue;
            }
            // 아래 둘은 매니저를 본다. **메타데이터 층까지만** 세운다 — 조회가 plugin
            // 프로세스를 띄우면 관측이 자기 대상을 바꾼다(ADR-0136). 그래서 아직 아무
            // plugin 도 안 뜬 데몬에서는 구독자가 0 으로 나오고, 그것이 그 시점의
            // 사실이다(매니저가 아예 없을 때의 `-32000` 과 구분된다).
            if cmd.request.method.starts_with("debug.event_bus.") {
                super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
                let resp = crate::ipc::handler::debug_plugin::handle_event_bus(
                    app.plugin_manager.as_mut(),
                    &cmd.request.method,
                    &cmd.request.params,
                    rpc_id,
                );
                send_response(&cmd.response_tx, resp);
                continue;
            }
            // plugin popup **조회**. 매니저만 읽어 창이 없어도 답이 정의된다
            // (gui 와 같은 함수를 부른다).
            //
            // `open`/`close` 는 여기 없다. 컴파일이 막아서가 아니라 — 헤드리스에는
            // plugin popup 을 **닫는 경로가 하나도 없기** 때문이다(debug close 도,
            // plugin 자신의 release `popup.close` 도 gui 게이트 안의 `app::dispatch`
            // 에 산다). open 만 열면 그 빌드에서 닫을 수 없는 인스턴스가 남는다 —
            // 표면을 넓히면서 정리 책임을 새로 지는 형태라 열지 않았다.
            if cmd.request.method == "debug.popup.list" {
                super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
                let resp =
                    crate::ipc::handler::popup::handle_list(app.plugin_manager.as_ref(), rpc_id);
                send_response(&cmd.response_tx, resp);
                continue;
            }
            // 등록된 전체화면 무대 **조회**. 읽는 것이 gui 무관 메타 표뿐이라 창이
            // 없어도 답이 정의된다(gui 와 같은 함수를 부른다).
            //
            // 같은 갈래의 `open`/`close`/`state` 는 여기 없다 — 셋 다
            // `pick_debug_window` 로 창을 지목한다. 무대는 창 단위라 창이 없으면 답이
            // 정의되지 않는다. "컴파일되는가" 와 "열어도 되는가" 는 다른 물음이다.
            if cmd.request.method == "debug.fullscreen.list" {
                let resp = crate::core::app_surface_debug::fullscreen_list(rpc_id);
                send_response(&cmd.response_tx, resp);
                continue;
            }
            if cmd.request.method == "debug.extension.invoke_hook" {
                super::headless_plugins::ensure_plugin_manager_metadata(app, engine);
                crate::ipc::handler::debug_plugin::handle_extension_invoke_hook(
                    app.plugin_manager.as_mut(),
                    &cmd.request.params,
                    rpc_id,
                    cmd.response_tx.clone(),
                );
                continue;
            }
        }
        // 2d) app 층 표면 중 **창이 없어도 답이 정의되는 것**을 여기서 답한다.
        //     gui 의 `app_methods` step 과 **같은 함수**를 부른다
        //     (`crate::core::app_surface`) — 두 벌로 두면 한쪽만 고쳐지는 순간 갈라진다.
        //
        //     여기 없는 app 층 메서드(`window.*` · `view.*` · `ui.screenshot` ·
        //     `remote.attach` · `system.gpu_stats`)는 읽는 것이 `App.view` 라서
        //     헤드리스에 대응물이 없다. 그 판정은 메서드별로
        //     `docs/dev-guide/headless-ipc-surface.md` 에 적혀 있다.
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
                    continue;
                }
                "remote.workspaces" => {
                    crate::core::app_surface::spawn_remote_workspaces(
                        rpc_id,
                        &cmd.request.params,
                        &cmd.response_tx,
                    );
                    continue;
                }
                // 대기 대상 store 는 **이 engine** 것이다. gui 처럼 창·parked 를 훑을
                // 일이 없다 — 헤드리스는 engine 이 하나뿐이고 `app.core_state` 는
                // 부팅이 채우지 않는다(`boot::bootstrap_engine` 이 새로 만들어 돌려준다).
                "agent.task_await" => {
                    crate::core::app_surface::spawn_task_await(
                        engine.task_waker_hub.clone(),
                        app.core.memory_arc(),
                        engine.agent_seq.clone(),
                        rpc_id,
                        cmd.request.params.clone(),
                        &cmd.response_tx,
                    );
                    continue;
                }
                "approval.await" => {
                    crate::core::app_surface::spawn_approval_await(
                        engine.approval_store.clone(),
                        app.core.memory_arc(),
                        rpc_id,
                        cmd.request.params.clone(),
                        &cmd.response_tx,
                    );
                    continue;
                }
                // 데몬은 자기 IPC 로 멈출 수 있어야 한다. gui 는 `AppEvent::Shutdown` 을
                // winit proxy 로 보내는데 헤드리스엔 proxy 가 없으므로, 호출자에게 성공을
                // 회신한 뒤 **호출자(run loop)에게 break 를 돌려준다.** gui 와 같은
                // debug 격리(`DEBUG_METHODS`)를 유지한다.
                #[cfg(debug_assertions)]
                "system.shutdown" => {
                    send_response(
                        &cmd.response_tx,
                        crate::ipc::protocol::JsonRpcResponse::success(
                            rpc_id,
                            serde_json::json!({"shutdown": true}),
                        ),
                    );
                    return std::ops::ControlFlow::Break(());
                }
                _ => {}
            }
        }
        // 2c) 지목한 대상을 이 engine 이 안 가졌으면 거절한다 — gui 와 같은 판정
        //     (`app/ipc/routing.rs`). 헤드리스는 engine 이 하나라 라우팅할 곳이 없지만,
        //     **판정은 있어야 한다**: 없으면 대상을 잘못 적은 요청이 그대로 실행된다.
        //     실측(2026-09-05): `workspace.create {workspace_id: <없는 id>}` 가 성공을
        //     돌려주고 워크스페이스를 만들었다 — 핸들러가 그 키를 안 읽기 때문이다.
        //
        //     예약 prefix 로 한정하는 이유는 아래 5) 와의 순서다. 예약되지 않은
        //     prefix 는 plugin 이 점유할 수 있어서, 여기서 자르면 forward 될 호출을
        //     불러 보기도 전에 죽인다. 예약된 것은 어떤 plugin 도 못 가지므로
        //     (매니페스트 검증이 거절한다) 그런 위험이 없다.
        if let Some(rid) = crate::core::request_target::request_resource_id(
            &cmd.request.method,
            &cmd.request.params,
        ) && crate::core::request_target::prefix_is_host_reserved(&cmd.request.method)
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
            continue;
        }
        // 3) engine handler 직결. 권한 게이트 / audit / rate-limit / cap 은
        //    handle_with_caller 내부가 자체 수행한다.
        let resp = crate::ipc::handler::handle_with_caller(
            &mut app.core,
            state,
            engine,
            &cmd.request,
            &caller,
        );
        // 4) 핸들러가 발화한 Intent 를 **응답 전에** 적용한다. gui 의
        //    `App::dispatch_with_caller` 가 응답 반환 전에 `dispatch_pending_intents`
        //    를 부르는 것과 같은 계약이며, 이게 없으면 큐가 프로세스 수명 동안 쌓이고
        //    (`docs/adr/0111-headless-drains-the-intent-queue.md`) set_mark /
        //    completion / notification 같은 에이전트 표면이 headless 에서 무응답이 된다.
        crate::intent::headless::drain_pending_intents(&mut app.core, state, engine);
        crate::intent::headless::drain_pending_host_events(&app.core, state, engine);
        // 5) 호스트가 모르는 메서드는 plugin namespace 로 넘긴다. 넘어갔으면 응답은
        //    plugin 이 줄 때까지 보류되고, `headless_plugins::pump_plugins` 의
        //    `mgr.pump` 가 도착 시 client 에 회신한다(gui 와 같은 계약).
        if is_unrouted_here(&resp) && forward_to_plugin_namespace(app, engine, &cmd) {
            continue;
        }
        send_response(&cmd.response_tx, resp);
    }
    std::ops::ControlFlow::Continue(())
}

/// engine handler 가 "여기서는 이 이름을 라우팅하지 못했다" 로 답했는가.
///
/// **두 코드를 함께 본다.** 종단(`unrouted_for_external_caller`)은 같은 사실을 두 갈래로
/// 답한다 — 표에 없으면 `-32601`, 표에 있는데 이 조합에 arm 이 없으면 `-32017`. forward
/// 가 물어야 할 것은 그 구분이 아니라 **"engine 이 못 답했나"** 하나이므로 둘 다 신호다.
///
/// 한쪽만 보면 무엇이 죽는지는 실측돼 있다(2026-09-05). `-32601` 만 보면 표에 등재된 채
/// 번들 plugin namespace 아래 있는 여덟(`image.*` 7 · `markdown.navigate`)이 `-32017` 을
/// 받아 forward 를 못 타고, plugin 이 답하던 호출이 host 의 거절로 바뀐다 — 그 여덟에게
/// "이 바이너리에 없다" 는 **사실도 아니다**(plugin 이 답한다).
///
/// 이 함수가 코드를 신호로 쓰는 것 자체가 gui 와 다른 재료다 — gui 는 같은 forward 를
/// `src/app/ipc/routing.rs` 에서 **namespace 해소**로, 그것도 종단보다 앞에서 정한다.
/// 그 비대칭이 위 여덟의 대가를 만든 원인이고, 통합 여부는 별개 축이다.
#[cfg(not(feature = "gui"))]
fn is_unrouted_here(resp: &crate::ipc::protocol::JsonRpcResponse) -> bool {
    resp.error
        .as_ref()
        .is_some_and(|e| e.code == -32601 || e.code == -32017)
}

/// plugin namespace forward — 넘겼으면 `true`(응답은 plugin 이 준다).
///
/// plugin manager 를 여기서 lazy 로 띄운다. 헤드리스 데몬은 attach 세션이 없으면
/// plugin 을 하나도 안 띄우는 것이 기본값이고(그래서 `ensure_plugin_manager` 의
/// 유일한 호출자가 attach 경로였다), 그 가벼움을 유지하면서 plugin 메서드에는
/// 답하려면 **처음 그런 호출이 왔을 때** 띄우는 수밖에 없다 — namespace 표가
/// spawn 시점에 채워져서 부르기 전에는 소속을 알 수 없기 때문이다.
///
/// 대가는 명시해 둔다: 호스트가 모르는 메서드를 **한 번이라도** 부르면(오타 포함)
/// 그 데몬은 그 시점에 plugin 을 기동한다. 기동은 프로세스 수명당 1 회고,
/// 기동 후에도 namespace 가 안 맞으면 원래의 `-32601` 이 그대로 나간다.
#[cfg(not(feature = "gui"))]
fn forward_to_plugin_namespace(
    app: &mut App,
    engine: &CoreState,
    cmd: &crate::ipc::server::IpcCommand,
) -> bool {
    super::headless_plugins::ensure_plugin_manager(app, engine);
    let Some(mgr) = app.plugin_manager.as_mut() else {
        return false;
    };
    if mgr.ipc_namespaces.resolve(&cmd.request.method).is_none() {
        return false;
    }
    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
    mgr.forward_namespace_call(
        &cmd.request.method,
        cmd.request.params.clone(),
        None, // CLI/사용자 호출 — plugin → plugin 호출은 별도 경로(gui 와 같다).
        id,
        cmd.response_tx.clone(),
    );
    true
}
