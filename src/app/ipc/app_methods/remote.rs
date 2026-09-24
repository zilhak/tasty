//! 원격 workspace 조회·attach의 블로킹 연결 준비를 워커에서 수행한다.

use crate::AppEvent;
use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    pub(super) fn ipc_dispatch_remote_workspaces(&mut self, cmd: &IpcCommand) {
        crate::core::app_surface::spawn_remote_workspaces(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            &cmd.request.params,
            &cmd.response_tx,
        );
    }

    /// mirror를 만들되 로컬·원격 포커스는 옮기지 않는다.
    /// 기존 workspace는 attaching으로 즉시 응답한다. 새 workspace는 생성 결과 ID를 응답한 뒤
    /// mirror 연결을 이어 가므로 두 경우 모두 응답이 mirror 연결 완료를 뜻하지 않는다.
    pub(super) fn ipc_dispatch_remote_attach(&mut self, cmd: &IpcCommand) {
        let rpc_id = cmd.request.id.clone().unwrap_or(serde_json::Value::Null);
        let conn = match RemoteConnParams::parse(&cmd.request.params) {
            Ok(c) => c,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, msg),
                );
                return;
            }
        };
        let target = match RemoteAttachTarget::parse(&cmd.request.params) {
            Ok(t) => t,
            Err(msg) => {
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::invalid_params(rpc_id, msg),
                );
                return;
            }
        };

        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        match target {
            RemoteAttachTarget::Existing(remote_ws) => {
                std::thread::spawn(move || {
                    let result = conn.resolve_endpoint();
                    send_attach_outcome(&tx, &proxy, remote_ws, result);
                });
                send_response(
                    &cmd.response_tx,
                    host_ipc::protocol::JsonRpcResponse::success(
                        rpc_id,
                        serde_json::json!({ "attaching": true, "remote_workspace": remote_ws }),
                    ),
                );
            }
            RemoteAttachTarget::Create { name, cwd } => {
                let response_tx = cmd.response_tx.clone();
                std::thread::spawn(move || {
                    remote_attach_create_worker(conn, name, cwd, rpc_id, &response_tx, &tx, &proxy);
                });
            }
        }
    }
}

use crate::core::app_surface::RemoteConnParams;

impl RemoteConnParams {
    /// SSH 접속 준비는 블로킹하므로 워커에서 호출한다.
    fn resolve_endpoint(&self) -> anyhow::Result<(Option<tasty_ssh::SshTunnel>, u16)> {
        let (target, rt, pm, pf) = tasty_remote::browse::resolve_connection_spec(
            self.profile.as_deref(),
            self.ssh.as_deref(),
            &self.remote_tasty,
            &self.remote_port_mode,
        )?;
        tasty_remote::browse::resolve_endpoint(&target, &rt, &pm, pf.as_deref())
    }
}

enum RemoteAttachTarget {
    Existing(u32),
    Create {
        name: Option<String>,
        cwd: Option<String>,
    },
}

impl RemoteAttachTarget {
    fn parse(params: &serde_json::Value) -> Result<Self, String> {
        // 범위 밖 숫자를 잘라 다른 workspace ID로 바꾸지 않는다.
        let remote_ws = params::read_int::<u32>(params, "remote_workspace")?;
        let new_workspace = params
            .get("new_workspace")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let cwd = params
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        match (remote_ws, new_workspace) {
            (Some(_), true) => {
                Err("'remote_workspace' and 'new_workspace' are mutually exclusive".to_string())
            }
            (None, false) => Err(
                "one of 'remote_workspace' (u32) or 'new_workspace' (bool) is required".to_string(),
            ),
            (Some(id), false) => {
                if name.is_some() || cwd.is_some() {
                    return Err(
                        "'name'/'cwd' require 'new_workspace': true (they describe the workspace to create)"
                            .to_string(),
                    );
                }
                Ok(Self::Existing(id))
            }
            (None, true) => Ok(Self::Create { name, cwd }),
        }
    }
}

/// 결과 채널에 터널을 넘기고 메인 루프를 깨운다. mirror 생성은 이후에 수행한다.
fn send_attach_outcome(
    tx: &std::sync::mpsc::Sender<crate::app::auto_attach::AutoAttachOutcome>,
    proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
    remote_ws: u32,
    result: anyhow::Result<(Option<tasty_ssh::SshTunnel>, u16)>,
) {
    let outcome = crate::app::auto_attach::AutoAttachOutcome {
        anchor_ws_id: None,
        remote_ws,
        result,
        is_reconnect: false,
    };
    let _ = tx.send(outcome); // 수신자(메인 루프) drop 시에만 실패 — 무시.
    let _ = proxy.send_event(AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
}

/// 새 workspace의 생성 결과를 응답하고 attach를 요청한다. 실패하면 attach 요청은 보내지 않는다.
fn remote_attach_create_worker(
    conn: RemoteConnParams,
    name: Option<String>,
    cwd: Option<String>,
    rpc_id: serde_json::Value,
    response_tx: &std::sync::mpsc::SyncSender<host_ipc::protocol::JsonRpcResponse>,
    tx: &std::sync::mpsc::Sender<crate::app::auto_attach::AutoAttachOutcome>,
    proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
) {
    let (tunnel, port) = match conn.resolve_endpoint() {
        Ok(v) => v,
        Err(e) => {
            send_response(
                response_tx,
                host_ipc::protocol::JsonRpcResponse::error(
                    rpc_id,
                    -32050,
                    // 중첩 오류의 접속 실패 원인도 응답에 포함한다.
                    format!("remote endpoint resolve failed: {e:#}"),
                ),
            );
            return;
        }
    };
    let created = match tasty_remote::create::create_via_port(port, name.as_deref(), cwd.as_deref())
    {
        Ok(c) => c,
        Err(e) => {
            send_response(
                response_tx,
                host_ipc::protocol::JsonRpcResponse::error(
                    rpc_id,
                    -32050,
                    format!("remote workspace.create failed: {e:#}"),
                ),
            );
            return;
        }
    };
    send_response(
        response_tx,
        host_ipc::protocol::JsonRpcResponse::success(
            rpc_id,
            serde_json::json!({
                "attaching": true,
                "created": true,
                "remote_workspace": created.id,
                "name": created.name,
                "index": created.index,
            }),
        ),
    );
    send_attach_outcome(tx, proxy, created.id, Ok((tunnel, port)));
}
