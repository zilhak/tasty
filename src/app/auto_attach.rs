//! 매핑된 워크스페이스의 자동 attach와 재연결을 처리한다.
//! SSH 연결 준비는 워커가 맡고 메인 루프가 결과를 mirror 세션에 적용한다.
//! 재연결은 예약 시각 또는 해당 워크스페이스 재활성화로 시도한다.
//! 자동 재시도 상한에 도달해도 사용자가 다시 활성화하면 재시도할 수 있다.
//! docs/dev-guide/attach-behavior.md#gui-자동-재연결-스코프 참조.

use std::time::Instant;

use tasty_remote_profiles::{Passkeys, RemoteProfiles};
use tasty_ssh::{self as ssh, Backoff, PortMode, SshTarget, SshTunnel};

use super::attach_client::SessionState;
use crate::app::App;
use crate::model::WorkspaceAttachTarget;
use crate::view::ui::View as _;

/// 자동 재연결의 실패 횟수 상한. 워크스페이스 재활성화로 재시도하는 동작은 막지 않는다.
const MAX_RECONNECT_ATTEMPTS: u32 = 20;

/// 실패한 재연결의 다음 시도 시각과 누적 횟수. 첫 시도는 슬롯 없이 즉시 수행한다.
pub(crate) struct ReconnectSlot {
    backoff: Backoff,
    next_attempt: Instant,
    attempts: u32,
    /// 슬롯을 지우면 첫 시도로 취급하므로 중단 상태를 별도로 보존한다.
    /// 사용자가 해당 워크스페이스로 돌아오면 재개할 수 있다.
    given_up: bool,
}

impl ReconnectSlot {
    fn new() -> Self {
        Self {
            backoff: Backoff::new(),
            next_attempt: Instant::now(),
            attempts: 0,
            given_up: false,
        }
    }
}

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

/// AppEvent에 넣을 수 없는 터널 핸들을 별도 결과 채널로 전달한다.
pub(crate) struct AutoAttachOutcome {
    /// 자동 attach의 로컬 anchor ID. anchor 없는 수동 요청은 None이며 중복 방지 집합에 넣지 않는다.
    pub(crate) anchor_ws_id: Option<u32>,
    pub(crate) remote_ws: u32,
    pub(crate) result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// 신규 mirror 생성 대신 기존 세션 재연결과 실패 시 백오프 갱신을 수행할지 구별한다.
    pub(crate) is_reconnect: bool,
}

impl App {
    /// 활성 워크스페이스의 전환을 한 번 계산해 두 트리거가 같은 전환을 보게 한다.
    pub(crate) fn poll_auto_attach(&mut self) {
        let prev_active = self.auto_attach_last_active_ws;
        let current_ws_id = self
            .focused_window()
            .and_then(|main| main.core_state.workspaces.get(main.state.active_workspace))
            .map(|ws| ws.id);
        self.auto_attach_last_active_ws = current_ws_id;

        self.maybe_trigger_auto_attach(current_ws_id, prev_active);
        self.maybe_trigger_reconnect(current_ws_id, prev_active);
        self.drain_auto_attach_results();
    }

    /// 타이머만 등록·해제한다. 중단 상태를 담은 재연결 슬롯은 보존한다.
    pub(crate) fn sync_reconnect_timers(&mut self, now: Instant) {
        let wakeups: Vec<(u32, Instant)> = self
            .attach_client_sessions
            .iter()
            .filter(|s| s.state() == SessionState::Reconnecting)
            .filter_map(|s| s.anchor_ws_id())
            // 워커가 이미 떠 있는 anchor 는 결과 이벤트(`AutoAttachReady`)가 깨운다.
            .filter(|anchor| !self.auto_attach_active.contains(anchor))
            .filter_map(|anchor| {
                reconnect_wakeup_at(self.auto_attach_reconnect.get(&anchor)).map(|at| (anchor, at))
            })
            .collect();
        crate::app::timers::sync_reconnect_timers(&mut self.timers, &wakeups, now);
    }

    /// 신규 매핑은 활성 상태면 연결하고, 연결 해제 뒤 대기 중인 anchor는 재활성화를 요구한다.
    /// 대상 ID는 매핑에서 가져오며 포커스를 바꾸지 않는다.
    fn maybe_trigger_auto_attach(&mut self, current_ws_id: Option<u32>, prev_active: Option<u32>) {
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
        // 재연결이 맡은 anchor에 새 mirror를 중복 생성하지 않는다.
        if self
            .attach_client_sessions
            .iter()
            .any(|s| s.anchor_ws_id() == Some(anchor) && s.state() == SessionState::Reconnecting)
        {
            return;
        }
        let pending_reactivation = self.auto_attach_pending_reactivation.contains(&anchor);
        if !is_attach_trigger_allowed(pending_reactivation, current_ws_id, prev_active) {
            return;
        }
        if self.auto_attach_active.contains(&anchor) {
            return;
        }
        let Some(remote_ws) = mapping.remote_workspace else {
            return;
        };

        self.auto_attach_active.insert(anchor);
        self.auto_attach_pending_reactivation.remove(&anchor);
        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        let target = mapping.target.clone();
        // SSH 연결 준비가 메인 루프를 막지 않게 한다.
        std::thread::spawn(move || {
            let result = resolve_endpoint(&target);
            let outcome = AutoAttachOutcome {
                anchor_ws_id: Some(anchor),
                remote_ws,
                result,
                is_reconnect: false,
            };
            let _ = tx.send(outcome); // 수신자(메인 루프) drop 시 send 실패 — 무시.
            let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
        });
    }

    /// 시도 중이 아닌 anchor를 예약 시각 또는 해당 워크스페이스 재활성화 때 재연결한다.
    fn maybe_trigger_reconnect(&mut self, current_ws_id: Option<u32>, prev_active: Option<u32>) {
        let anchors: Vec<u32> = self
            .attach_client_sessions
            .iter()
            .filter(|s| s.state() == SessionState::Reconnecting)
            .filter_map(|s| s.anchor_ws_id())
            .collect();
        for anchor in anchors {
            if self.auto_attach_active.contains(&anchor) {
                continue;
            }
            let edge_now =
                current_ws_id == Some(anchor) && is_reactivation_edge(current_ws_id, prev_active);
            let existing_slot = self.auto_attach_reconnect.get(&anchor);
            let due = reconnect_due(existing_slot, Instant::now());
            if !edge_now && !due {
                continue;
            }
            if should_reset_given_up(existing_slot, edge_now) {
                self.auto_attach_reconnect.remove(&anchor);
            }
            // 대기 중 워크스페이스나 매핑이 사라졌을 수 있으므로 다시 읽는다.
            let mapping = self.main_windows_iter_mut().find_map(|m| {
                m.core_state
                    .workspaces
                    .iter()
                    .find(|ws| ws.id == anchor)
                    .and_then(|ws| ws.attach_mapping.clone())
            });
            let Some(mapping) = mapping else {
                self.auto_attach_reconnect.remove(&anchor);
                self.auto_attach_pending_reactivation.remove(&anchor);
                continue;
            };
            let Some(remote_ws) = mapping.remote_workspace else {
                continue; // 원격 workspace id 미지정 — 자동 attach 와 동일 원칙 3.
            };

            self.auto_attach_active.insert(anchor);
            self.auto_attach_pending_reactivation.remove(&anchor);
            let tx = self.auto_attach_tx.clone();
            let proxy = self.view.proxy.clone();
            let target = mapping.target.clone();
            std::thread::spawn(move || {
                let result = resolve_endpoint(&target);
                let outcome = AutoAttachOutcome {
                    anchor_ws_id: Some(anchor),
                    remote_ws,
                    result,
                    is_reconnect: true,
                };
                let _ = tx.send(outcome); // 수신자(메인 루프) drop 시 send 실패 — 무시.
                let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
            });
        }
    }

    pub(crate) fn drain_auto_attach_results(&mut self) {
        while let Ok(outcome) = self.auto_attach_rx.try_recv() {
            self.apply_auto_attach_outcome(outcome);
        }
    }

    fn apply_auto_attach_outcome(&mut self, outcome: AutoAttachOutcome) {
        let AutoAttachOutcome {
            anchor_ws_id,
            remote_ws,
            result,
            is_reconnect,
        } = outcome;
        match result {
            Ok((tunnel, port)) => {
                self.handle_auto_attach_connected(
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
                    self.auto_attach_active.remove(&anchor);
                    if is_reconnect {
                        self.on_reconnect_attempt_failed(anchor, &e);
                    }
                }
            }
        }
    }

    fn handle_auto_attach_connected(
        &mut self,
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
                self.auto_attach_active.remove(&anchor);
            }
            return;
        }
        let attach_result = if is_reconnect {
            let idx = anchor_ws_id.and_then(|anchor| {
                self.attach_client_sessions.iter().position(|s| {
                    s.anchor_ws_id() == Some(anchor) && s.state() == SessionState::Reconnecting
                })
            });
            match idx {
                Some(idx) => self.reconnect_session(idx, port, tunnel),
                // 사용자가 기다리는 동안 mirror를 닫았으면 연결할 세션이 없다.
                None => Ok(()),
            }
        } else {
            self.start_gui_attach(port, remote_ws, tunnel, anchor_ws_id)
                .map(|_| ())
        };
        match attach_result {
            Ok(()) => {
                if let Some(anchor) = anchor_ws_id {
                    self.auto_attach_reconnect.remove(&anchor);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "attach mirror 실패 (anchor ws {anchor_ws_id:?}, remote ws {remote_ws}, reconnect={is_reconnect}): {e}"
                );
                if let Some(anchor) = anchor_ws_id {
                    self.auto_attach_active.remove(&anchor);
                    if is_reconnect {
                        self.on_reconnect_attempt_failed(anchor, &e);
                    }
                }
            }
        }
    }

    /// already_attached는 긴 고정 간격, 나머지 실패는 지수 백오프로 재시도한다.
    /// jitter로 동시 재시도 집중을 줄이며 상한에 도달하면 자동 재시도만 멈춘다.
    fn on_reconnect_attempt_failed(&mut self, anchor: u32, err: &anyhow::Error) {
        let permanent_conflict = err.to_string().contains("already_attached");
        let slot = self
            .auto_attach_reconnect
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
        for main in self.main_windows_iter_mut() {
            if main.core_state.workspaces.iter().any(|ws| ws.id == anchor) {
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
