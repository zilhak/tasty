//! `remote.workspaces` · `remote.attach` — 원격 인스턴스를 둘러보고 붙는 두 메서드.
//!
//! 이 둘만 따로 사는 이유는 **블로킹이라 워커로 나간다**는 것이다. 접속 스펙 resolve 와
//! SSH 터널 수립은 프레임 루프에서 못 돌리므로, 여기 있는 것의 절반은 워커에 넘길 입력을
//! 짜고(`RemoteAttachTarget::parse`) 워커가 낸 결과를 이벤트로 돌려보내는 코드다
//! (`send_attach_outcome`). 형제 메서드들은 그 자리에서 답하고 끝난다.
//!
//! 여기에는 `request.method` 로 갈래를 치는 코드가 없다 — 라우팅은 부모의
//! `ipc_step_app_methods` 가 끝내고, 이 파일은 고른 뒤의 일만 한다.

use crate::AppEvent;
use crate::adapters::ipc::handler::params;
use crate::app::App;
use crate::ipc as host_ipc;
use crate::ipc::server::{IpcCommand, send_response};

impl App {
    /// `remote.workspaces` — 본체는 두 조합이 공유한다
    /// (`crate::core::app_surface`). App 상태를 읽지 않는다.
    pub(super) fn ipc_dispatch_remote_workspaces(&mut self, cmd: &IpcCommand) {
        crate::core::app_surface::spawn_remote_workspaces(
            cmd.request.id.clone().unwrap_or(serde_json::Value::Null),
            &cmd.request.params,
            &cmd.response_tx,
        );
    }

    /// `remote.attach` { remote_workspace | new_workspace , profile? , ssh? ,
    /// remote_tasty? , remote_port_mode? , name? , cwd? } → 원격 워크스페이스를
    /// **로컬 mirror 로 attach**. `new_workspace: true` 면 원격에 워크스페이스를 **먼저
    /// 만들고** 그것을 attach 한다(생성+attach 복합 능력의 IPC 면 — CLI 면은
    /// `tasty remote new-workspace` + `tasty remote attach --workspace <id>`, 양쪽이
    /// `tasty_remote::create` 코어를 공유한다. 원칙 2).
    ///
    /// **focus 중립(원칙 1 핵심)**: 이 IPC/에이전트 경로는 mirror workspace 를 *생성만*
    /// 하고 focus 를 그 ws 로 옮기지 않는다. mirror 생성 실체(`start_gui_attach`)는
    /// `engine.workspaces.push` 만 하고 `active_workspace` 를 건드리지 않는다(조용한 생성).
    /// 새 mirror 로의 focus 이동은 **사용자 입력 경로 전용 별도 단계**다(원격 워크스페이스 추가 팝업에서
    /// 사용자가 확정할 때) — release IPC 에는 focus 변경 API 가 없다(원칙 3).
    /// 원격측 active 도 바뀌지 않는다(`workspace.create` 는 Agent origin).
    ///
    /// 블로킹 SSH 터널 수립은 워커 스레드에서 한다(auto-attach 와 동일한 결과 채널 재사용,
    /// anchor=None).
    ///
    /// **회신 계약이 두 갈래인 이유**:
    /// - 기존 ws attach: 즉시 `{attaching:true}`(fire-and-forget). 호출자가 이미 대상 id
    ///   를 알고 있으므로 회신에 새 정보가 없고, mirror 는 비동기로 나타난다.
    /// - `new_workspace`: **생성 완료까지 기다렸다 지연 회신**해 `remote_workspace`(새 id)
    ///   를 돌려준다. 즉시 회신으로는 ① 호출자가 만들어진 id 를 알 길이 없고 ② 생성 실패
    ///   (예: 없는 `cwd`)를 통보할 방법이 없기 때문이다. `remote.workspaces` 가 이미 워커
    ///   완료 후 지연 회신하는 선례다. 지연 구간은 SSH 터널 수립 + IPC 1회 뿐이고
    ///   attach 자체(mirror 구성)는 기다리지 않는다.
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
                // 즉시 회신(fire-and-forget). mirror 는 비동기 생성, focus 는 이동하지 않음.
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
    /// 접속 스펙 resolve → 엔드포인트(SSH 터널/loopback). **블로킹** — 워커 전용.
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

/// `remote.attach` 의 attach 대상 — 기존 원격 ws 지정 vs 원격에 새로 생성.
enum RemoteAttachTarget {
    Existing(u32),
    Create {
        name: Option<String>,
        cwd: Option<String>,
    },
}

impl RemoteAttachTarget {
    fn parse(params: &serde_json::Value) -> Result<Self, String> {
        // `as u32` 로 자르면 범위 밖 값이 **실재하는 다른 워크스페이스**를 가리킨다.
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
                // 생성 전용 옵션을 조용히 무시하면 "이름을 줬는데 안 붙었다" 로 보인다.
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

/// 해석된 엔드포인트를 auto-attach 결과 채널로 넘기고 메인 루프를 깨운다.
/// 메인 루프가 drain 해 focus 중립 `start_gui_attach` 를 수행한다.
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

/// `new_workspace` 워커 — 엔드포인트 해석 → 원격 `workspace.create` → 지연 회신 →
/// 생성된 id 로 attach outcome push.
///
/// 실패하면 outcome 을 보내지 않고 에러로 회신한다(붙을 대상이 없으므로). 그 경우
/// 터널 핸들은 여기서 Drop 되어 자식 ssh 가 회수된다(고아 터널 방지).
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
                    // `{e:#}` — anyhow 컨텍스트 체인을 한 줄로 편다. 최상위만 실으면
                    // 정작 원인(포트 발견 실패/터널 거부)이 호출자에게 안 보인다.
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
            // 원격이 `cwd does not exist: …` 같은 invalid_params 를 돌려준 경우도 여기로
            // 온다 — 원문 메시지를 그대로 실어 호출자가 원인을 알 수 있게 한다.
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
