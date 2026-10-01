//! 매핑된 워크스페이스의 자동 attach와 재연결을 처리한다.
//! SSH 연결 준비는 워커가 맡고 메인 루프가 결과를 mirror 세션에 적용한다.
//! 재연결은 예약 시각 또는 해당 워크스페이스 재활성화로 시도한다.
//! 자동 재시도 상한에 도달해도 사용자가 다시 활성화하면 재시도할 수 있다.
//! docs/dev-guide/attach-behavior.md#gui-자동-재연결-스코프 참조.

use std::time::Instant;

use tasty_remote_profiles::{Passkeys, RemoteProfiles};
use tasty_ssh::{self as ssh, Backoff, PortMode, SshTarget, SshTunnel};

use tasty_remote::client_session::SessionState;
use tasty_remote::outbound::{ReconnectSlot,AutoAttachOutcome};
use crate::app::App;
use crate::model::WorkspaceAttachTarget;
use crate::view::ui::View as _;

/// 자동 재연결의 실패 횟수 상한. 워크스페이스 재활성화로 재시도하는 동작은 막지 않는다.
const MAX_RECONNECT_ATTEMPTS: u32 = 20;



fn reconnect_due(slot: Option<&ReconnectSlot>, now: Instant) -> bool {
    match slot {
        Some(slot) if slot.given_up => false,
        Some(slot) => now >= slot.next_attempt,
        None => true,
    }
}

/// 다음 재연결을 예약할 시각. None이어도 중단 상태를 가진 슬롯은 지우지 않는다.
/// 슬롯이 없는 첫 시도는 현재 프레임에서 처리한다.
fn reconnect_wakeup_at(slot: Option<&ReconnectSlot>) -> Option<Instant> {
    match slot {
        Some(slot) if slot.given_up => None,
        Some(slot) => Some(slot.next_attempt),
        None => None,
    }
}

/// 이번 실패까지 포함한 횟수를 받는다.
fn reconnect_exhausted(attempts: u32) -> bool {
    attempts >= MAX_RECONNECT_ATTEMPTS
}

/// 정상 진행 중인 백오프는 워크스페이스 전환만으로 초기화하지 않는다.
fn should_reset_given_up(slot: Option<&ReconnectSlot>, edge_now: bool) -> bool {
    edge_now && slot.is_some_and(|s| s.given_up)
}


impl App {
    /// 활성 워크스페이스의 전환을 한 번 계산해 두 트리거가 같은 전환을 보게 한다.
    pub(crate) fn poll_auto_attach(&mut self) {
        let prev_active = self.remote.last_active_ws;
        let current_ws_id = self
            .focused_pair()
            .and_then(|(main, engine)| {
                engine
                    .core
                    .workspace_at(main.state.active_workspace_index(engine.core))
            })
            .map(|ws| ws.id);
        self.remote.last_active_ws = current_ws_id;

        self.maybe_trigger_auto_attach(current_ws_id, prev_active);
        self.maybe_trigger_reconnect(current_ws_id, prev_active);
        self.drain_auto_attach_results();
    }

    /// 타이머만 등록·해제한다. 중단 상태를 담은 재연결 슬롯은 보존한다.
    pub(crate) fn sync_reconnect_timers(&mut self, now: Instant) {
        let wakeups: Vec<(u32, Instant)> = self
            .remote.sessions
            .iter()
            .filter(|s| s.state() == SessionState::Reconnecting)
            .filter_map(|s| s.anchor_ws_id())
            // 워커가 이미 떠 있는 anchor 는 결과 이벤트(`AutoAttachReady`)가 깨운다.
            .filter(|anchor| !self.remote.active.contains(anchor))
            .filter_map(|anchor| {
                reconnect_wakeup_at(self.remote.reconnect.get(&anchor)).map(|at| (anchor, at))
            })
            .collect();
        crate::app::timers::sync_reconnect_timers(&mut self.timers, &wakeups, now);
    }

    /// 신규 매핑은 활성 상태면 연결하고, 연결 해제 뒤 대기 중인 anchor는 재활성화를 요구한다.
    /// 대상 ID는 매핑에서 가져오며 포커스를 바꾸지 않는다.
    fn maybe_trigger_auto_attach(&mut self, current_ws_id: Option<u32>, prev_active: Option<u32>) {
        let candidate = {
            let Some((main, engine)) = self.focused_pair() else {
                return;
            };
            let idx = main.state.active_workspace_index(engine.core);
            match engine.workspace_at(idx) {
                Some(ws) => ws.attach_mapping.as_ref().map(|m| (ws.id, m.clone())),
                None => None,
            }
        };
        let Some((anchor, mapping)) = candidate else {
            return;
        };
        // 재연결이 맡은 anchor에 새 mirror를 중복 생성하지 않는다.
        if self
            .remote.sessions
            .iter()
            .any(|s| s.anchor_ws_id() == Some(anchor) && s.state() == SessionState::Reconnecting)
        {
            return;
        }
        let pending_reactivation = self.remote.pending_reactivation.contains(&anchor);
        if !is_attach_trigger_allowed(pending_reactivation, current_ws_id, prev_active) {
            return;
        }
        if self.remote.active.contains(&anchor) {
            return;
        }
        let Some(remote_ws) = mapping.remote_workspace else {
            return;
        };

        let attempt=match self.remote.begin_attempt(Some(anchor),Some(mapping.clone())) {
            Ok(attempt)=>attempt,Err(error)=>{tracing::warn!("{error}");return;},
        };
        let endpoint_target=match self.mirror_install_target(None,Some(anchor),None,false) {
            Ok(target)=>target,Err(error)=>{self.remote.finish_attempt(&attempt);tracing::warn!("remote target unavailable: {error}");return;},
        };
        self.state.pending_remote_endpoints.insert(attempt.clone(),endpoint_target);
        self.remote.active.insert(anchor);
        self.remote.pending_reactivation.remove(&anchor);
        let tx = self.remote.tx.clone();
        let proxy = self.view.proxy.clone();
        let target = mapping.target.clone();
        // SSH 연결 준비가 메인 루프를 막지 않게 한다.
        let spawned=self.remote.spawn_attempt(attempt.clone(),move || {
            let result = resolve_endpoint_bound(&target, &attempt);
            let outcome = AutoAttachOutcome {
                attempt,
                anchor_ws_id: Some(anchor),
                remote_ws,
                result,
                is_reconnect: false,
            };
            let _ = tx.send(outcome); // 수신자(메인 루프) drop 시 send 실패 — 무시.
            let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
        });
        if let Err(error)=spawned {self.state.pending_remote_endpoints.retain(|token,_|token.is_active());self.remote.active.remove(&anchor);tracing::warn!("remote endpoint start rejected: {error}");}
    }

    /// 시도 중이 아닌 anchor를 예약 시각 또는 해당 워크스페이스 재활성화 때 재연결한다.
    fn maybe_trigger_reconnect(&mut self, current_ws_id: Option<u32>, prev_active: Option<u32>) {
        let anchors: Vec<u32> = self
            .remote.sessions
            .iter()
            .filter(|s| s.state() == SessionState::Reconnecting)
            .filter_map(|s| s.anchor_ws_id())
            .collect();
        for anchor in anchors {
            if self.remote.active.contains(&anchor) {
                continue;
            }
            let edge_now =
                current_ws_id == Some(anchor) && is_reactivation_edge(current_ws_id, prev_active);
            let existing_slot = self.remote.reconnect.get(&anchor);
            let due = reconnect_due(existing_slot, Instant::now());
            if !edge_now && !due {
                continue;
            }
            if should_reset_given_up(existing_slot, edge_now) {
                self.remote.reconnect.remove(&anchor);
            }
            // 대기 중 워크스페이스나 매핑이 사라졌을 수 있으므로 다시 읽는다.
            let mapping = self.engines().windows().find_map(|(_, e)| {
                e.workspaces()
                    .into_iter()
                    .find(|ws| ws.id == anchor)
                    .and_then(|ws| ws.attach_mapping.clone())
            });
            let Some(mapping) = mapping else {
                self.remote.reconnect.remove(&anchor);
                self.remote.pending_reactivation.remove(&anchor);
                continue;
            };
            let Some(remote_ws) = mapping.remote_workspace else {
                continue; // 원격 workspace id 미지정 — 자동 attach 와 동일 원칙 3.
            };

            let attempt=match self.remote.begin_attempt(Some(anchor),Some(mapping.clone())) {
                Ok(attempt)=>attempt,Err(error)=>{tracing::warn!("{error}");continue;},
            };
            let index=self.remote.sessions.iter().position(|session|session.state.anchor_ws_id==Some(anchor));
            let endpoint_target=match self.mirror_install_target(None,Some(anchor),index,false) {
                Ok(target)=>target,Err(error)=>{self.remote.finish_attempt(&attempt);tracing::warn!("remote reconnect target unavailable: {error}");continue;},
            };
            self.state.pending_remote_endpoints.insert(attempt.clone(),endpoint_target);
            self.remote.active.insert(anchor);
            self.remote.pending_reactivation.remove(&anchor);
            let tx = self.remote.tx.clone();
            let proxy = self.view.proxy.clone();
            let target = mapping.target.clone();
            let spawned=self.remote.spawn_attempt(attempt.clone(),move || {
                let result = resolve_endpoint_bound(&target, &attempt);
                let outcome = AutoAttachOutcome {
                    attempt,
                    anchor_ws_id: Some(anchor),
                    remote_ws,
                    result,
                    is_reconnect: true,
                };
                let _ = tx.send(outcome); // 수신자(메인 루프) drop 시 send 실패 — 무시.
                let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
            });
            if let Err(error)=spawned {self.state.pending_remote_endpoints.retain(|token,_|token.is_active());self.remote.active.remove(&anchor);tracing::warn!("remote reconnect start rejected: {error}");}
        }
    }

    pub(crate) fn drain_auto_attach_results(&mut self) {
        self.remote.reap_attempts();
        let stale:Vec<_>=self.state.pending_remote_endpoints.iter().filter(|(_,target)|!self.mirror_install_target_is_current(target)).map(|(token,target)|(token.clone(),target.anchor)).collect();
        for (token,anchor) in stale {
            self.state.pending_remote_endpoints.remove(&token);
            self.remote.cancel_attempt(&token);
            if let Some(anchor)=anchor {self.remote.active.remove(&anchor);}
        }
        self.state.pending_remote_endpoints.retain(|token,_|token.is_active());
        while let Ok(outcome) = self.remote.rx.try_recv() {
            self.apply_auto_attach_outcome(outcome);
        }
    }

    fn apply_auto_attach_outcome(&mut self, outcome: AutoAttachOutcome) {
        let Some(target)=self.state.pending_remote_endpoints.remove(&outcome.attempt) else {self.remote.finish_attempt(&outcome.attempt);self.remote.discard_endpoint_outcome(outcome);return;};
        if !self.mirror_install_target_is_current(&target) {if let Some(retired)=self.remote.finish_attempt(&outcome.attempt) && let Some(anchor)=retired.anchor {self.remote.active.remove(&anchor);}self.remote.discard_endpoint_outcome(outcome);return;}
        let Some(accepted)=self.remote.finish_attempt(&outcome.attempt) else {
            tracing::debug!("discarding result from retired remote connection attempt");self.remote.discard_endpoint_outcome(outcome);return;
        };
        if accepted.anchor!=outcome.anchor_ws_id {tracing::error!("remote outcome belongs to another anchor");self.remote.discard_endpoint_outcome(outcome);return;}
        if let Some(anchor)=accepted.anchor {
            let current=self.engines.all_sessions().find_map(|session|session.core_state.workspaces().iter().find(|workspace|workspace.id==anchor).and_then(|workspace|workspace.attach_mapping.clone()));
            if current!=accepted.mapping {
                self.remote.active.remove(&anchor);
                tracing::debug!("discarding SSH result after attach mapping changed");self.remote.discard_endpoint_outcome(outcome);return;
            }
        }
        let AutoAttachOutcome {
            attempt:_,
            anchor_ws_id,
            remote_ws,
            result,
            is_reconnect,
        } = outcome;
        match result {
            Ok((tunnel, port)) => {
                self.handle_auto_attach_connected(
                    target,
                    anchor_ws_id,
                    remote_ws,
                    is_reconnect,
                    tunnel,
                    port,
                );
            }
            Err(e) => {
                tracing::warn!(
                    "attach 엔드포인트 해석 실패 (anchor ws {anchor_ws_id:?}, remote ws {remote_ws}, reconnect={is_reconnect}): {e}"
                );
                if let Some(anchor) = anchor_ws_id {
                    self.remote.active.remove(&anchor);
                    if is_reconnect {
                        self.on_reconnect_attempt_failed(anchor, &e);
                    }
                }
            }
        }
    }

    fn handle_auto_attach_connected(
        &mut self,
        target:crate::app::attach_client::pending::PendingMirrorInstall,
        anchor_ws_id: Option<u32>,
        remote_ws: u32,
        is_reconnect: bool,
        tunnel: Option<SshTunnel>,
        port: u16,
    ) {
        // release에서는 연결을 시도하기 전에 자기 포트를 거절한다.
        #[cfg(not(debug_assertions))]
        if self.hub.ipc_server.as_ref().map(|s| s.port()) == Some(port) {
            tracing::warn!(
                "self(loopback) attach (port={port}) 는 release 빌드에서 차단됩니다 \
                 — 로컬 self-attach 는 debug 빌드 전용."
            );
            if let Some(anchor) = anchor_ws_id {
                self.remote.active.remove(&anchor);
            }
            self.remote.retire_tunnel(tunnel);
            return;
        }
        let attach_result=self.queue_mirror_connection(target,port,remote_ws,tunnel);
        match attach_result {
            // 접수는 성공 설치가 아니다. 실패 이력은 pending 설치 성공 때만 해제한다.
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(
                    "attach mirror 실패 (anchor ws {anchor_ws_id:?}, remote ws {remote_ws}, reconnect={is_reconnect}): {e}"
                );
                if let Some(anchor) = anchor_ws_id {
                    self.remote.active.remove(&anchor);
                    if is_reconnect {
                        self.on_reconnect_attempt_failed(anchor, &e);
                    }
                }
            }
        }
    }

    /// already_attached는 긴 고정 간격, 나머지 실패는 지수 백오프로 재시도한다.
    /// jitter로 동시 재시도 집중을 줄이며 상한에 도달하면 자동 재시도만 멈춘다.
    pub(super) fn on_reconnect_attempt_failed(&mut self, anchor: u32, err: &anyhow::Error) {
        let permanent_conflict = err.to_string().contains("already_attached");
        let slot = self
            .remote.reconnect
            .entry(anchor)
            .or_insert_with(ReconnectSlot::new);
        slot.attempts += 1;
        if reconnect_exhausted(slot.attempts) {
            slot.given_up = true;
            self.notify_reconnect_giveup(anchor);
            return;
        }
        if permanent_conflict {
            slot.backoff.reset();
            while slot.backoff.current() < std::time::Duration::from_secs(30) {
                slot.backoff.advance();
            }
        }
        let base = slot.backoff.current();
        slot.backoff.advance();
        use rand::Rng;
        let jitter = 0.8 + rand::rng().random::<f64>() * 0.4;
        slot.next_attempt = Instant::now() + base.mul_f64(jitter);
    }

    /// 자동 재시도 중단을 알린다. 워크스페이스 재활성화로 다시 시도할 상태는 유지한다.
    fn notify_reconnect_giveup(&mut self, anchor: u32) {
        for (_, main, engine) in self.engines_mut().window_pairs() {
            if engine.workspaces().into_iter().any(|ws| ws.id == anchor) {
                main.state.toasts.push(
                    crate::i18n::t("attach.toast.mirror_reconnect_giveup").to_string(),
                    crate::adapters::ui::ToastKind::Warning,
                    crate::adapters::ui::ToastScope::Window,
                );
                main.mark_dirty();
                break;
            }
        }
        tracing::warn!(
            "attach 재연결 포기 (anchor ws {anchor}) — {MAX_RECONNECT_ATTEMPTS}회 시도 후 자동 재시도 중단"
        );
    }
}

/// 워커 스레드에서 프로필과 SSH 터널 또는 loopback 포트를 준비한다.
fn resolve_endpoint_bound(target:&WorkspaceAttachTarget,attempt:&tasty_remote::outbound::AttemptToken)->anyhow::Result<(Option<SshTunnel>,u16)> {
    let cancel=tasty_ssh::SshCancel::new();
    attempt.register_ssh(cancel.clone()).map_err(anyhow::Error::msg)?;
    let _scope=cancel.scope();
    let result=resolve_endpoint(target)?;
    if !attempt.is_active() {anyhow::bail!("remote endpoint attempt cancelled");}
    Ok(result)
}

fn resolve_endpoint(target: &WorkspaceAttachTarget) -> anyhow::Result<(Option<SshTunnel>, u16)> {
    let (ssh_target, remote_tasty, port_mode, port_file) = match target {
        WorkspaceAttachTarget::Profile { name } => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let p = profiles.get(name).ok_or_else(|| {
                anyhow::anyhow!(
                    "{}",
                    crate::i18n::t_fmt("cli.remote_profile.not_found", name)
                )
            })?;
            ssh::resolve_attach_target(p, &profiles, &passkeys)?
        }
        WorkspaceAttachTarget::Inline {
            host,
            remote_tasty,
            port_mode,
            port_file,
        } => (
            SshTarget::parse(host),
            remote_tasty.clone().unwrap_or_else(|| "tasty".into()),
            port_mode.clone().unwrap_or_else(|| "auto".into()),
            port_file.clone(),
        ),
    };

    if let Some(port) = parse_loopback_port(&ssh_target.destination) {
        return Ok((None, port));
    }

    let ssh = ssh::resolve_ssh_path();
    let mode = PortMode::parse(&port_mode)?;
    // TASTY_SSH_VERIFY가 설정된 검증 환경에서는 accept-new를 사용한다. 기본은 strict다.
    let verify = std::env::var("TASTY_SSH_VERIFY").is_ok();
    let debug = cfg!(debug_assertions);
    let remote_port = ssh::discover_remote_port(
        &ssh,
        &ssh_target,
        &remote_tasty,
        mode,
        verify,
        debug,
        port_file.as_deref(),
    )?;
    let tunnel = SshTunnel::establish(&ssh, &ssh_target, remote_port, verify)?;
    let local_port = tunnel.local_port;
    Ok((Some(tunnel), local_port))
}

fn is_reactivation_edge(current: Option<u32>, previous: Option<u32>) -> bool {
    current.is_some() && current != previous
}

fn is_attach_trigger_allowed(
    pending_reactivation: bool,
    current: Option<u32>,
    previous: Option<u32>,
) -> bool {
    !pending_reactivation || is_reactivation_edge(current, previous)
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
    fn given_up_slot_schedules_no_wakeup_but_survives() {
        let mut slot = ReconnectSlot::new();
        slot.next_attempt = Instant::now() + std::time::Duration::from_secs(5);
        slot.given_up = true;
        assert_eq!(reconnect_wakeup_at(Some(&slot)), None);
        assert!(!reconnect_due(Some(&slot), Instant::now()));
    }

    #[test]
    fn active_backoff_schedules_its_next_attempt() {
        let at = Instant::now() + std::time::Duration::from_secs(5);
        let mut slot = ReconnectSlot::new();
        slot.next_attempt = at;
        assert_eq!(reconnect_wakeup_at(Some(&slot)), Some(at));
    }

    #[test]
    fn absent_slot_schedules_no_wakeup_because_it_is_already_due() {
        assert_eq!(reconnect_wakeup_at(None), None);
        assert!(reconnect_due(None, Instant::now()));
    }

    #[test]
    fn reactivation_edge_only_on_transition() {
        assert!(is_reactivation_edge(Some(1), None));
        assert!(!is_reactivation_edge(Some(1), Some(1)));
        assert!(is_reactivation_edge(Some(2), Some(1)));
        assert!(!is_reactivation_edge(None, Some(1)));
    }

    #[test]
    fn new_mapping_triggers_immediately_without_transition() {
        assert!(is_attach_trigger_allowed(false, Some(1), Some(1)));
        assert!(is_attach_trigger_allowed(false, Some(2), Some(1)));
    }

    #[test]
    fn disconnected_anchor_waits_for_transition_before_retrigger() {
        assert!(!is_attach_trigger_allowed(true, Some(1), Some(1)));
        assert!(is_attach_trigger_allowed(true, Some(1), Some(2)));
    }

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

    #[test]
    fn reconnect_due_when_no_slot_yet() {
        assert!(reconnect_due(None, Instant::now()));
    }

    #[test]
    fn reconnect_due_respects_backoff_next_attempt() {
        let mut slot = ReconnectSlot::new();
        slot.next_attempt = Instant::now() + std::time::Duration::from_secs(10);
        assert!(!reconnect_due(Some(&slot), Instant::now()));
        slot.next_attempt = Instant::now() - std::time::Duration::from_secs(1);
        assert!(reconnect_due(Some(&slot), Instant::now()));
    }

    #[test]
    fn reconnect_due_is_false_when_given_up_even_past_next_attempt() {
        let mut slot = ReconnectSlot::new();
        slot.given_up = true;
        slot.next_attempt = Instant::now() - std::time::Duration::from_secs(1);
        assert!(!reconnect_due(Some(&slot), Instant::now()));
    }

    #[test]
    fn reconnect_exhausted_at_max_not_before() {
        assert!(!reconnect_exhausted(MAX_RECONNECT_ATTEMPTS - 1));
        assert!(reconnect_exhausted(MAX_RECONNECT_ATTEMPTS));
        assert!(reconnect_exhausted(MAX_RECONNECT_ATTEMPTS + 1));
    }

    #[test]
    fn given_up_slot_resets_only_on_edge_reactivation() {
        let mut slot = ReconnectSlot::new();
        slot.given_up = true;
        assert!(!should_reset_given_up(Some(&slot), false));
        assert!(should_reset_given_up(Some(&slot), true));
        slot.given_up = false;
        assert!(!should_reset_given_up(Some(&slot), true));
        assert!(!should_reset_given_up(None, true));
    }
}
