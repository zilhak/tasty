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
//!
//! ## TODO 27 — silent disconnect 후 backoff 자동 재연결
//!
//! disconnect 로 anchor 세션이 `Reconnecting` 상태(`attach_client.rs`)가 되면, 위
//! 레벨/엣지 트리거와 **병행해** 아래 backoff 스케줄이 돈다:
//! ```text
//! [poll_auto_attach] (매 프레임, 최소 AttachPoll 3초 backstop 으로 idle 에도 보장)
//!   ├─ maybe_trigger_auto_attach: anchor 에 Reconnecting 세션이 있으면 스킵(레이스 방지 — 재연결은 아래가 전담)
//!   ├─ maybe_trigger_reconnect: Reconnecting 세션이 있는 각 anchor 마다
//!   │     - 사용자가 지금 그 워크스페이스로 전환해왔으면(엣지) 즉시, 아니면
//!   │       backoff `next_attempt` 시각이 됐으면 → 워커 spawn(`is_reconnect: true`)
//!   └─ drain_auto_attach_results: `is_reconnect` 로 성공 시 `reconnect_session`
//!         (survivor mapping, TODO 28) vs `start_gui_attach` 분기. 실패는
//!         `on_reconnect_attempt_failed` 로 backoff 갱신(`already_attached` 는
//!         영구 충돌로 취급해 지수 증가 없이 max 간격 고정, 그 외엔 통상 지수 증가).
//!         시도 상한(`MAX_RECONNECT_ATTEMPTS`) 초과 시 자동 재시도만 중단(안내
//!         toast) — `auto_attach_pending_reactivation` 은 남겨 사용자가 워크스페이스를
//!         왕복하면 여전히 수동으로 재시도된다.
//! ```
//! **backoff 는 non-blocking** — `tasty_cli::ssh::Backoff::sleep()`(blocking) 대신
//! `current()`/`advance()` 로 "다음 시도 시각"만 계산해 [`ReconnectSlot`] 에 저장하고,
//! 매 프레임 `Instant::now()` 와 비교한다(GUI 메인 스레드를 블록할 수 없어서).

use std::time::Instant;

use tasty_cli::ssh::{self, Backoff, PortMode, SshTarget, SshTunnel};
use tasty_remote_profiles::{Passkeys, RemoteProfiles};

use super::attach_client::SessionState;
use crate::app::App;
use crate::model::WorkspaceAttachTarget;
use crate::view::ui::View as _;

/// TODO 27 — anchor 하나당 backoff 재연결 시도 상한. 초과하면 자동(backoff) 재시도를
/// 멈추고 안내 toast 를 띄운다 — 무한 재시도로 조용히 CPU/네트워크를 계속 쓰는 것을
/// 막는다. 사용자가 워크스페이스를 왕복하는 엣지 트리거는 이 상한과 무관하게 계속
/// 동작한다(수동 재시도는 항상 가능).
const MAX_RECONNECT_ATTEMPTS: u32 = 20;

/// TODO 27 — anchor 하나의 backoff 재연결 스케줄. `Reconnecting` 진입 시 생성되지
/// 않고(첫 시도는 즉시), 시도가 **실패**할 때만 갱신되며 다음 시도 시각을 저장한다.
pub(crate) struct ReconnectSlot {
    backoff: Backoff,
    next_attempt: Instant,
    attempts: u32,
}

impl ReconnectSlot {
    fn new() -> Self {
        Self {
            backoff: Backoff::new(),
            next_attempt: Instant::now(),
            attempts: 0,
        }
    }
}

/// 자동 attach 워커 스레드 → 메인 루프 결과. 터널 핸들/포트를 채널로 전달한다
/// (AppEvent 는 Debug 라 핸들을 싣지 못해 별도 채널 사용).
pub(crate) struct AutoAttachOutcome {
    /// 매핑된(anchor) 로컬 워크스페이스 id. 자동 attach(매핑 활성화)면 `Some(id)`,
    /// **IPC 수동 트리거**(`remote.attach` — anchor 없는 브라우징→attach)면 `None`.
    /// None 이면 재attach 게이트(`auto_attach_active`)를 건드리지 않는다.
    pub(crate) anchor_ws_id: Option<u32>,
    /// 원격 tasty 의 attach 대상 workspace_id.
    pub(crate) remote_ws: u32,
    /// 엔드포인트 해석 결과: `(터널 핸들 or None, 접속 포트)`.
    pub(crate) result: anyhow::Result<(Option<SshTunnel>, u16)>,
    /// TODO 27 — 이 워커가 backoff 재연결 트리거(`maybe_trigger_reconnect`)로
    /// spawn 됐는지. `drain_auto_attach_results` 가 성공 시 `reconnect_session`
    /// (survivor mapping)과 `start_gui_attach`(완전 신규) 중 무엇을 부를지, 실패 시
    /// backoff 스케줄을 갱신할지 결정하는 데 쓴다.
    pub(crate) is_reconnect: bool,
}

impl App {
    /// `about_to_wait` 매 프레임 — 활성 워크스페이스 매핑을 보고 자동 attach 를
    /// 트리거하고(레벨/엣지), `Reconnecting` anchor 들의 backoff 재연결을 확인한 뒤,
    /// 완료된 워커 결과를 적용한다(다 cheap — 후보 없으면 즉시 반환). 엣지 판정에 쓰는
    /// (현재/직전 활성 ws id)를 한 프레임에 한 번만 계산해 두 트리거가 공유한다 —
    /// 따로 계산하면 `auto_attach_last_active_ws` 갱신 순서에 따라 엣지가 어긋난다.
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

    /// 활성 워크스페이스가 매핑 Some & 아직 attach 안 됐으면 워커 스레드로 SSH 터널
    /// 수립을 시작한다(메인 루프 무블록). 포커스 독립(원칙 3): 활성 상태를 *읽어*
    /// 트리거할 뿐, 동작은 ID(anchor/remote_ws)로 결정된다.
    ///
    /// **재진입 대기(pending reactivation) anchor 만 엣지 게이팅**:
    /// `auto_attach_pending_reactivation` 에 속한 anchor(= silent disconnect 로
    /// `cleanup_mirror_workspace` 가 방금 정리한 워크스페이스)는 `auto_attach_last_active_ws`
    /// 와 비교해 활성 워크스페이스가 **바뀐 프레임**(전환 엣지)이어야만 후보로 본다 —
    /// disconnect 직후에도 그 워크스페이스를 계속 보고 있으면(전환 없음) 재트리거하지
    /// 않는다("정리만, 자동 재연결은 사용자가 벗어났다 되돌아오는 등 수동 재진입
    /// 필요"). **그 집합에 없는 anchor 는 기존처럼 활성화 즉시(레벨) 트리거된다** —
    /// 예를 들어 이미 활성인 워크스페이스에 `attach_mapping` 을 방금 새로 설정한
    /// 경우(`tasty set workspace --ssh-profile ...`)는 disconnect 를 겪은 적이 없어
    /// 전환을 요구하면 안 된다(엣지를 모든 anchor 에 무차별 적용하면 이 흔한 CLI
    /// 시나리오가 트리거되지 않는 회귀가 생긴다).
    fn maybe_trigger_auto_attach(&mut self, current_ws_id: Option<u32>, prev_active: Option<u32>) {
        // 활성 ws 의 (anchor id, 매핑)을 읽어 후보 수집.
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
        // TODO 27 — 이 anchor 에 이미 `Reconnecting` 세션이 있으면 재연결은
        // `maybe_trigger_reconnect` 전담 대상이다. 그쪽이 `auto_attach_active` 를 아직
        // 안 채운 순간(레이스 윈도우)에 여기가 먼저 완전 신규 `start_gui_attach` 를
        // 트리거해버리면 같은 anchor 에 mirror workspace 가 중복 생성된다 — 반드시
        // 스킵.
        if self
            .attach_client_sessions
            .iter()
            .any(|s| s.anchor_ws_id() == Some(anchor) && s.state() == SessionState::Reconnecting)
        {
            return;
        }
        // 재진입 대기 중인 anchor 만 전환 엣지를 요구 — 그 외(신규 mapping 등)는
        // 레벨(즉시) 트리거.
        let pending_reactivation = self.auto_attach_pending_reactivation.contains(&anchor);
        if !is_attach_trigger_allowed(pending_reactivation, current_ws_id, prev_active) {
            return;
        }
        // 이미 진행 중/established → skip(중복 attach 방지).
        if self.auto_attach_active.contains(&anchor) {
            return;
        }
        // 원격 workspace id 미지정 → 자동 attach skip(원칙 3, plan §11-2).
        let Some(remote_ws) = mapping.remote_workspace else {
            return;
        };

        self.auto_attach_active.insert(anchor);
        // 트리거 성공(워커 spawn) — 재진입 대기 해소.
        self.auto_attach_pending_reactivation.remove(&anchor);
        let tx = self.auto_attach_tx.clone();
        let proxy = self.view.proxy.clone();
        let target = mapping.target.clone();
        // 워커: 프로필 resolve + 포트 발견 + ssh -L 터널(최대 ~수초 블록).
        std::thread::spawn(move || {
            let result = resolve_endpoint(&target);
            let outcome = AutoAttachOutcome {
                anchor_ws_id: Some(anchor),
                remote_ws,
                result,
                is_reconnect: false,
            };
            let _ = tx.send(outcome); // 수신자(메인 루프) drop 시 send 실패 — 무시.
            // 메인 루프를 깨워 결과를 drain 시킨다(idle 상태에서도 즉시 반영).
            let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady); // event loop 종료 시에만 실패 — 무시
        });
    }

    /// TODO 27 — `Reconnecting` 세션이 있는 각 anchor 의 backoff 스케줄을 확인해
    /// 재연결 워커를 spawn 한다. 두 트리거를 병행: ① 사용자가 **지금** 그 anchor
    /// 워크스페이스로 전환해 돌아왔으면(엣지, `current_ws_id == anchor` 한정 — 다른
    /// 워크스페이스로의 무관한 전환까지 모든 Reconnecting anchor 를 깨우면 안 된다)
    /// 즉시, ② 아니어도 backoff 의 `next_attempt` 시각이 지났으면. 이미 시도가
    /// 진행 중인 anchor(`auto_attach_active`)는 skip(기존 게이트 재사용 — 중복 attach
    /// 방지).
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
            let due = match self.auto_attach_reconnect.get(&anchor) {
                Some(slot) => Instant::now() >= slot.next_attempt,
                // 아직 실패한 적 없음(Reconnecting 진입 직후) — 1차 시도는 즉시.
                None => true,
            };
            if !edge_now && !due {
                continue;
            }
            // mapping 재조회 — anchor 워크스페이스 자체가 삭제됐거나 attach_mapping 이
            // 그 사이 바뀌었으면(Codex 크로스체크 지적) 이 재연결 대상은 소멸한 것 —
            // 스케줄을 정리하고 skip.
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
                let _ = tx.send(outcome);
                let _ = proxy.send_event(crate::app::event::AppEvent::AutoAttachReady);
            });
        }
    }

    /// 워커가 보낸 엔드포인트 결과를 적용한다 — 성공이면 mirror 를 띄우고(터널 핸들
    /// 세션에 보관), 실패면 anchor 게이트를 풀어 재활성 시 재시도 가능하게 한다.
    /// `is_reconnect` 로 신규 attach(`start_gui_attach`) 와 재연결(`reconnect_session`,
    /// TODO 27/28)을 분기한다.
    pub(crate) fn drain_auto_attach_results(&mut self) {
        while let Ok(outcome) = self.auto_attach_rx.try_recv() {
            let AutoAttachOutcome {
                anchor_ws_id,
                remote_ws,
                result,
                is_reconnect,
            } = outcome;
            match result {
                Ok((tunnel, port)) => {
                    // self(loopback) mirror 차단(원칙 1 ②): resolve 된 포트가 이 인스턴스
                    // 자신의 IPC 포트면 자기 화면 mirror = 사용자 입력 재현 성격이라
                    // release 에서 거부한다. SSH 터널의 local_port 는 자기 포트와 다르므로
                    // 원격 attach 는 통과한다. 로컬 self-mirror 검증은 debug 빌드로.
                    #[cfg(not(debug_assertions))]
                    if self.hub.ipc_server.as_ref().map(|s| s.port()) == Some(port) {
                        tracing::warn!(
                            "self(loopback) attach (port={port}) 는 release 빌드에서 차단됩니다 \
                             — 로컬 self-attach 는 debug 빌드 전용."
                        );
                        if let Some(anchor) = anchor_ws_id {
                            self.auto_attach_active.remove(&anchor);
                        }
                        continue;
                    }
                    let attach_result = if is_reconnect {
                        let idx = anchor_ws_id.and_then(|anchor| {
                            self.attach_client_sessions.iter().position(|s| {
                                s.anchor_ws_id() == Some(anchor)
                                    && s.state() == SessionState::Reconnecting
                            })
                        });
                        match idx {
                            Some(idx) => self.reconnect_session(idx, port, tunnel),
                            // Reconnecting 세션이 그 사이 사라짐(사용자가 mirror 를
                            // 직접 닫는 등) — 재연결 대상 소멸, no-op 성공 취급.
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
    }

    /// TODO 27 — 재연결 시도 실패 후 backoff 스케줄을 갱신한다. `already_attached`
    /// (다른 클라이언트가 여전히 그 원격 워크스페이스를 점유 중 — 영구적 충돌일 수
    /// 있음)는 지수 증가 대신 max 간격으로 고정해 폭주 없이 계속 대기하고, 그 외
    /// (네트워크/SSH 등 일시적 실패)는 통상적인 지수 백오프로 늘린다. 시도 횟수가
    /// 상한을 넘으면 자동(backoff) 재시도만 멈추고(수동 엣지 트리거는 계속 유효) 1 회
    /// 안내 toast 를 띄운다. 간격에 ±20% jitter 를 둬 여러 anchor 가 동시에 끊겼을 때
    /// 재시도가 한 tick 에 몰리는 것을 방지한다.
    fn on_reconnect_attempt_failed(&mut self, anchor: u32, err: &anyhow::Error) {
        let permanent_conflict = err.to_string().contains("already_attached");
        let slot = self
            .auto_attach_reconnect
            .entry(anchor)
            .or_insert_with(ReconnectSlot::new);
        slot.attempts += 1;
        if slot.attempts > MAX_RECONNECT_ATTEMPTS {
            self.auto_attach_reconnect.remove(&anchor);
            self.notify_reconnect_giveup(anchor);
            return;
        }
        if permanent_conflict {
            // 다른 클라이언트가 계속 점유 중 — 지수 증가 없이 max 간격 고정(폭주
            // 방지). 상대가 disconnect 하면 다음 tick 에 다시 시도할 여지는 유지.
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

    /// TODO 27 — `MAX_RECONNECT_ATTEMPTS` 초과로 자동 재시도를 포기했음을 anchor
    /// 워크스페이스에 1 회 안내한다. `auto_attach_pending_reactivation` 은 여기서
    /// 건드리지 않는다 — 사용자가 그 워크스페이스를 왕복하는 엣지 트리거는 여전히
    /// 유효해야 한다("자동만 멈춤, 수동은 항상 가능").
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

/// 매핑 타깃 → 접속 엔드포인트(터널 or loopback 포트). 워커 스레드에서 실행(블록 OK).
fn resolve_endpoint(target: &WorkspaceAttachTarget) -> anyhow::Result<(Option<SshTunnel>, u16)> {
    // ① 접속 스펙 결정(destination + remote_tasty + port_mode + port_file).
    let (ssh_target, remote_tasty, port_mode, port_file) = match target {
        WorkspaceAttachTarget::Profile { name } => {
            let profiles = RemoteProfiles::load();
            let passkeys = Passkeys::load();
            let p = profiles
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("원격 프로필 '{name}' 을 찾을 수 없습니다"))?;
            // tasty-attach kind 검증 + 비활성 게이트 + ref/inline resolve 를 한곳에서.
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

/// `current`(이번 프레임 활성 ws id) 가 `previous`(직전 프레임 활성 ws id) 와 달라진
/// **전환** 인지 판정한다. `maybe_trigger_auto_attach` 의 엣지 트리거 조건
/// (silent disconnect 후 사용자가 실제로 워크스페이스를 전환해 돌아와야 재시도)의
/// 핵심 술어라 독립적으로 테스트한다. `current` 가 `None`(포커스 창에 활성 ws 자체가
/// 없는 비정상 상태)이면 전환으로 치지 않는다.
fn is_reactivation_edge(current: Option<u32>, previous: Option<u32>) -> bool {
    current.is_some() && current != previous
}

/// `maybe_trigger_auto_attach` 가 candidate anchor 를 실제로 트리거할지 결정하는
/// 술어. `pending_reactivation`(그 anchor 가 `App.auto_attach_pending_reactivation`
/// 에 있는지 — silent disconnect 로 방금 정리돼 재진입 대기 중인지)이 `false` 면
/// (신규 mapping 등 disconnect 를 겪은 적 없는 anchor) 무조건 허용한다(레벨 트리거,
/// 기존 동작과 동일). `true` 면 `is_reactivation_edge` 로 워크스페이스 전환이 실제로
/// 있었을 때만 허용한다 — 그래야 "이미 활성인 워크스페이스에 매핑을 새로 설정하면
/// 즉시 트리거"와 "disconnect 직후엔 전환 전까지 억제"가 동시에 성립한다.
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
    fn reactivation_edge_only_on_transition() {
        // 첫 활성화(직전 없음) — 전환으로 인정.
        assert!(is_reactivation_edge(Some(1), None));
        // 같은 ws 가 계속 활성 — 전환 아님(이게 없으면 silent disconnect 직후 anchor
        // 가 여전히 활성인 상태에서 매 프레임 재트리거되는 회귀가 재현된다).
        assert!(!is_reactivation_edge(Some(1), Some(1)));
        // 다른 ws 로 전환 — 전환으로 인정.
        assert!(is_reactivation_edge(Some(2), Some(1)));
        // 활성 ws 자체가 없음 — 전환 아님.
        assert!(!is_reactivation_edge(None, Some(1)));
    }

    #[test]
    fn new_mapping_triggers_immediately_without_transition() {
        // 신규 mapping(= auto_attach_pending_reactivation 에 없음) 은 워크스페이스
        // 전환 없이도(current == previous, "계속 활성 상태") 즉시 트리거 후보가 된다
        // — `tasty set workspace --ssh-profile ...` 를 이미 활성인 워크스페이스에
        // 실행하는 흔한 CLI 시나리오의 회귀 방지 테스트.
        assert!(is_attach_trigger_allowed(false, Some(1), Some(1)));
        // 전환이 있어도 당연히 허용.
        assert!(is_attach_trigger_allowed(false, Some(2), Some(1)));
    }

    #[test]
    fn disconnected_anchor_waits_for_transition_before_retrigger() {
        // disconnect 로 정리된(= auto_attach_pending_reactivation 에 있음) anchor 는
        // 워크스페이스가 계속 활성 상태(전환 없음)면 재트리거되지 않는다 — 기존
        // TODO 06 목표(조용한 자동 재연결 억제) 유지 확인.
        assert!(!is_attach_trigger_allowed(true, Some(1), Some(1)));
        // 다른 워크스페이스로 갔다가 돌아오는 등 실제 전환이 있으면 허용.
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
}
