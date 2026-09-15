//! 외부 IPC 봉투 인증. 권한·예산·관측은 공통 요청 게이트가 수행한다.
use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::caller::resolve_caller_from_envelope;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(crate) fn ipc_resolve_caller(
        &mut self,
        cmd: &IpcCommand,
    ) -> Option<host_ipc::caller::CallerContext> {
        match resolve_caller_from_envelope(&self.core, &cmd.request) {
            Ok(caller) => Some(caller),
            Err(response) => {
                send_response(&cmd.response_tx, response);
                None
            }
        }
    }
}
