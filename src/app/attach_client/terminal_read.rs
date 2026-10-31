//! 원본 터미널 읽기의 송신과 마크 적용. 대기 상태는 [`crate::remote::terminal_read::client`] 에 있다.

use crate::app::App;
use crate::remote::terminal_read::client::{self as reads, SessionView};
use tasty_ipc::terminal_read::TerminalReadMsg;
use tasty_remote::client_session::SessionState;

impl App {
    /// 세션 상태를 알리고 대기 요청을 보낸 뒤, 위치를 받은 마크 요청에 로컬 마크를 적용한다.
    pub(crate) fn pump_terminal_reads(&mut self) {
        let sends = {
            let views: Vec<SessionView<'_>> = self
                .remote
                .sessions
                .iter()
                .map(|s| SessionView {
                    workspace: s.state.local_workspace,
                    capable: s.transport.terminal_read,
                    connected: s.state.phase == SessionState::Connected,
                    epoch: s.transport.frame_tx.epoch(),
                    remote_to_local: &s.state.remote_to_local,
                })
                .collect();
            reads::client().sync(&views)
        };
        for (workspace, msg) in sends {
            let TerminalReadMsg::TerminalReadRequest { request_id, .. } = &msg else {
                continue;
            };
            let request_id = *request_id;
            let sent = self
                .remote
                .sessions
                .iter()
                .find(|s| s.state.local_workspace == workspace)
                .is_some_and(|sess| match serde_json::to_value(&msg) {
                    Ok(value) => super::wire::send_control_frame(&sess.transport.frame_tx, &value)
                        .map_err(|e| tracing::warn!("attach terminal read not sent: {e}"))
                        .is_ok(),
                    Err(e) => {
                        tracing::warn!("attach terminal read did not serialize: {e}");
                        false
                    }
                });
            if !sent {
                reads::client().send_failed(request_id);
            }
        }
        let marks = reads::client().marks_to_apply();
        for (request_id, local) in marks {
            let mut applied = false;
            for mut engine in self.engines_mut().windowed_and_parked() {
                if let Some(t) = engine.find_terminal_by_id_mut(local) {
                    t.set_mark();
                    applied = true;
                    break;
                }
            }
            reads::client().mark_applied(request_id, applied);
        }
    }
}
