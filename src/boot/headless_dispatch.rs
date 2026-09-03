//! Headless 빌드 전용 IPC dispatch.
//!
//! gui 의 `App::process_ipc` (`src/app/ipc.rs`, `#[cfg(feature="gui")]`) 는 view /
//! parked_states / plugin_manager 의존이 큰 5-step 라우터다. headless 는 engine 이
//! 단 하나뿐이라 그 전부가 불필요하므로, caller 해석 → `handle_with_caller` 직결로
//! 간소화한다.
//!
//! 생략(gui 대비, 단계 0 토대 범위):
//! - plugin namespace forward (plugin_manager 없음)
//! - caller elevation / audit-on-deny (view 의존; deny 자체는 handle_with_caller 가 회신)
//! - app_methods / window_required / debug step (창/스크린샷/system.shutdown 등).
//!   예외는 `timer.list` 하나 — 읽는 대상(TimerHub)이 `App` 에 있어 engine handler
//!   로는 답할 수 없고, 관측이 gui 에서만 되면 headless 인스턴스의 wakeup 원인을
//!   물어볼 방법이 사라진다.
//! - dispatch_list_global / find_request_owner / parked fallback (engine 1 개)

#![cfg(not(feature = "gui"))]

use crate::app::App;
use crate::core::CoreState;
use crate::ipc::caller::resolve_caller_from_envelope;
use crate::ipc::server::send_response;
use crate::state::AppState;

/// IPC 큐를 비차단으로 비우고 각 명령을 단일 engine 으로 dispatch 한다.
/// `IpcReady` 수신 시 headless 메인 루프가 호출.
pub(crate) fn pump_ipc(app: &mut App, state: &mut AppState, engine: &mut CoreState) {
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
        send_response(&cmd.response_tx, resp);
    }
}
