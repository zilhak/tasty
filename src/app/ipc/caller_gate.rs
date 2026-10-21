//! 외부 IPC 봉투 인증. 권한·예산·관측은 공통 요청 게이트가 수행한다.
use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::handler::gate::{GateProgress, Step};
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    /// 세션 토큰이 있으면 writer 판정이 끝날 때까지 `Pending`이다. 거절했으면 응답을 보내고 None이다.
    pub(crate) fn ipc_resolve_caller(
        &mut self,
        cmd: &IpcCommand,
        progress: &mut GateProgress,
    ) -> Step<Option<host_ipc::caller::CallerContext>> {
        match crate::ipc::handler::gate::resolve_caller(&self.services, &cmd.request, progress) {
            Step::Pending => Step::Pending,
            Step::Ready(Ok(caller)) => Step::Ready(Some(caller)),
            Step::Ready(Err(response)) => {
                send_response(&cmd.response_tx, response);
                Step::Ready(None)
            }
        }
    }
}
