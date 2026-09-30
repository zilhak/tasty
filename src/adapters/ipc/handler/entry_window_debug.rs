//! EntryWindow의 디버그 라우터. release에서는 이 파일을 제외한다.
#![cfg(debug_assertions)]

use crate::core::engine_access::EngineMut;
use crate::ipc::protocol::{JsonRpcRequest, JsonRpcResponse};

impl super::EntryWindow<'_> {
    pub(crate) fn route_debug(
        &mut self,
        engine: &mut EngineMut<'_>,
        request: &JsonRpcRequest,
        id: serde_json::Value,
    ) -> Option<JsonRpcResponse> {
        super::super::route_debug_handler(self.state, engine, request, id)
    }
}
