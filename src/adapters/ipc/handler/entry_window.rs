//! IPC 라우터가 요청을 처리하는 동안 사용할 창 연산을 제한한다(ADR-0002).
//! engine에는 좁은 포트를 넘기고, 생성된 intent는 창 큐로 옮긴다.
//! AppState 전체가 필요한 창·디버그 핸들러 호출은 이 타입 안에서만 수행한다.
//! AppState를 꺼내는 접근자는 제공하지 않는다.
//! 디버그 라우터는 비공개 필드에 접근해야 하므로 별도 자식 모듈에 두고 파일 전체에 cfg를 건다.

#[path = "entry_window_debug.rs"]
mod debug;

use crate::ipc::window_port::IpcWindow;
use crate::state::AppState;

#[cfg(feature = "gui")]
use crate::ipc::caller::CallerContext;
#[cfg(feature = "gui")]
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

pub(crate) struct EntryWindow<'a> {
    state: &'a mut AppState,
}

impl<'a> EntryWindow<'a> {
    pub(crate) fn new(state: &'a mut AppState) -> Self {
        Self { state }
    }

    pub(crate) fn port(&mut self) -> &mut dyn IpcWindow {
        &mut *self.state
    }

    /// 창 핸들러마다 호출자 권한을 확인해야 한다. 목록 검사는 window_router_caller_tests를 따른다.
    #[cfg(feature = "gui")]
    pub(crate) fn route_window(
        &mut self,
        engine: &mut crate::core::CoreState,
        caller: &CallerContext,
        request: &JsonRpcRequest,
        id: serde_json::Value,
    ) -> Option<JsonRpcResponse> {
        super::route_window_handler(self.state, engine, caller, request, id)
    }
}
