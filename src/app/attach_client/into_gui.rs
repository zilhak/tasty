//! GUI의 `attach.into_gui`. 다른 포트를 거쳐 자기 자신에 닿는 대상(사용자가 연 SSH 터널 등)을
//! 큐에 넣기 전에 거절한다.
//!
//! 판정은 대상 포트의 `system.info`가 보고한 `instance_id`로 한다. 이 프로세스의 `system.info`는
//! 메인 루프가 답하므로 메인 루프에서 물으면 자기 자신일 때 응답을 기다리며 멈춘다. 그래서 워커가
//! 묻고, 결과를 받은 메인 루프가 거절하거나 큐에 넣은 뒤 응답한다.

use std::sync::mpsc::{Receiver, Sender, SyncSender};

use crate::adapters::ipc::handler::attach;
use crate::app::App;
use crate::app::event::AppEvent;
use crate::ipc::server::{IpcCommand, send_response};
use tasty_ipc::protocol::JsonRpcResponse;

/// 응답을 돌려줄 곳. 외부 IPC와 플러그인 호출이 같은 판정을 거친다.
enum Reply {
    Ipc(SyncSender<JsonRpcResponse>),
    Plugin(Box<tasty_host_plugin::manager::PendingPluginCall>),
}

/// 워커가 판정을 마친 요청.
pub(crate) struct IntoGuiCheck {
    reply: Reply,
    rpc_id: serde_json::Value,
    method: String,
    params: serde_json::Value,
    port: u16,
    workspace: u32,
    verdict: Verdict,
}

/// 워커의 판정 결과.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// 다른 상대이거나 판정할 수 없다. 큐에 넣는다.
    Other,
    /// 이 프로세스 자신이다. 거절한다.
    ThisInstance,
    /// 판정 중 패닉했다. 응답 없이 끝나지 않도록 내부 오류로 답한다.
    Panicked,
}

/// 패닉도 판정 결과로 바꿔 응답이 반드시 나가게 한다.
fn judge(probe: impl FnOnce() -> bool + std::panic::UnwindSafe) -> Verdict {
    match std::panic::catch_unwind(probe) {
        Ok(true) => Verdict::ThisInstance,
        Ok(false) => Verdict::Other,
        Err(_) => {
            tracing::error!("attach.into_gui self check panicked");
            Verdict::Panicked
        }
    }
}

pub(crate) struct IntoGuiChecks {
    tx: Sender<IntoGuiCheck>,
    rx: Receiver<IntoGuiCheck>,
}

impl Default for IntoGuiChecks {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { tx, rx }
    }
}

/// 대상 포트의 상대가 이 프로세스인지. 상대가 tasty가 아니거나 `instance_id`를 보고하지 않거나
/// 요청이 실패하면 판정하지 않고 false로 둔다. 연결 실패는 기존처럼 큐 처리 단계에서 드러난다.
fn reaches_this_instance(port: u16) -> bool {
    match tasty_remote::self_instance::refuse_this_instance(true, port, None) {
        Ok(()) => false,
        Err(error) => {
            let this = error
                .downcast_ref::<tasty_remote::self_instance::ThisInstance>()
                .is_some();
            if !this {
                tracing::debug!(%error, port, "attach.into_gui target did not answer system.info");
            }
            this
        }
    }
}

/// 다른 포트를 거쳐 자기 자신에 닿은 대상의 거절 응답. 자기 포트 거절과 같은 코드다.
fn this_instance_refused(id: serde_json::Value, port: u16) -> JsonRpcResponse {
    JsonRpcResponse::invalid_params(
        id,
        format!(
            "attach target port {port} reaches this instance itself; attaching to itself is refused"
        ),
    )
}

impl App {
    /// 외부 IPC의 `attach.into_gui`. 인자와 자기 포트는 바로 판정하고, 다른 포트는 워커에서
    /// 상대를 확인한 뒤 응답한다.
    pub(in crate::app) fn ipc_dispatch_attach_into_gui(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        self.start_into_gui_check(
            Reply::Ipc(cmd.response_tx.clone()),
            rpc_id,
            &cmd.request.method,
            &cmd.request.params,
        );
    }

    /// 플러그인 호출의 `attach.into_gui`. 게이트를 통과한 뒤 외부 IPC와 같은 판정을 거친다.
    pub(crate) fn plugin_dispatch_attach_into_gui(
        &mut self,
        call: &tasty_host_plugin::manager::PendingPluginCall,
    ) {
        self.start_into_gui_check(
            Reply::Plugin(Box::new(call.clone())),
            serde_json::Value::from(call.call_id),
            &call.method,
            &call.params,
        );
    }

    fn start_into_gui_check(
        &mut self,
        reply: Reply,
        rpc_id: serde_json::Value,
        method: &str,
        params: &serde_json::Value,
    ) {
        let (port, workspace) =
            match attach::into_gui_target(self.services.own_ipc_port(), &rpc_id, params) {
                Ok(target) => target,
                Err(response) => {
                    self.deliver_into_gui(reply, response);
                    return;
                }
            };
        let tx = self.state.into_gui_checks.tx.clone();
        let proxy = self.view.proxy.clone();
        let method = method.to_owned();
        let params = params.clone();
        let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
        let worker_id = rpc_id.clone();
        let spawned = std::thread::Builder::new()
            .name("into-gui-check".into())
            .spawn(move || {
                let Ok(reply) = reply_rx.recv() else {
                    return;
                };
                let check = IntoGuiCheck {
                    reply,
                    rpc_id: worker_id,
                    method,
                    params,
                    port,
                    workspace,
                    verdict: judge(move || reaches_this_instance(port)),
                };
                if tx.send(check).is_err() {
                    tracing::debug!("attach.into_gui check finished after the main loop ended");
                    return;
                }
                if let Err(error) = proxy.send_event(AppEvent::AutoAttachReady) {
                    tracing::debug!(%error, "attach.into_gui check could not wake the main loop");
                }
            });
        match spawned {
            Ok(_) => {
                // 워커가 시작된 뒤에만 응답 경로를 넘긴다. 실패하면 아래에서 바로 답한다.
                if let Err(std::sync::mpsc::SendError(reply)) = reply_tx.send(reply) {
                    self.deliver_into_gui(
                        reply,
                        JsonRpcResponse::internal_error(rpc_id, "attach.into_gui check ended"),
                    );
                }
            }
            Err(error) => self.deliver_into_gui(
                reply,
                JsonRpcResponse::internal_error(
                    rpc_id,
                    format!("attach.into_gui check thread failed: {error}"),
                ),
            ),
        }
    }

    fn deliver_into_gui(&mut self, reply: Reply, response: JsonRpcResponse) {
        match reply {
            Reply::Ipc(tx) => send_response(&tx, response),
            Reply::Plugin(call) => {
                // 원래 오류 코드를 보존해야 인자 오류가 일반 서버 오류로 바뀌지 않는다.
                let (result, error, code) = match response.error {
                    Some(err) => (None, Some(err.message), Some(err.code)),
                    None => (response.result, None, None),
                };
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    mgr.send_plugin_call_result(&call, result, error, code);
                }
            }
        }
    }

    /// 판정을 마친 요청을 거절하거나, 요청을 받은 창(routing과 같은 규칙)의 큐에 넣고 응답한다.
    pub(crate) fn drain_into_gui_checks(&mut self) {
        while let Ok(check) = self.state.into_gui_checks.rx.try_recv() {
            let response = if check.verdict == Verdict::ThisInstance {
                this_instance_refused(check.rpc_id, check.port)
            } else if check.verdict == Verdict::Panicked {
                JsonRpcResponse::internal_error(check.rpc_id, "attach.into_gui self check panicked")
            } else if self.queue_into_gui(&check) {
                attach::into_gui_queued(check.rpc_id, check.port, check.workspace)
            } else {
                crate::app::services::no_application_state(check.rpc_id)
            };
            self.deliver_into_gui(check.reply, response);
        }
    }

    /// engine 처리기가 하던 것처럼 요청을 받은 창, 없으면 첫 parked engine의 큐에 넣는다.
    fn queue_into_gui(&mut self, check: &IntoGuiCheck) -> bool {
        let target = self
            .find_request_owner(&check.method, &check.params)
            .ok()
            .flatten()
            .or(self.view.focused_view_id);
        let request = (check.port, check.workspace);
        if let Some(id) = target
            && let Some((_, engine)) = self.engines_mut().window_pair(id)
        {
            engine.remote.pending_gui_attach.push(request);
            return true;
        }
        if let Some((_, engine)) = self.engines_mut().first_parked_session() {
            engine.remote.pending_gui_attach.push(request);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_reporting_this_process_is_judged_as_itself() {
        let (port, server) = crate::app::auto_attach::fake_peer::serve_once(
            serde_json::json!({ "instance_id": tasty_ipc::instance::instance_id() }),
        );
        assert!(reaches_this_instance(port));
        assert!(server.join().expect("server").contains("\"system.info\""));
    }

    #[test]
    fn a_panicking_check_still_ends_in_a_verdict() {
        assert_eq!(judge(|| panic!("probe")), Verdict::Panicked);
        assert_eq!(judge(|| true), Verdict::ThisInstance);
        assert_eq!(judge(|| false), Verdict::Other);
    }

    #[test]
    fn another_or_a_silent_target_is_queued_as_before() {
        let (port, server) = crate::app::auto_attach::fake_peer::serve_once(
            serde_json::json!({ "instance_id": "x" }),
        );
        assert!(!reaches_this_instance(port));
        server.join().expect("server");
        assert!(!reaches_this_instance(
            crate::app::auto_attach::fake_peer::free_port()
        ));
    }
}
