//! 요청 게이트의 저장 결과를 기다리는 요청.
//!
//! 메인 루프는 writer 결과를 기다리지 않고 회차마다 맨 앞 요청만 확인한다. 맨 앞 요청이 끝나기 전에는
//! 뒤 요청을 시작하지 않으므로 IPC 명령과 plugin 호출이 접수 순서대로 실행된다. 앞 요청이 바꾼 상태를
//! 뒤 요청의 게이트가 본다(예: 세션 철회 뒤의 같은 토큰 요청).

use std::collections::VecDeque;

use tasty_host_plugin::manager::PendingPluginCall;

use crate::app::services::AppServices;
use crate::ipc::handler::gate::GateProgress;
use crate::ipc::protocol::JsonRpcRequest;
use crate::ipc::server::IpcCommand;

/// 게이트를 진행 중인 요청과 그 진행 상태.
pub(crate) enum Gated {
    Ipc {
        cmd: IpcCommand,
        progress: GateProgress,
    },
    Plugin {
        call: PendingPluginCall,
        request: JsonRpcRequest,
        progress: GateProgress,
    },
}

impl Gated {
    fn progress_mut(&mut self) -> &mut GateProgress {
        match self {
            Self::Ipc { progress, .. } | Self::Plugin { progress, .. } => progress,
        }
    }
}

#[derive(Default)]
pub(crate) struct GateQueue {
    waiting: VecDeque<Gated>,
}

impl GateQueue {
    pub(crate) fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    /// 새 요청은 뒤에 둔다. 맨 앞에서 꺼내 진행하다 다시 기다리게 된 요청은 앞자리로 돌려놓는다.
    pub(crate) fn park(&mut self, gated: Gated, resumed: bool) {
        if resumed {
            self.waiting.push_front(gated);
        } else {
            self.waiting.push_back(gated);
        }
    }

    /// 맨 앞 요청이 기다리던 결과가 나왔거나 아직 시작하지 않은 요청이면 꺼낸다.
    pub(crate) fn pop_ready(&mut self, core: &AppServices) -> Option<Gated> {
        let front = self.waiting.front_mut()?;
        if !front.progress_mut().poll(core) {
            return None;
        }
        self.waiting.pop_front()
    }

    /// 종료가 시작되면 기다리던 요청에 종료 응답을 보내고 실행하지 않는다. 정리 중인 engine을
    /// 건드리지 않기 위해서다. 이미 넣은 게이트 요청(세션 정리·rate limit 소비)은 확정될 수 있다.
    pub(crate) fn reject_all(
        &mut self,
        mut plugins: Option<&mut crate::plugin::PluginManager>,
    ) -> usize {
        let rejected = self.waiting.len();
        for gated in self.waiting.drain(..) {
            match gated {
                Gated::Ipc { cmd, .. } => {
                    let id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
                    crate::ipc::server::send_response(
                        &cmd.response_tx,
                        crate::ipc::handler::gate::shutting_down(id),
                    );
                }
                Gated::Plugin { call, .. } => {
                    if let Some(manager) = plugins.as_deref_mut() {
                        manager.send_plugin_call_result(
                            &call,
                            None,
                            Some("host is shutting down".to_string()),
                            Some(-32000),
                        );
                    }
                }
            }
        }
        rejected
    }
}
