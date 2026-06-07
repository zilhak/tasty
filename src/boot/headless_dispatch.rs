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
//! - app_methods / window_required / debug step (창/스크린샷/system.shutdown 등)
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
        // 2) engine handler 직결. 권한 게이트 / audit / rate-limit / cap 은
        //    handle_with_caller 내부가 자체 수행한다.
        let resp = crate::ipc::handler::handle_with_caller(
            &mut app.core,
            state,
            engine,
            &cmd.request,
            &caller,
        );
        send_response(&cmd.response_tx, resp);
    }
}
