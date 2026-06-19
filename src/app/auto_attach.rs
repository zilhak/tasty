//! attach/detach 단계 7 — 매핑된 워크스페이스 자동 attach 결선.
//!
//! 사용자 핵심 요구("워크스페이스1 = a컴퓨터, 워크스페이스2 = b컴퓨터")의 종착점:
//! 매핑(`Workspace.attach_mapping`)이 있는 로컬 워크스페이스를 **활성화하면** 호스트가
//! 자동으로 ① 프로필 resolve → ② SSH 터널(`tasty_cli::ssh`) 수립 → ③ 그 `local_port`
//! 를 작업 J 의 [`App::start_gui_attach`] 에 넘겨 원격 워크스페이스를 GUI mirror 로
//! 띄운다. 터널 핸들은 client 세션에 보관돼 세션 수명 동안 살아있다(Drop 시 자식 ssh
//! kill — 고아 터널 방지).
//!
//! 결선 파이프라인(plan-v2 §6.2):
//! ```text
//! [about_to_wait] poll_auto_attach
//!   ├─ maybe_trigger_auto_attach: 활성 ws 매핑 Some & 미attach → 워커 스레드 spawn
//!   │     (워커: 프로필 resolve + SSH 터널 수립 = 최대 수초 블록 → 메인 무블록)
//!   │     완료 → 결과 채널 push + AppEvent::AutoAttachReady wake
//!   └─ drain_auto_attach_results: 채널 drain → start_gui_attach(port, remote_ws, tunnel)
//! ```
//!
//! - **loopback 직결**: 인라인 host 가 `127.0.0.1:PORT`/`localhost:PORT` 면 터널 없이
//!   그 포트로 직접 attach(로컬 검증·동일 머신 다중 인스턴스).
//! - **중복 방지**: anchor(매핑된 로컬 ws id)를 `auto_attach_active` 에 넣어 재트리거
//!   skip. 세션 정리(force-detach/EOF) 시 제거해 재활성 시 재attach 가능.
//! - **원칙 3**: `remote_workspace` 가 None 이면 자동 attach skip(ID 명시 필요).

use tasty_cli::ssh::{self, PortMode, SshTarget, SshTunnel};
use tasty_remote_profiles::{Passkeys, RemoteProfiles};

use crate::app::App;
use crate::model::WorkspaceAttachTarget;

/// 자동 attach 워커 스레드 → 메인 루프 결과. 터널 핸들/포트를 채널로 전달한다
/// (AppEvent 는 Debug 라 핸들을 싣지 못해 별도 채널 사용).
pub(crate) struct AutoAttachOutcome {
    /// 매핑된(anchor) 로컬 워크스페이스 id.
    pub(crate) anchor_ws_id: u32,
    /// 원격 tasty 의 attach 대상 workspace_id.
    pub(crate) remote_ws: u32,
    /// 엔드포인트 해석 결과: `(터널 핸들 or None, 접속 포트)`.
    pub(crate) result: anyhow::Result<(Option<SshTunnel>, u16)>,
}

impl App {
    /// `about_to_wait` 매 프레임 — 활성 워크스페이스 매핑을 보고 자동 attach 를
    /// 트리거하고, 완료된 워커 결과를 적용한다(둘 다 cheap — 후보 없으면 즉시 반환).
    pub(crate) fn poll_auto_attach(&mut self) {
        self.maybe_trigger_auto_attach();
        self.drain_auto_attach_results();
    }

    /// 활성 워크스페이스가 매핑 Some & 아직 attach 안 됐으면 워커 스레드로 SSH 터널
    /// 수립을 시작한다(메인 루프 무블록). 포커스 독립(원칙 3): 활성 상태를 *읽어*
    /// 트리거할 뿐, 동작은 ID(anchor/remote_ws)로 결정된다.
    fn maybe_trigger_auto_attach(&mut self) {
        // 활성 ws 의 (anchor id, 매핑)을 읽어 후보 수집(borrow 스코프 분리).
        let candidate = {
            let Some(main) = self.focused_window_mut() else {
                return;
            };
            let idx = main.state.active_workspace;
            match main.core_state.workspaces.get(idx) {
                Some(ws) => ws.attach_mapping.as_ref().map(|m| (ws.id, m.clone())),
                None => None,
            }
        };
        let Some((anchor, mapping)) = candidate else {
            return;
        };
        // 이미 진행 중/established → skip(중복 attach 방지).
        if self.auto_attach_active.contains(&anchor) {
            return;
        }
        // 원격 workspace id 미지정 → 자동 attach skip(원칙 3, plan §11-2).
        let Some(remote_ws) = mapping.remote_workspace else {
            return;
        };

        self.auto_attach_active.insert(anchor);
        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        let target = mapping.target.clone();
        // 워커: 프로필 resolve + 포트 발견 + ssh -L 터널(최대 ~수초 블록).
        std::thread::spawn(move || {
            let result = resolve_endpoint(&target);
            let _ = tx.send(AutoAttachOutcome { // 수신자가 이미 drop 됐을 수 있음(메인 루프 종료) — 무시
                anchor_ws_id: anchor,
                remote_ws,
                result,
            });
            // 메인 루프를 깨워 결과를 drain 시킨다(idle 상태에서도 즉시 반영).
            let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady);  // event loop 종료 시에만 실패 — 무시
        });
    }

    /// 워커가 보낸 엔드포인트 결과를 적용한다 — 성공이면 mirror 를 띄우고(터널 핸들
    /// 세션에 보관), 실패면 anchor 게이트를 풀어 재활성 시 재시도 가능하게 한다.
    pub(crate) fn drain_auto_attach_results(&mut self) {
        while let Ok(outcome) = self.auto_attach_rx.try_recv() {
            let AutoAttachOutcome {
                anchor_ws_id,
                remote_ws,
                result,
            } = outcome;
            match result {
                Ok((tunnel, port)) => {
                    if let Err(e) =
                        self.start_gui_attach(port, remote_ws, tunnel, Some(anchor_ws_id))
                    {
                        tracing::warn!(
                            "auto-attach mirror 실패 (anchor ws {anchor_ws_id}, remote ws {remote_ws}): {e}"
                        );
                        self.auto_attach_active.remove(&anchor_ws_id);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "auto-attach 엔드포인트 해석 실패 (anchor ws {anchor_ws_id}): {e}"
                    );
                    self.auto_attach_active.remove(&anchor_ws_id);
                }
            }
        }
    }
}

/// 매핑 타깃 → 접속 엔드포인트(터널 or loopback 포트). 워커 스레드에서 실행(블록 OK).
fn resolve_endpoint(target: &WorkspaceAttachTarget) -> anyhow::Result<(Option<SshTunnel>, u16)> {
    // ① 접속 스펙 결정(destination + remote_tasty + port_mode).
    let (ssh_target, remote_tasty, port_mode) = match target {
        WorkspaceAttachTarget::Profile { name } => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let p = profiles
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("원격 프로필 '{name}' 을 찾을 수 없습니다"))?;
            // ssh kind 검증 + 비활성 게이트 + passkey resolve 를 한곳에서.
            ssh::resolve_attach_target(p, &passkeys)?
        }
        WorkspaceAttachTarget::Inline {
            host,
            remote_tasty,
            port_mode,
        } => (
            SshTarget::parse(host),
            remote_tasty.clone().unwrap_or_else(|| "tasty".into()),
            port_mode.clone().unwrap_or_else(|| "auto".into()),
        ),
    };

    // ② loopback 직결(터널 없이): host 가 127.0.0.1:PORT / localhost:PORT 면 그 포트로.
    if let Some(port) = parse_loopback_port(&ssh_target.destination) {
        return Ok((None, port));
    }

    // ③ SSH 터널: 원격 포트 발견 → ssh -L → local_port.
    let ssh = ssh::resolve_ssh_path();
    let mode = PortMode::parse(&port_mode)?;
    // 자동 검증(Claude Bash) 한정 host key accept-new. 평상시 기본 strict 유지(보안).
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    let remote_port =
        ssh::discover_remote_port(&ssh, &ssh_target, &remote_tasty, mode, verify, debug)?;
    let tunnel = SshTunnel::establish(&ssh, &ssh_target, remote_port, verify)?;
    let local_port = tunnel.local_port;
    Ok((Some(tunnel), local_port))
}

/// `127.0.0.1:PORT` / `localhost:PORT` / `[::1]:PORT` 면 PORT 를 돌려준다(loopback 직결).
/// 그 외(원격 호스트/alias)는 None → SSH 터널 경로.
fn parse_loopback_port(dest: &str) -> Option<u16> {
    let (host, port_str) = if let Some(rest) = dest.strip_prefix("[::1]:") {
        ("::1", rest)
    } else {
        let (h, p) = dest.rsplit_once(':')?;
        (h, p)
    };
    let is_loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
    if !is_loopback {
        return None;
    }
    port_str.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_ports_parsed() {
        assert_eq!(parse_loopback_port("127.0.0.1:45123"), Some(45123));
        assert_eq!(parse_loopback_port("localhost:8080"), Some(8080));
        assert_eq!(parse_loopback_port("[::1]:9000"), Some(9000));
    }

    #[test]
    fn non_loopback_is_none() {
        assert_eq!(parse_loopback_port("gx10"), None);
        assert_eq!(parse_loopback_port("user@host"), None);
        assert_eq!(parse_loopback_port("192.168.0.10:45123"), None);
        assert_eq!(parse_loopback_port("example.com:22"), None);
    }
}
