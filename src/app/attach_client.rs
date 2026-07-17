//! attach/detach 작업 J (B2) — 호스트측 in-process attach-client.
//!
//! step4/6 은 서버(피점유) plane 과 CLI demux-dump 만 만들었고, GUI 가 원격 grid 를
//! mirror 해 그리는 경로는 step6 R2("후속")로 남았다. 이 모듈이 그 마지막 통합이다:
//!
//! - `dispatch_pending_gui_attach`: IPC `attach.into_gui` 가 쌓은 `(port, workspace)`
//!   요청을 `about_to_wait` 에서 drain.
//! - `start_gui_attach`: 원격 tasty(loopback port)에 attach 연결 → `attached_workspace`
//!   디스크립터로 **로컬 mirror Workspace 트리 재구성**(mirror `Terminal::new_detached`
//!   를 `TerminalStore` 삽입, remote↔local id 재매핑) → 기존 렌더러 재사용(신규 셰이더
//!   0). 입력은 `set_input_sink` 로 forward(keyboard.rs 무변경).
//! - `apply_attach_client_output`: reader thread 가 `AttachClientData` 로 깨울 때마다
//!   누적된 원격 출력을 mirror 에 적용하고 화면을 repaint. 끊긴(force-detach/EOF) 세션의
//!   mirror 를 정리. (`AttachPoll` 3초 tick 도 backstop 으로 같은 함수를 호출한다.)
//!
//! client mirror 는 내가 직접 다루는 대상이라 로컬 워크스페이스처럼 **데이터가 오는 즉시**
//! 갱신한다(로컬 PTY 의 TerminalOutput wake 와 동형). 서버측 readonly 뷰(`attach_poll` ①)만
//! 3초 cadence 로 게이트한다(plan §4). 범위는 작업 J — 자동 매핑(ssh-profiles/
//! workspace.attach_mapping)은 단계 7. 이 모듈의 `start_gui_attach` 가 단계 7 Phase B2 의
//! 호출 진입점이다.

use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use tasty_cli::stream::StreamConnection;
use tasty_terminal::Terminal;

use crate::AppEvent;
use crate::app::App;
use crate::ipc::stream::{self, STREAM_PROTO, StreamControl, StreamTag, StructuralOp};
use crate::model::{
    EmptySurface, Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, TerminalSurface,
    Workspace,
};
use crate::view::ui::View as _;

/// reader thread 가 원격에서 받은 mirror 갱신 이벤트. 출력 바이트와 resize 통지를
/// **한 버퍼에 순서대로** 담아 프레임 도착 순서(원격의 apply 순서)를 보존한다 —
/// resize 앞뒤 출력이 올바른 그리드에서 재생되도록.
pub(crate) enum MirrorEvent {
    /// 원격 출력 바이트 `(remote_surface_id, bytes)`.
    Data(u32, Vec<u8>),
    /// 원격 grid resize `(remote_surface_id, cols, rows)` — mirror 크기 갱신.
    Resize(u32, usize, usize),
    /// 원격 surface 의 busy/idle 활동 상태 `(remote_surface_id, busy)`. mirror 터미널은
    /// 로컬 PTY 가 없어 스스로 활동 상태를 계산할 수 없으므로(`process_id()` 가 항상
    /// `None`), 이 push 가 mirror 워크스페이스 사이드바 status dot 의 유일한 데이터
    /// 소스다(`CoreState::set_mirror_surface_busy`).
    Activity(u32, bool),
    /// forward 한 구조 op 가 원격에서 실패했다(2단계). `reason`(예: 미등록 kind)을 담아
    /// 메인루프가 실패 toast 를 띄운다. 성공은 무음(별도 이벤트 없음).
    StructuralFailed(String),
    /// 원격 워크스페이스 구조가 바뀌었다(3단계 역반영). 원격 ws 전체 트리+surfaces 를
    /// 담아 메인루프가 mirror 트리를 증분 재구성한다(survivor 터미널 local id 유지 →
    /// scrollback 보존, 신규만 새 mirror, 사라진 것 제거).
    StructuralDelta {
        workspace_id: u32,
        tree: Value,
        surfaces: Vec<Value>,
    },
    /// (03 screenshot→remote-clipboard) 원격이 이 mirror 세션이 업로드한 캡처를
    /// 처리한 결과(`capture_result` 커스텀 이벤트 — `StreamControl` enum 밖, 그
    /// enum 이 인식 못 하는 별도 "event" 값으로 같은 Control 채널을 탄다). 성공 시
    /// `path` 가 원격 파일시스템 경로, 실패 시 `reason`.
    CaptureResult {
        ok: bool,
        path: Option<String>,
        reason: Option<String>,
    },
}

/// reader thread 가 누적하고 메인 스레드의 apply 가 drain 하는 원격 mirror 이벤트 버퍼.
pub(crate) type RemoteOutputBuffer = Arc<Mutex<Vec<MirrorEvent>>>;

/// client 가 점유한 원격 워크스페이스의 로컬 mirror 세션(작업 J).
pub(crate) struct AttachClientSession {
    /// 로컬에 추가된 mirror Workspace 의 id.
    local_workspace: u32,
    /// 원격 surface_id → 로컬 mirror surface_id. 출력 demux 적용에 사용.
    remote_to_local: HashMap<u32, u32>,
    /// reader thread 가 누적하는 원격 출력 `(remote_surface_id, bytes)`.
    output: RemoteOutputBuffer,
    /// reader thread 가 EOF/force-detach 를 만나면 set. apply 가 보고 mirror 정리.
    disconnected: Arc<AtomicBool>,
    /// 입력/Detach 프레임 송신용 writer(다중 forwarder 와 직렬화 공유).
    writer: Arc<Mutex<TcpStream>>,
    // 이유: 서버가 할당한 mirror 세션 식별자 — 현재 read 경로 없음(진단/향후 프레임 라우팅용 보관).
    #[allow(dead_code)]
    client_id: u32,
    /// 단계 7 — 자동 attach 의 SSH 터널 핸들. 세션이 살아있는 동안 보관해 Drop(자식
    /// ssh kill)을 막는다. 수동 트리거(`attach.into_gui`)·loopback 은 None.
    #[allow(dead_code)]
    tunnel: Option<tasty_cli::ssh::SshTunnel>,
    /// 단계 7 — 이 mirror 를 띄운 매핑된(anchor) 로컬 워크스페이스 id. 세션 정리 시
    /// `auto_attach_active` 에서 제거해 재활성 시 재attach 가능하게 한다. 수동 None.
    anchor_ws_id: Option<u32>,
    /// forward 한 구조 op 의 op_id 시퀀스(2단계). 회신 correlate/로그용 — 단조 증가.
    op_seq: u64,
    /// client-driven resize(ADR-0045) 중복 전송 억제. **원격 surface_id →
    /// 마지막으로 forward 한 (cols, rows)**. 로컬 레이아웃 스윕은 매 프레임 돌고
    /// mirror grid 는 server echo 로만 갱신되므로, echo 왕복(약 1 RTT) 동안 같은
    /// 목표가 매 프레임 재계산된다 — 여기서 직전 전송값과 같으면 재전송을 생략해
    /// 네트워크 프레임 폭주를 막는다(서버측 동일값 no-op 이 2차 방어). TCP 는 신뢰
    /// 전송이라 한 번 보낸 값은 도달이 보장돼 재전송이 불필요하다.
    last_forwarded_resize: HashMap<u32, (usize, usize)>,
}

impl App {
    /// `about_to_wait` 에서 호출 — IPC 가 쌓은 GUI attach 요청을 drain 해 실행한다.
    pub(crate) fn dispatch_pending_gui_attach(&mut self) {
        let mut reqs: Vec<(u16, u32)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            reqs.append(&mut main.core_state.pending_gui_attach);
        }
        if let Some(e) = self.core_state.as_mut() {
            reqs.append(&mut e.pending_gui_attach);
        }
        // self(loopback) GUI mirror 차단(원칙 1 ②): target port 가 이 인스턴스 자신의
        // IPC 포트면 사용자 입력 재현(자기 화면 mirror) 성격이라 release 에서 거부한다.
        // 원격 GUI mirror 는 ssh -L 터널의 local_port(자기 IPC 포트와 다름)라 통과한다.
        // 로컬 self-mirror 검증은 debug 빌드 `tasty debug attach` 로 한다.
        #[cfg(not(debug_assertions))]
        let self_port = self.hub.ipc_server.as_ref().map(|s| s.port());
        for (port, workspace) in reqs {
            #[cfg(not(debug_assertions))]
            if self_port == Some(port) {
                tracing::warn!(
                    "self(loopback) attach.into_gui (port={port}) 는 release 빌드에서 \
                     차단됩니다 — 로컬 self-attach 는 debug 빌드 전용."
                );
                continue;
            }
            if let Err(e) = self.start_gui_attach(port, workspace, None, None) {
                tracing::warn!("gui attach failed (port={port}, ws={workspace}): {e}");
            }
        }

        // 사용자 경로(remote_attach 팝업 Connect) — 조회 터널을 재사용해 attach 하고,
        // 성공 시 새 mirror ws 로 **focus 이동**(사용자 확정 동작 — 원칙 1②). IPC 경로와
        // 분리된 별도 큐라 release IPC/에이전트가 이 focus 이동 경로를 탈 수 없다.
        let mut user_reqs: Vec<crate::core::GuiAttachUserReq> = Vec::new();
        for main in self.main_windows_iter_mut() {
            user_reqs.append(&mut main.core_state.pending_gui_attach_user);
        }
        if let Some(e) = self.core_state.as_mut() {
            user_reqs.append(&mut e.pending_gui_attach_user);
        }
        for req in user_reqs {
            // self(loopback) attach 는 release 에서 차단(원칙 1②) — IPC 경로와 동일 게이트.
            #[cfg(not(debug_assertions))]
            if self_port == Some(req.port) {
                tracing::warn!(
                    "self(loopback) remote-attach (port={}) 는 release 빌드에서 차단됩니다.",
                    req.port
                );
                continue;
            }
            match self.start_gui_attach(req.port, req.workspace, req.tunnel, None) {
                Ok(ws_id) => self.focus_mirror_workspace(ws_id),
                Err(e) => tracing::warn!(
                    "remote-attach failed (port={}, ws={}): {e}",
                    req.port,
                    req.workspace
                ),
            }
        }
    }

    /// 원격 tasty(loopback `port`)의 `workspace` 를 mirror 로 재구성해 GUI 에 띄운다.
    /// loopback 연결+핸드셰이크는 near-instant 라 동기 처리.
    ///
    /// 단계 7 자동 attach(`auto_attach.rs`)는 SSH 터널을 먼저 세워 그 `tunnel.local_port`
    /// 를 `port` 로 넘기고 `tunnel` 핸들을 세션에 실어 Drop 을 막는다. `anchor_ws_id` 는
    /// 매핑된 로컬 워크스페이스 id(세션 정리 시 재attach 게이트 해제용). 수동 트리거는
    /// 둘 다 None.
    ///
    /// 반환값은 새로 만든 로컬 mirror workspace 의 id — 사용자 경로(remote_attach 팝업)
    /// 가 이 id 로 focus 를 옮기는 데 쓴다(IPC/자동 경로는 반환값을 무시해 focus 중립).
    pub(crate) fn start_gui_attach(
        &mut self,
        port: u16,
        workspace: u32,
        tunnel: Option<tasty_cli::ssh::SshTunnel>,
        anchor_ws_id: Option<u32>,
    ) -> anyhow::Result<u32> {
        // 1. 연결 + 핸드셰이크 + 디스크립터 수신.
        let sock = TcpStream::connect(("127.0.0.1", port))?;
        let (mut conn, client_id) =
            StreamConnection::open_attach_workspace(sock, STREAM_PROTO, workspace)?;
        let first = conn.recv()?;
        if first.tag != StreamTag::Control {
            anyhow::bail!("expected attach Control frame, got {:?}", first.tag);
        }
        let ctrl: Value = serde_json::from_slice(&first.payload)?;
        match ctrl.get("event").and_then(|v| v.as_str()) {
            Some("attached_workspace") => {}
            Some("attach_error") => {
                let reason = ctrl
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                anyhow::bail!("workspace attach rejected: {reason}");
            }
            other => anyhow::bail!("unexpected attach control event: {other:?}"),
        }

        let name = ctrl
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("remote")
            .to_string();
        let surfaces = ctrl
            .get("surfaces")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tree = ctrl.get("tree").cloned().unwrap_or(Value::Null);

        // 입력/Detach 송신용 writer(forwarder 들과 직렬화 공유 — 프레임 인터리브 방지).
        let writer = Arc::new(Mutex::new(conn.try_clone_writer()?));

        // 2. focus 엔진에 mirror 구성(스코프 borrow). 로컬 id 재매핑 + mirror terminal +
        //    입력 sink forwarder.
        let local_ws_id;
        // client mirror reader thread 가 원격 출력 수신 즉시 메인 루프를 깨우는 데 쓴다
        // (실시간 갱신 — 서버 readonly 의 3초 cadence 와 분리).
        let proxy;
        let mut remote_to_local: HashMap<u32, u32> = HashMap::new();
        let mut terminal_locals: HashSet<u32> = HashSet::new();
        {
            let Some(main) = self.focused_window_mut() else {
                anyhow::bail!("no focused window to host mirror workspace");
            };
            proxy = main.proxy.clone();
            let engine = &mut main.core_state;
            let ids = engine.next_ids.clone();

            for s in &surfaces {
                let remote_id = s.get("remote_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let local_id = ids.next_surface();
                remote_to_local.insert(remote_id, local_id);
                if s.get("role").and_then(|v| v.as_str()) == Some("terminal") {
                    let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                    let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                    make_mirror_surface(remote_id, local_id, cols, rows, &writer, engine);
                    terminal_locals.insert(local_id);
                }
                // 비-터미널은 mirror 불가(design §4.4) → 로컬 id 만(placeholder leaf).
            }

            local_ws_id = ids.next_workspace();
            let mut ws = build_mirror_workspace(
                local_ws_id,
                &name,
                &tree,
                &ids,
                &remote_to_local,
                &terminal_locals,
            );
            // client mirror 표식 — 사이드바 이름 앞 하늘색 glyph(레일=우하단 chip)로 표시
            // (로컬 ws 와 구분; status dot 은 실행상태 전용). 상세 view.rs draw_workspace_card.
            ws.mirror = true;
            engine.workspaces.push(ws);
            main.mark_dirty();
        }

        // 3. reader thread: 원격 출력 → 버퍼(remote_id 키). EOF/force → disconnected.
        let output: RemoteOutputBuffer = Arc::new(Mutex::new(Vec::new()));
        let disconnected = Arc::new(AtomicBool::new(false));
        {
            let output = output.clone();
            let disconnected = disconnected.clone();
            std::thread::spawn(move || {
                loop {
                    match conn.recv() {
                        Ok(frame) => match frame.tag {
                            StreamTag::Data => {
                                if let Some((sid, payload)) = stream::decode_mux(&frame.payload)
                                    && let Ok(mut buf) = output.lock()
                                {
                                    buf.push(MirrorEvent::Data(sid, payload.to_vec()));
                                }
                                // 실시간 갱신: 데이터가 오는 즉시 메인 루프를 깨워 mirror 에
                                // 적용한다(로컬 PTY 의 TerminalOutput wake 와 동형).
                                let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                            }
                            StreamTag::Detach => {
                                disconnected.store(true, Ordering::SeqCst);
                                let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                                break;
                            }
                            StreamTag::Control => {
                                if String::from_utf8_lossy(&frame.payload)
                                    .contains("force_detached")
                                {
                                    disconnected.store(true, Ordering::SeqCst);
                                    let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                                    break;
                                }
                                // mid-session Control: 원격 resize 통지 / forward 회신.
                                // 알 수 없는 event(구/신 스키마)는 파싱 실패 → 무시(전방 호환).
                                let mirror_ev =
                                    match serde_json::from_slice::<StreamControl>(&frame.payload) {
                                        Ok(StreamControl::Resize {
                                            surface_id,
                                            cols,
                                            rows,
                                        }) => Some(MirrorEvent::Resize(surface_id, cols, rows)),
                                        Ok(StreamControl::Activity { surface_id, busy }) => {
                                            Some(MirrorEvent::Activity(surface_id, busy))
                                        }
                                        // 2단계: forward 실패 회신 → 실패 toast. 성공은 무음.
                                        Ok(StreamControl::StructuralResult {
                                            ok: false,
                                            reason,
                                            ..
                                        }) => Some(MirrorEvent::StructuralFailed(
                                            reason.unwrap_or_default(),
                                        )),
                                        // 3단계: 원격 구조 변경 역반영 → mirror 트리 재구성.
                                        Ok(StreamControl::StructuralDelta {
                                            workspace_id,
                                            tree,
                                            surfaces,
                                        }) => Some(MirrorEvent::StructuralDelta {
                                            workspace_id,
                                            tree,
                                            surfaces,
                                        }),
                                        // StreamControl 이 인식 못 하는 payload — (03)
                                        // capture_result 커스텀 이벤트인지 확인(별도
                                        // enum, StreamControl 비수정 — parse_capture_result 참조).
                                        Ok(_) | Err(_) => parse_capture_result(&frame.payload),
                                    };
                                if let Some(ev) = mirror_ev
                                    && let Ok(mut buf) = output.lock()
                                {
                                    buf.push(ev);
                                    drop(buf);
                                    let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                                }
                            }
                            StreamTag::Ping => {}
                        },
                        Err(_) => {
                            disconnected.store(true, Ordering::SeqCst);
                            let _ = proxy.send_event(AppEvent::AttachClientData); // event loop 종료 시에만 실패 — 무시
                            break;
                        }
                    }
                }
            });
        }

        self.attach_client_sessions.push(AttachClientSession {
            local_workspace: local_ws_id,
            remote_to_local,
            output,
            disconnected,
            writer,
            client_id,
            tunnel,
            anchor_ws_id,
            op_seq: 0,
            last_forwarded_resize: HashMap::new(),
        });
        tracing::info!(
            "gui attach: mirror workspace {local_ws_id} from 127.0.0.1:{port} (remote ws {workspace})"
        );
        Ok(local_ws_id)
    }

    /// 사용자 경로 전용 — 새 mirror workspace 로 focus 를 옮긴다(원격 워크스페이스 추가
    /// 팝업의 Connect 확정). mirror 를 호스팅한 창의 `active_workspace` 를 그 ws 인덱스로
    /// 설정한다. IPC/자동 attach 경로는 이 함수를 호출하지 않아 focus 중립을 유지한다.
    fn focus_mirror_workspace(&mut self, ws_id: u32) {
        for main in self.main_windows_iter_mut() {
            if let Some(idx) = main
                .core_state
                .workspaces
                .iter()
                .position(|ws| ws.id == ws_id)
            {
                main.state.active_workspace = idx;
                main.mark_dirty();
                break;
            }
        }
    }

    /// `AttachClientData`(reader wake)마다 — 누적 원격 출력을 mirror Terminal 에
    /// 적용(repaint) + 끊긴 세션 정리. client mirror 는 데이터가 오는 즉시 갱신한다
    /// (로컬 워크스페이스와 동일한 반응성). `AttachPoll` 3초 tick 도 backstop 으로 호출.
    pub(crate) fn apply_attach_client_output(&mut self) {
        if self.attach_client_sessions.is_empty() {
            return;
        }
        let mut dead: Vec<usize> = Vec::new();
        for idx in 0..self.attach_client_sessions.len() {
            let (drained, local_ws, disconnected) = {
                let sess = &self.attach_client_sessions[idx];
                let drained: Vec<MirrorEvent> = match sess.output.lock() {
                    Ok(mut b) => std::mem::take(&mut *b),
                    Err(_) => Vec::new(),
                };
                (
                    drained,
                    sess.local_workspace,
                    sess.disconnected.load(Ordering::SeqCst),
                )
            };

            if !drained.is_empty()
                && let Some(wid) = self.find_main_with_workspace(local_ws)
            {
                // 세션(remote→local 매핑)과 그 mirror 를 호스팅하는 창 engine 을 **분리
                // 대여**(self 의 서로 다른 필드 → disjoint borrow). delta 가 매핑을
                // 갱신하므로 clone 이 아닌 **라이브 매핑**을 써야 같은 drain 안의 이후
                // Data 가 새 surface 로 라우팅된다. 이벤트를 **도착 순서대로** 적용한다.
                let sess = &mut self.attach_client_sessions[idx];
                if let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) {
                    for ev in drained {
                        match ev {
                            MirrorEvent::Data(remote_id, bytes) => {
                                if let Some(&local) = sess.remote_to_local.get(&remote_id)
                                    && let Some(t) = main.core_state.terminals.get_mut(local)
                                {
                                    t.feed_bytes(&bytes);
                                }
                            }
                            MirrorEvent::Resize(remote_id, cols, rows) => {
                                // mirror 그리드를 원격 새 크기로 갱신. 로컬 resize
                                // 스윕은 detached mirror 를 건너뛰므로, 이 경로가
                                // mirror 를 리사이즈하는 유일한 지점이다.
                                if let Some(&local) = sess.remote_to_local.get(&remote_id)
                                    && let Some(t) = main.core_state.terminals.get_mut(local)
                                {
                                    t.resize(cols, rows);
                                }
                            }
                            MirrorEvent::Activity(remote_id, busy) => {
                                if let Some(&local) = sess.remote_to_local.get(&remote_id) {
                                    main.core_state.set_mirror_surface_busy(local, busy);
                                }
                            }
                            MirrorEvent::StructuralFailed(reason) => {
                                // forward 한 구조 op 가 원격에서 실패(예: 미등록 kind).
                                // 사용자에게 실패 toast. 로컬/원격 어느 쪽도 구조 변경
                                // 없음(요청/응답).
                                let base =
                                    crate::i18n::t("attach.toast.mirror_structural_forward_failed");
                                let msg: String = if reason.is_empty() {
                                    base.to_string()
                                } else {
                                    format!("{base} ({reason})")
                                };
                                main.state.toasts.push(
                                    msg,
                                    crate::adapters::ui::ToastKind::Warning,
                                    crate::adapters::ui::ToastScope::Window,
                                );
                            }
                            MirrorEvent::StructuralDelta {
                                workspace_id,
                                tree,
                                surfaces,
                            } => {
                                // 원격 구조 변경 역반영: survivor 터미널 local id 를
                                // 유지하며 mirror 트리를 재구성(신규 추가/사라진 것 제거).
                                apply_mirror_structural_delta(
                                    sess,
                                    main,
                                    workspace_id,
                                    &tree,
                                    &surfaces,
                                );
                            }
                            MirrorEvent::CaptureResult { ok, path, reason } => {
                                // (03) 원격이 이 세션의 캡처 업로드를 처리한 결과.
                                let msg = if ok {
                                    format!(
                                        "{} ({})",
                                        crate::i18n::t("attach.toast.mirror_capture_saved"),
                                        path.unwrap_or_default()
                                    )
                                } else {
                                    let base = crate::i18n::t("attach.toast.mirror_capture_failed");
                                    match reason {
                                        Some(r) if !r.is_empty() => format!("{base} ({r})"),
                                        _ => base.to_string(),
                                    }
                                };
                                let kind = if ok {
                                    crate::adapters::ui::ToastKind::Success
                                } else {
                                    crate::adapters::ui::ToastKind::Warning
                                };
                                main.state.toasts.push(
                                    msg,
                                    kind,
                                    crate::adapters::ui::ToastScope::Window,
                                );
                            }
                        }
                    }
                    main.mark_dirty();
                }
            }
            if disconnected {
                dead.push(idx);
            }
        }
        for &idx in dead.iter().rev() {
            let sess = self.attach_client_sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess);
        }
    }

    /// 끊긴(force-detach/EOF) 세션의 mirror workspace + mirror terminal 을 제거한다.
    /// 원칙 1①: 서버의 force-detach 가 client 의 *닫힌항목 히스토리/포커스* 를 건드리지
    /// 않게 — mirror workspace 만 제거하고 active index 만 클램프한다.
    fn cleanup_mirror_workspace(&mut self, sess: &AttachClientSession) {
        for main in self.main_windows_iter_mut() {
            let engine = &mut main.core_state;
            let Some(pos) = engine
                .workspaces
                .iter()
                .position(|ws| ws.id == sess.local_workspace)
            else {
                continue;
            };
            for &local in sess.remote_to_local.values() {
                engine.terminals.remove(local);
                engine.forget_mirror_surface_busy(local);
            }
            engine.workspaces.remove(pos);
            // active_workspace 인덱스 클램프(제거로 out-of-range 방지).
            let len = engine.workspaces.len().max(1);
            if main.state.active_workspace >= len {
                main.state.active_workspace = len - 1;
            } else if pos < main.state.active_workspace {
                main.state.active_workspace -= 1;
            }
            main.mark_dirty();
            break;
        }
        // 원격에 detach 통지(best-effort).
        if let Ok(mut w) = sess.writer.lock() {
            let _ = stream::write_frame(&mut *w, StreamTag::Detach, &[]); // best-effort detach 통지 — 종료 경로, 실패 무시
        }
        // 단계 7 — 자동 attach 였다면 anchor 게이트 해제(재활성 시 재attach 가능).
        if let Some(anchor) = sess.anchor_ws_id {
            self.auto_attach_active.remove(&anchor);
        }
        // 터널 핸들(sess.tunnel)은 여기서 Drop → 자식 ssh kill(고아 터널 방지).
    }

    /// `about_to_wait` 에서 호출 — 사용자가 mirror 워크스페이스 **자체를 닫으면**
    /// (context menu / 단축키 `close_workspace`) 로컬 워크스페이스는 즉시 사라지지만
    /// 그 워크스페이스를 mirror 하던 attach 세션은 남는다. 세션 소켓이 열린 채라
    /// 원격에 `Detach` 가 전달되지 않고 원격의 hard workspace 점유가 해제되지 않아
    /// 재연결 시 "사용 중"으로 남는다. 세션의 `local_workspace` 가 어느 창에도 없으면
    /// 고아로 보고 `cleanup_mirror_workspace` 로 정리한다 — `Detach` 통지 → 원격이
    /// `Disconnected` 로 점유 해제 + anchor 게이트 해제 + 터널 kill. disconnected
    /// (EOF/force-detach) 정리와 동형이되, 트리거가 **로컬 사용자 close** 인 경로다.
    /// 세션 push 는 항상 mirror workspace 생성(같은 동기 함수) 뒤라 attach 셋업 중
    /// false-positive 고아는 발생하지 않는다.
    pub(crate) fn detach_orphaned_mirror_sessions(&mut self) {
        if self.attach_client_sessions.is_empty() {
            return;
        }
        // (idx, local_workspace) 를 먼저 수집한 뒤 존재 여부를 조회 — iter 대여를
        // 들고 find_main_with_workspace(&self) 를 부르지 않도록 분리.
        let orphaned: Vec<usize> = self
            .attach_client_sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| (idx, s.local_workspace))
            .filter(|&(_, ws)| self.find_main_with_workspace(ws).is_none())
            .map(|(idx, _)| idx)
            .collect();
        for &idx in orphaned.iter().rev() {
            let sess = self.attach_client_sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess);
        }
    }

    /// `about_to_wait` 에서 호출 — `Core::apply` 가 mirror 워크스페이스 구조 op 를 쌓은
    /// forward 큐를 drain 해 원격에 전송한다(2단계). 각 op 의 anchor 로컬 surface id 를
    /// 세션 매핑으로 원격 id 로 치환한 뒤 attach stream 의 `StreamTag::Control` 로 보낸다.
    /// 로컬은 이미 mutation 이 차단됐고(요청/응답), 원격 실행 결과는 reader 가 받는
    /// `StructuralResult`(실패 시 toast)로 반영된다.
    pub(crate) fn dispatch_pending_structural_forwards(&mut self) {
        let mut pending: Vec<StructuralOp> = Vec::new();
        for main in self.main_windows_iter_mut() {
            pending.append(&mut main.core_state.pending_structural_forward);
        }
        if let Some(e) = self.core_state.as_mut() {
            pending.append(&mut e.pending_structural_forward);
        }
        for local_op in pending {
            self.forward_one_structural_op(local_op);
        }
    }

    /// forward 큐의 op 하나를 담당 mirror 세션으로 전송한다. anchor 로컬 surface 를 가진
    /// 세션을 찾아 local→remote 치환 후 `StructuralOp` 프레임을 write half 로 보낸다.
    /// 세션을 못 찾으면(예상 밖) warn 후 drop.
    fn forward_one_structural_op(&mut self, local_op: StructuralOp) {
        let local_anchor = local_op.anchor_surface_id();
        // anchor 로컬 surface 를 mirror 로 보유한 세션.
        let Some(sess) = self
            .attach_client_sessions
            .iter_mut()
            .find(|s| s.remote_to_local.values().any(|&l| l == local_anchor))
        else {
            tracing::warn!(
                "structural forward: mirror 세션이 로컬 surface {local_anchor} 를 갖지 않음 — drop"
            );
            return;
        };
        // local → remote anchor.
        let Some(remote_anchor) = sess
            .remote_to_local
            .iter()
            .find(|&(_, &l)| l == local_anchor)
            .map(|(&r, _)| r)
        else {
            tracing::warn!(
                "structural forward: 로컬 surface {local_anchor} 의 원격 id 없음 — drop"
            );
            return;
        };
        let wire = local_op.with_anchor_surface_id(remote_anchor);
        let op_id = sess.op_seq;
        sess.op_seq += 1;
        let payload = serde_json::to_vec(&StreamControl::StructuralOp { op_id, op: wire })
            .unwrap_or_default();
        match sess.writer.lock() {
            Ok(mut w) => {
                if let Err(e) = stream::write_frame(&mut *w, StreamTag::Control, &payload) {
                    tracing::warn!("structural forward send 실패: {e}");
                }
            }
            Err(_) => tracing::warn!("structural forward: writer lock 실패 — drop"),
        }
    }

    /// `about_to_wait` 에서 호출 — `Core::resize_all_terminals` 의 로컬 레이아웃
    /// 스윕이 mirror(detached) 터미널마다 쌓은 client-driven resize 큐를 drain 해
    /// 원격에 forward 한다(ADR-0045). 각 로컬 surface id 를 세션 매핑으로 원격 id 로
    /// 치환하고, 세션의 last-forwarded dedup 을 통과한 것만 `StreamControl::ClientResize`
    /// 로 보낸다. 로컬 mirror grid 는 여기서 건드리지 않는다 — server 의 `Resize`
    /// echo 가 유일한 갱신원(desync 방지).
    pub(crate) fn dispatch_pending_resize_forwards(&mut self) {
        let mut pending: Vec<(u32, usize, usize)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            for (sid, (cols, rows)) in main.core_state.pending_resize_forward.drain() {
                pending.push((sid, cols, rows));
            }
        }
        if let Some(e) = self.core_state.as_mut() {
            for (sid, (cols, rows)) in e.pending_resize_forward.drain() {
                pending.push((sid, cols, rows));
            }
        }
        for (local_sid, cols, rows) in pending {
            self.forward_one_resize(local_sid, cols, rows);
        }
    }

    /// resize 큐의 항목 하나를 담당 mirror 세션으로 전송한다. 로컬 mirror surface 를
    /// 보유한 세션을 찾아 local→remote 치환 후, 직전 전송값과 다르면
    /// `ClientResize` 프레임을 write half 로 보낸다(같으면 생략 — coalesce).
    /// 세션/원격 id 를 못 찾으면(예상 밖) warn 후 drop.
    fn forward_one_resize(&mut self, local_sid: u32, cols: usize, rows: usize) {
        let Some(sess) = self
            .attach_client_sessions
            .iter_mut()
            .find(|s| s.remote_to_local.values().any(|&l| l == local_sid))
        else {
            tracing::warn!(
                "resize forward: mirror 세션이 로컬 surface {local_sid} 를 갖지 않음 — drop"
            );
            return;
        };
        // local → remote surface id.
        let Some(remote_sid) = sess
            .remote_to_local
            .iter()
            .find(|&(_, &l)| l == local_sid)
            .map(|(&r, _)| r)
        else {
            tracing::warn!("resize forward: 로컬 surface {local_sid} 의 원격 id 없음 — drop");
            return;
        };
        // dedup: 직전 forward 와 같은 (cols, rows)면 재전송 생략(coalesce).
        if sess.last_forwarded_resize.get(&remote_sid) == Some(&(cols, rows)) {
            return;
        }
        let payload = serde_json::to_vec(&StreamControl::ClientResize {
            surface_id: remote_sid,
            cols,
            rows,
        })
        .unwrap_or_default();
        match sess.writer.lock() {
            Ok(mut w) => {
                if let Err(e) = stream::write_frame(&mut *w, StreamTag::Control, &payload) {
                    tracing::warn!("resize forward send 실패: {e}");
                    return;
                }
            }
            Err(_) => {
                tracing::warn!("resize forward: writer lock 실패 — drop");
                return;
            }
        }
        sess.last_forwarded_resize.insert(remote_sid, (cols, rows));
    }
}

/// 원격 surface 하나에 대응하는 mirror 터미널을 만들어 `engine` 에 삽입한다.
/// `Terminal::new_detached`(로컬 PTY 없음) + 입력 sink forwarder(로컬 키 입력 →
/// `encode_mux(remote_id)` → writer → 원격 PTY, 서버 holder+workspace 검증) + 옵저버
/// 게이트 초기화. 핸드셰이크(`start_gui_attach`)와 역반영(`apply_mirror_structural_delta`)이
/// 공유한다. 입력 forwarder 는 mirror drop(세션 정리/역반영 remove) 시 sink 채널이 끊겨
/// 자연 종료한다.
fn make_mirror_surface(
    remote_id: u32,
    local_id: u32,
    cols: usize,
    rows: usize,
    writer: &Arc<Mutex<TcpStream>>,
    engine: &mut crate::core::CoreState,
) {
    let mut mirror = Terminal::new_detached(cols, rows);
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    mirror.set_input_sink(tx);
    let fwd_writer = writer.clone();
    std::thread::spawn(move || {
        for chunk in rx {
            let framed = stream::encode_mux(remote_id, &chunk);
            let Ok(mut w) = fwd_writer.lock() else { break };
            if stream::write_frame(&mut *w, StreamTag::Data, &framed).is_err() {
                break;
            }
        }
    });
    // Mirror emit 은 process() 밖(feed_bytes)이라 process 진입의 lazy 게이트 동기화가
    // 닿지 않는다 — 옵저버가 먼저 등록된 경우를 위해 insert 시점에 게이트를 직접 초기화.
    mirror.set_output_events_enabled(engine.observer_router.wants(local_id));
    engine.terminals.insert(local_id, mirror);
}

/// 원격 구조 변경 delta(3단계 역반영)를 mirror 트리에 적용한다. 원격 ws 의 실행 후 전체
/// 트리+surfaces 를 받아:
/// 1. survivor(기존 매핑에 있는 remote_id)는 **기존 local id 를 재사용**(터미널을
///    재생성하지 않아 scrollback/grid 보존),
/// 2. 신규 remote surface 는 로컬 id 발급 + `make_mirror_surface`(터미널만),
/// 3. 사라진 것은 mirror 터미널 제거,
/// 4. 갱신된 매핑으로 `build_mirror_workspace` 재실행 → 같은 local ws id 로 교체.
///
/// pane 상위 배치는 `build_mirror_workspace` 의 기존 horizontal-chain 근사를 그대로
/// 승계한다(핸드셰이크와 동일 수준 — 3단계가 악화시키지 않음).
///
/// **focus 보존(수정 방향 B)**: 순수 pane/tab 전환(클릭·키보드 이동)은 forward 되는
/// StructuralOp 가 없어 원격의 `Workspace.focused_pane`/`Pane.active_tab` 은 갱신되지
/// 않는다(대개 워크스페이스 생성 시점의 첫 pane/첫 탭에 고정). 아래 4단계가 그 값을
/// 그대로 담은 delta 로 로컬 트리를 통째로 교체하면, 사용자가 로컬에서만 이동해둔
/// focus 가 매번 그 고정값으로 되돌아간다 — 이를 막기 위해 교체 **전** 로컬에서 실제로
/// focus 돼 있던 surface 를 remote id 기준으로 캡처해뒀다가, 교체 **후** 새 트리에서
/// 그 surface 를 찾아 focus 를 복원한다(서버 상태는 건드리지 않음 — client-only 보정).
fn apply_mirror_structural_delta(
    sess: &mut AttachClientSession,
    main: &mut crate::view::main::MainView,
    workspace_id: u32,
    tree: &Value,
    surfaces: &[Value],
) {
    let engine = &mut main.core_state;
    let ids = engine.next_ids.clone();

    // focus 캡처(교체 전) — 로컬에서 실제로 focus 돼 있던 surface 를, 재구성마다 바뀌는
    // local id 대신 안정적인 **remote id** 로 기억한다(옛 remote_to_local 기준).
    let old_focused_remote: Option<u32> = engine
        .workspaces
        .iter()
        .find(|w| w.id == sess.local_workspace)
        .and_then(|ws| capture_focused_remote(ws, &sess.remote_to_local));

    // 1·2. new_map(survivor 유지 + 신규 할당) + terminal_locals 수집.
    let mut new_map: HashMap<u32, u32> = HashMap::new();
    let mut terminal_locals: HashSet<u32> = HashSet::new();
    for s in surfaces {
        let remote_id = s.get("remote_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let is_terminal = s.get("role").and_then(|v| v.as_str()) == Some("terminal");
        let local_id = match sess.remote_to_local.get(&remote_id) {
            Some(&l) => l, // survivor — 기존 local id 재사용(터미널 유지).
            None => {
                let l = ids.next_surface();
                if is_terminal {
                    let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                    let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                    make_mirror_surface(remote_id, l, cols, rows, &sess.writer, engine);
                }
                l
            }
        };
        if is_terminal {
            terminal_locals.insert(local_id);
        }
        new_map.insert(remote_id, local_id);
    }

    // 3. removed — 기존 매핑 중 새 surfaces 에 없는 것: mirror 터미널 제거(입력
    //    forwarder 는 sink drop 으로 자연 종료).
    for (&remote_id, &local_id) in sess.remote_to_local.iter() {
        if !new_map.contains_key(&remote_id) {
            engine.terminals.remove(local_id);
            engine.forget_mirror_surface_busy(local_id);
        }
    }

    // 매핑 교체(이후 같은 drain 의 Data 는 갱신된 매핑으로 라우팅된다).
    sess.remote_to_local = new_map;

    // 4. 트리 재구성 → 같은 local ws id 로 in-place 교체(survivor local id 유지 →
    //    위치·구성만 갱신, active_workspace 인덱스 불변).
    if let Some(pos) = engine
        .workspaces
        .iter()
        .position(|w| w.id == sess.local_workspace)
    {
        let name = engine.workspaces[pos].name.clone();
        let mut ws = build_mirror_workspace(
            sess.local_workspace,
            &name,
            tree,
            &ids,
            &sess.remote_to_local,
            &terminal_locals,
        );
        ws.mirror = true;
        restore_focus_after_delta(&mut ws, old_focused_remote, &sess.remote_to_local);
        engine.workspaces[pos] = ws;
    } else {
        tracing::warn!(
            "structural delta: mirror workspace {} (remote {workspace_id}) 를 못 찾음 — drop",
            sess.local_workspace
        );
    }
}

/// delta 로 새로 만들어진 `ws` 에 `old_focused_remote`(교체 전 캡처한 remote surface
/// id)가 가리키던 위치로 focus 를 되돌린다. 캡처해둔 surface 가 새 트리에도 살아있으면
/// (이번 op 로 사라지지 않았으면) `ws.focused_pane`/해당 pane 의 `active_tab`/그 tab 의
/// `focused_surface` 를 그 위치로 맞춘다. surface 자체가 이번 op 로 없어졌으면(예:
/// 그 surface 를 닫은 CloseSurface) 억지로 복원하지 않고 `ws` 가 이미 담고 있는 원격
/// 값(= 원격의 고정 focused_pane/active_tab) 그대로 둔다.
fn restore_focus_after_delta(
    ws: &mut Workspace,
    old_focused_remote: Option<u32>,
    remote_to_local: &HashMap<u32, u32>,
) {
    let Some(remote_sid) = old_focused_remote else {
        return;
    };
    let Some(&new_local_sid) = remote_to_local.get(&remote_sid) else {
        return;
    };
    let Some((pane_id, tab_id)) = find_pane_and_tab_for_surface(ws, new_local_sid) else {
        return;
    };
    ws.focused_pane = pane_id;
    if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
        && let Some(tab_index) = pane.tabs.iter().position(|t| t.id == tab_id)
    {
        pane.active_tab = tab_index;
        pane.tabs[tab_index].focused_surface = new_local_sid;
    }
}

/// 현재 `ws`(교체되기 전의 mirror workspace)에서 실제로 focus 돼 있는 surface 를
/// **remote surface id** 로 찾아 반환한다(`remote_to_local` 역조회). local pane/tab/
/// surface id 는 매 delta 마다 재발급되어 안정적이지 않으므로, 여러 delta 를 거쳐도
/// 불변인 remote id 를 캡처의 기준으로 삼는다. focus 가 가리키는 surface 가 아직 이
/// 세션에 매핑되지 않았으면(예상 밖) `None`.
fn capture_focused_remote(ws: &Workspace, remote_to_local: &HashMap<u32, u32>) -> Option<u32> {
    let pane = ws.pane_layout().find_pane(ws.focused_pane)?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let local_sid = tab.focused_surface_id()?;
    remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
}

/// 주어진 workspace 안에서 `surface_id` 를 포함하는 (pane_id, tab_id) 를 찾는다.
/// `CoreState::find_pane_for_surface`/`find_tab_for_surface` 와 동형이지만 **단일
/// workspace 로 스코프를 좁힌** 버전 — `apply_mirror_structural_delta` 가 아직
/// `engine.workspaces` 에 삽입하기 **전의** 갓 만든 `Workspace` 값에도 바로 쓸 수
/// 있어야 하기 때문(engine 전체 순회 버전은 삽입 후에만 그 워크스페이스를 찾는다).
fn find_pane_and_tab_for_surface(ws: &Workspace, surface_id: u32) -> Option<(u32, u32)> {
    for pane_id in ws.pane_layout().all_pane_ids() {
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            continue;
        };
        for tab in &pane.tabs {
            if tab.contains_surface(surface_id) {
                return Some((pane_id, tab.id));
            }
        }
    }
    None
}

/// pane JSON(`{"id", "tabs":[...]}` — 평면 "panes" 원소/트리 Leaf 공용 shape)
/// → 로컬 `Pane`. 새 local pane id 발급 + 각 tab 의 layout(`build_layout`)/
/// focused_surface remote→local 매핑. `build_mirror_workspace`의 평면 fallback
/// 경로와 `build_pane_node`(트리 파서)가 공유한다.
fn build_pane_from_json(
    p: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
) -> Pane {
    let tabs_json = p
        .get("tabs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut tabs: Vec<Tab> = Vec::new();
    let mut active_tab = 0usize;
    for (i, t) in tabs_json.iter().enumerate() {
        let layout_json = t.get("layout").cloned().unwrap_or(Value::Null);
        let layout = build_layout(&layout_json, ids, map, term).unwrap_or_else(|| {
            SurfaceLayout::Leaf(Box::new(EmptySurface::new(ids.next_surface())))
        });
        let remote_focus = t
            .get("focused_surface")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;
        let focused_surface = map
            .get(&remote_focus)
            .copied()
            .or_else(|| layout.first_surface_id())
            .unwrap_or(0);
        let tab_name = t
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("Shell")
            .to_string();
        if t.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
            active_tab = i;
        }
        tabs.push(Tab {
            id: ids.next_tab(),
            name: tab_name,
            explicit_name: None,
            osc_title: None,
            layout_opt: Some(layout),
            focused_surface,
            cached_display_name: None,
        });
    }
    if tabs.is_empty() {
        let sid = ids.next_surface();
        tabs.push(Tab {
            id: ids.next_tab(),
            name: "Shell".to_string(),
            explicit_name: None,
            osc_title: None,
            layout_opt: Some(SurfaceLayout::Leaf(Box::new(EmptySurface::new(sid)))),
            focused_surface: sid,
            cached_display_name: None,
        });
    }
    Pane {
        id: ids.next_pane(),
        tabs,
        active_tab,
        tab_scroll_offset: 0.0,
    }
}

/// "pane_layout" JSON(`PaneNode::to_tree_json_full` shape) → `PaneNode`
/// (direction/ratio 보존). Leaf 파싱 시 (remote_pane_id → 신규 local pane id)를
/// `pane_id_map` 에 기록해, 호출부가 focused_pane remote→local 해석에 재사용한다
/// (트리 재귀 파서는 기존 `local_panes: Vec<(remote_id, Pane)>` 평면 리스트가
/// 없으므로 이 매핑이 그 대체 경로다).
fn build_pane_node(
    node: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    pane_id_map: &mut HashMap<u32, u32>,
) -> Option<PaneNode> {
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote_pane = node.get("id").and_then(|v| v.as_u64())? as u32;
            let pane = build_pane_from_json(node, ids, map, term);
            pane_id_map.insert(remote_pane, pane.id);
            Some(PaneNode::Leaf(pane))
        }
        "Split" => {
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("vertical") => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let first = build_pane_node(node.get("first")?, ids, map, term, pane_id_map)?;
            let second = build_pane_node(node.get("second")?, ids, map, term, pane_id_map)?;
            Some(PaneNode::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
        _ => None,
    }
}

/// 디스크립터 `tree`(`to_attach_tree_json`)로 로컬 mirror Workspace 를 재구성한다.
///
/// 신버전 서버는 `"pane_layout"` 트리 필드(direction/ratio 보존, `build_pane_node`)를
/// 실어 pane 상위 배치를 정확히 재현한다. 그 필드가 없는 구버전 서버는 평면 `"panes"`
/// 리스트만 보내므로, 다중 pane 을 horizontal split chain 으로 best-effort 재구성하는
/// 기존 fallback 을 그대로 유지한다. 각 pane 의 tab 별 `SurfaceLayout`(분할 방향/비율)은
/// 두 경로 모두 `to_tree_json_full` 로 보존돼 정확히 재현된다. remote leaf id 는 `map`
/// 으로 로컬 id 치환.
fn build_mirror_workspace(
    ws_id: u32,
    name: &str,
    tree: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
) -> Workspace {
    let remote_focused_pane = tree
        .get("focused_pane")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    // 신버전 서버: "pane_layout" 트리 필드로 direction/ratio 보존 파싱.
    if let Some(layout_json) = tree.get("pane_layout").filter(|v| !v.is_null()) {
        let mut pane_id_map = HashMap::new();
        if let Some(node) = build_pane_node(layout_json, ids, map, term, &mut pane_id_map) {
            let focused_local_pane = pane_id_map
                .get(&remote_focused_pane)
                .copied()
                .unwrap_or_else(|| node.first_pane().map(|p| p.id).unwrap_or(0));
            return Workspace::from_restored(
                ws_id,
                name.to_string(),
                String::new(),
                node,
                focused_local_pane,
            );
        }
        // "pane_layout" 이 있는데 파싱 실패(형태 불량) — 아래 구버전 fallback으로 흘려보냄.
    }

    // 구버전 fallback: 평면 "panes" 리스트 → horizontal chain(best-effort).
    let panes_json = tree
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut local_panes: Vec<(u32, Pane)> = Vec::new();
    for p in &panes_json {
        let remote_pane = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        local_panes.push((remote_pane, build_pane_from_json(p, ids, map, term)));
    }

    if local_panes.is_empty() {
        // 빈 트리 fallback — placeholder pane 1 개.
        let sid = ids.next_surface();
        let pane = Pane::new_with_surface(
            ids.next_pane(),
            ids.next_tab(),
            "Shell".to_string(),
            Box::new(EmptySurface::new(sid)),
        );
        let fp = pane.id;
        return Workspace::from_restored(
            ws_id,
            name.to_string(),
            String::new(),
            PaneNode::Leaf(pane),
            fp,
        );
    }

    let focused_local_pane = local_panes
        .iter()
        .find(|(rp, _)| *rp == remote_focused_pane)
        .map(|(_, p)| p.id)
        .unwrap_or(local_panes[0].1.id);

    // PaneNode: 1개=Leaf, 다중=horizontal split chain(best-effort — 구버전 서버는
    // pane 배치 정보를 안 보내므로 이 근사만 가능).
    let mut iter = local_panes.into_iter().map(|(_, p)| p);
    let mut node = PaneNode::Leaf(iter.next().unwrap());
    for p in iter {
        node = PaneNode::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(node),
            second: Box::new(PaneNode::Leaf(p)),
        };
    }

    Workspace::from_restored(
        ws_id,
        name.to_string(),
        String::new(),
        node,
        focused_local_pane,
    )
}

/// `to_tree_json_full` JSON → `SurfaceLayout`(분할 방향/비율/focus 보존). leaf 의 remote
/// id 는 `map` 으로 로컬 치환하고, 터미널이면 `TerminalSurface`(mirror grid 가 store 에
/// 있음), 아니면 placeholder `EmptySurface` leaf 로 만든다.
fn build_layout(
    node: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
) -> Option<SurfaceLayout> {
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote = node.get("id").and_then(|v| v.as_u64())? as u32;
            // map 에 없으면(예상 밖) 새 placeholder id 발급.
            let local = map
                .get(&remote)
                .copied()
                .unwrap_or_else(|| ids.next_surface());
            let surface: Box<dyn Surface> = if term.contains(&local) {
                Box::new(TerminalSurface { id: local })
            } else {
                Box::new(EmptySurface::new(local))
            };
            Some(SurfaceLayout::Leaf(surface))
        }
        "Split" => {
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("vertical") => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let focus_second = node
                .get("focus_second")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let first = build_layout(node.get("first")?, ids, map, term)?;
            let second = build_layout(node.get("second")?, ids, map, term)?;
            Some(SurfaceLayout::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
                focus_second,
            })
        }
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// (03) screenshot→remote-clipboard — mirror client 측 업로드 송신.
//
// 이 블록은 위 구조 op forward/역반영 로직(특히 `apply_mirror_structural_delta`)과
// 완전히 독립적이다 — 별도 기능(신규 03)이라 별도 impl 블록 + 전용 free fn 으로
// 분리해 둔다(병행 작업 merge 충돌 최소화).
// ─────────────────────────────────────────────────────────────────────────

/// 업로드 세션 식별자 시퀀스 — 프로세스 내 유일성만 필요(원격은 client_id 로도
/// 이미 세션이 구분되므로 재기동 간 유일성은 불필요).
static NEXT_CAPTURE_UPLOAD_ID: AtomicU64 = AtomicU64::new(1);

fn next_capture_upload_id() -> u64 {
    NEXT_CAPTURE_UPLOAD_ID.fetch_add(1, Ordering::Relaxed)
}

/// 한 청크의 raw payload 크기 상한. base64 인코딩(약 4/3 팽창) 후에도
/// `StreamTag::Control` 프레임의 `MAX_FRAME_LEN`(1MiB) 에 JSON 오버헤드를 포함해
/// 여유 있게 들어가도록 700KiB 로 잡는다(대부분의 스크린샷은 청크 1~2개).
const CAPTURE_CHUNK_RAW_LEN: usize = 700 * 1024;

/// `parse_capture_result`가 쓰는 wire shape. `StreamControl` enum 에는 없는
/// 이벤트라 별도로 직접 파싱한다.
#[derive(serde::Deserialize)]
struct CaptureResultWire {
    ok: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

/// `frame.payload` 가 (03) `capture_result` 커스텀 이벤트인지 확인해 `MirrorEvent`
/// 로 변환한다. `event` 필드가 다르거나 형태가 안 맞으면 `None`(다른 미지 이벤트와
/// 동일하게 조용히 무시 — 전방 호환).
fn parse_capture_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("capture_result") {
        return None;
    }
    let wire: CaptureResultWire = serde_json::from_value(value).ok()?;
    Some(MirrorEvent::CaptureResult {
        ok: wire.ok,
        path: wire.path,
        reason: wire.reason,
    })
}

impl App {
    /// (03) 캡처된 로컬 스크린샷을 `local_ws_id` mirror 세션의 attach 채널로
    /// 업로드하고, 완료 시 원격이 그 경로를 원격 클립보드에 쓰도록 요청한다.
    /// `StreamControl` enum(다른 worktree 가 동시 수정 중)은 건드리지 않고, 그
    /// enum 이 인식 못 하는 별도 "event" 값의 raw JSON 을 같은
    /// `StreamTag::Control` 채널에 실어 보낸다(파싱 실패 시 조용히 스킵되는
    /// 전방 호환 특성을 그대로 이용 — `stream_hub.rs`/서버측이 이를 받아 처리).
    pub(crate) fn forward_capture_to_remote_clipboard(
        &mut self,
        local_ws_id: u32,
        file_name: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .attach_client_sessions
            .iter()
            .find(|s| s.local_workspace == local_ws_id)
        else {
            anyhow::bail!("no attach session for mirror workspace {local_ws_id}");
        };
        let writer = sess.writer.clone();
        let upload_id = next_capture_upload_id();

        use base64::Engine as _;
        let chunks: Vec<&[u8]> = if bytes.is_empty() {
            vec![&[][..]]
        } else {
            bytes.chunks(CAPTURE_CHUNK_RAW_LEN).collect()
        };
        let total = chunks.len() as u32;
        for (seq, chunk) in chunks.into_iter().enumerate() {
            let msg = serde_json::json!({
                "event": "capture_chunk",
                "upload_id": upload_id,
                "seq": seq as u32,
                "total": total,
                "data_b64": base64::engine::general_purpose::STANDARD.encode(chunk),
            });
            send_capture_control_frame(&writer, &msg)?;
        }
        let commit = serde_json::json!({
            "event": "capture_commit",
            "upload_id": upload_id,
            "file_name": file_name,
        });
        send_capture_control_frame(&writer, &commit)
    }
}

/// (03) capture_chunk/capture_commit JSON 하나를 `StreamTag::Control` 프레임으로
/// 직렬화해 보낸다.
fn send_capture_control_frame(
    writer: &Arc<Mutex<TcpStream>>,
    msg: &serde_json::Value,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    let mut w = writer
        .lock()
        .map_err(|_| anyhow::anyhow!("attach writer lock poisoned"))?;
    stream::write_frame(&mut *w, StreamTag::Control, &payload)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::state::IdGenerator;

    /// to_tree_json_full → SurfaceLayout 재구성: 분할 방향/비율/focus 보존 +
    /// remote→local id 재매핑 + 터미널/placeholder kind 구분.
    #[test]
    fn build_layout_preserves_split_and_remaps_ids() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(100u32, 5u32); // 100 → local 5 (terminal)
        map.insert(101u32, 6u32); // 101 → local 6 (placeholder)
        let mut term = HashSet::new();
        term.insert(5u32);
        let node = serde_json::json!({
            "type": "Split",
            "direction": "vertical",
            "ratio": 0.3,
            "focus_second": true,
            "first": { "type": "Leaf", "id": 100, "kind": "terminal" },
            "second": { "type": "Leaf", "id": 101, "kind": "empty" },
        });
        let layout = build_layout(&node, &ids, &map, &term).expect("layout");
        match layout {
            SurfaceLayout::Split {
                direction,
                ratio,
                focus_second,
                first,
                second,
            } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert!((ratio - 0.3).abs() < 1e-6);
                assert!(focus_second);
                // remote id 가 로컬로 치환됐는지.
                assert_eq!(first.first_surface_id(), Some(5));
                assert_eq!(second.first_surface_id(), Some(6));
                // 터미널 vs placeholder kind.
                assert_eq!(first.find_surface(5).unwrap().kind(), "terminal");
                assert_ne!(second.find_surface(6).unwrap().kind(), "terminal");
            }
            _ => panic!("expected Split"),
        }
    }

    /// 단일 pane·tab 디스크립터 → mirror Workspace: 로컬 id 발급 + 트리 보존.
    #[test]
    fn build_mirror_workspace_single_pane_tab() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                    "layout": { "type": "Leaf", "id": 1, "kind": "terminal" }
                } ]
            } ]
        });
        let ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        assert_eq!(ws.id, 99);
        // mirror surface = 로컬 50 (remote 1 재매핑).
        assert_eq!(ws.all_surface_ids(), vec![50]);
    }

    /// 3단계 역반영의 핵심 계약: survivor(기존 매핑 remote_id)는 **기존 local id 를
    /// 유지**하고 신규 remote leaf 는 새 local id 로 트리에 삽입된다. `apply_mirror_
    /// structural_delta` 가 갱신하는 매핑을 그대로 재현해 `build_mirror_workspace` 에
    /// 넘겼을 때 survivor local id 가 보존되는지 검증한다(터미널 재생성 방지의 기반).
    #[test]
    fn build_mirror_workspace_preserves_survivor_and_inserts_new_leaf() {
        let ids = IdGenerator::new();
        // survivor: remote 1 → 기존 local 50(유지). 신규: remote 2 → 새 local 발급.
        let survivor_local = 50u32;
        let mut map = HashMap::new();
        map.insert(1u32, survivor_local);
        let new_local = ids.next_surface(); // 역반영이 신규에 발급하는 것과 동형.
        map.insert(2u32, new_local);
        let mut term = HashSet::new();
        term.insert(survivor_local);
        term.insert(new_local);
        // split 트리: survivor(remote 1) + 신규(remote 2).
        let tree = serde_json::json!({
            "id": 9, "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 1,
                    "layout": {
                        "type": "Split", "direction": "vertical", "ratio": 0.5,
                        "focus_second": false,
                        "first": { "type": "Leaf", "id": 1, "kind": "terminal" },
                        "second": { "type": "Leaf", "id": 2, "kind": "terminal" }
                    }
                } ]
            } ]
        });
        let ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        let sids = ws.all_surface_ids();
        assert!(
            sids.contains(&survivor_local),
            "survivor local id({survivor_local}) 가 유지돼야 한다: {sids:?}"
        );
        assert!(
            sids.contains(&new_local),
            "신규 leaf local id({new_local}) 가 트리에 삽입돼야 한다: {sids:?}"
        );
        assert_eq!(sids.len(), 2, "survivor + 신규 = 2개 leaf");
    }

    /// 빈/널 트리 fallback — panic 없이 placeholder workspace.
    #[test]
    fn build_mirror_workspace_empty_tree_fallback() {
        let ids = IdGenerator::new();
        let map = HashMap::new();
        let term = HashSet::new();
        let ws = build_mirror_workspace(1, "remote", &serde_json::Value::Null, &ids, &map, &term);
        assert_eq!(ws.id, 1);
        assert_eq!(ws.all_surface_ids().len(), 1);
    }

    /// pane_layout 필드가 있으면 direction/ratio/focused_pane 이 정확히 복원돼야 한다
    /// (이번 버그의 핵심 회귀 테스트).
    #[test]
    fn build_mirror_workspace_preserves_vertical_pane_split() {
        let ids = IdGenerator::new();
        let map = HashMap::new(); // 이 테스트는 focused_surface 매핑 불필요(pane 레벨 검증 목적)
        let term = HashSet::new();
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 8,
            "panes": [],
            "pane_layout": {
                "type": "Split",
                "direction": "vertical",
                "ratio": 0.3,
                "first": { "type": "Leaf", "id": 7, "tabs": [] },
                "second": { "type": "Leaf", "id": 8, "tabs": [] }
            }
        });
        let ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        match ws.pane_layout() {
            PaneNode::Split {
                direction,
                ratio,
                second,
                ..
            } => {
                assert_eq!(*direction, SplitDirection::Vertical);
                assert!((*ratio - 0.3).abs() < 0.001);
                // focused_pane(remote 8) 이 second(새로 발급된 로컬 id)로 매핑됐는지.
                if let PaneNode::Leaf(p) = second.as_ref() {
                    assert_eq!(ws.focused_pane, p.id);
                } else {
                    panic!("expected second to be Leaf");
                }
            }
            _ => panic!("expected Split, got Leaf"),
        }
    }

    /// pane_layout 필드가 없으면(구버전 서버) 기존 horizontal-chain fallback 이
    /// 그대로 동작해야 한다(하위호환 회귀 검증 — 기존 3개 테스트와 별개로, "필드 부재"
    /// 그 자체를 명시적으로 검증).
    #[test]
    fn build_mirror_workspace_falls_back_to_horizontal_chain_without_pane_layout_field() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 2,
            "panes": [
                { "id": 1, "tabs": [ { "id": 3, "name": "Shell", "active": true,
                    "focused_surface": 1, "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } } ] },
                { "id": 2, "tabs": [ { "id": 4, "name": "Shell", "active": true,
                    "focused_surface": 2, "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } } ] }
            ]
            // "pane_layout" 필드 없음 — 구버전 서버 흉내.
        });
        let ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        match ws.pane_layout() {
            PaneNode::Split {
                direction, ratio, ..
            } => {
                assert_eq!(*direction, SplitDirection::Horizontal);
                assert!((*ratio - 0.5).abs() < 1e-6);
            }
            _ => panic!("expected Split (2 panes → horizontal chain fallback)"),
        }
    }

    /// pane B, tab2 의 surface(local 52)를 담은 workspace 를 만들어 `capture_focused_remote`
    /// 가 **remote id 3**(local 52)을 정확히 되짚어내는지 검증한다(TODO
    /// 01-mirror-workspace-focus-jump 원인 분석의 "1. 캡처" 단계).
    #[test]
    fn capture_focused_remote_finds_remote_id_of_locally_focused_surface() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32); // pane A 의 surface
        map.insert(2u32, 51u32); // pane B, tab1 의 surface
        map.insert(3u32, 52u32); // pane B, tab2 의 surface — 사용자가 보고 있는 곳
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        term.insert(52u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 11,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        assert_eq!(
            capture_focused_remote(&ws, &map),
            Some(3),
            "focused_pane=pane B, active_tab=tab2(remote 3) 를 정확히 되짚어야 한다"
        );
    }

    /// 핵심 회귀 테스트(TODO 01-mirror-workspace-focus-jump): 클라이언트가 로컬에서만
    /// pane B 의 두 번째 탭으로 이동해둔 상태에서 구조 변경 delta 가 도착하면(원격의
    /// focused_pane 은 forward 되는 순수 focus op 가 없어 항상 최초 pane=pane A 로
    /// 고정), 패치 전에는 재구성된 트리가 pane A(첫 pane)로 focus 를 되돌렸다.
    /// `capture_focused_remote`(교체 전) → `restore_focus_after_delta`(교체 후) 조합이
    /// 실제 `apply_mirror_structural_delta` 가 쓰는 것과 동일한 복원 로직이다.
    #[test]
    fn focus_restore_keeps_client_on_pane_b_after_structural_delta_from_pane_a() {
        let ids = IdGenerator::new();

        // "before": 원격의 focused_pane 은 최초 pane A(10) 에 고정. 사용자는 로컬에서
        // pane B(11) 의 두 번째 탭(remote 3)으로 이동해 있다.
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        map.insert(3u32, 52u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        term.insert(52u32);
        let before_tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let mut before_ws = build_mirror_workspace(99, "remote", &before_tree, &ids, &map, &term);

        // 사용자가 로컬에서 pane B, tab2 로 이동한다 — 순수 클릭/키보드 네비게이션이라
        // 원격에는 아무것도 forward 되지 않는다(버그의 1번 원인). 서버가 선언한
        // focused_pane(10, pane A)과는 별개로, 클라이언트의 실제 로컬 focus 만 바뀐다.
        let pane_b_surface3_local = *map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&before_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist");
        before_ws.focused_pane = pane_b_id;
        let pane_b = before_ws
            .pane_layout_mut()
            .find_pane_mut(pane_b_id)
            .expect("pane B exists");
        let tab_index = pane_b
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .expect("tab exists");
        pane_b.active_tab = tab_index;
        pane_b.tabs[tab_index].focused_surface = pane_b_surface3_local;

        let old_focused_remote = capture_focused_remote(&before_ws, &map);
        assert_eq!(old_focused_remote, Some(3));

        // "after": pane A 에 새 탭(remote 4)이 background 로 추가된 구조 변경 delta.
        // 원격의 focused_pane 은 여전히 pane A(10) — 버그의 근본 원인 그대로 재현.
        let mut after_map = map.clone();
        after_map.insert(4u32, 53u32);
        term.insert(53u32);
        let after_tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } },
                    { "id": 101, "name": "Shell", "active": false, "focused_surface": 4,
                      "layout": { "type": "Leaf", "id": 4, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": false, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } },
                    { "id": 111, "name": "Shell", "active": true, "focused_surface": 3,
                      "layout": { "type": "Leaf", "id": 3, "kind": "terminal" } }
                ] }
            }
        });
        let mut after_ws =
            build_mirror_workspace(99, "remote", &after_tree, &ids, &after_map, &term);

        // 대조군 — 복원 없이 그대로 두면 pane A(원격의 고정값)에 focus 가 있다(버그 재현).
        let pane_a_local_surface = *after_map.get(&1).unwrap();
        let (pane_a_id, _) = find_pane_and_tab_for_surface(&after_ws, pane_a_local_surface)
            .expect("pane A surface must exist in rebuilt tree");
        assert_eq!(
            after_ws.focused_pane, pane_a_id,
            "패치 전이라면 재구성 직후 focus 는 항상 pane A(원격 고정값)"
        );

        // 수정된 복원 로직 적용.
        restore_focus_after_delta(&mut after_ws, old_focused_remote, &after_map);

        let pane_b_surface3_local = *after_map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&after_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist in rebuilt tree");
        assert_eq!(
            after_ws.focused_pane, pane_b_id,
            "복원 후 focus 는 pane A 가 아니라 사용자가 실제로 보던 pane B 에 있어야 한다"
        );
        let pane_b = after_ws
            .pane_layout()
            .find_pane(pane_b_id)
            .expect("pane B exists");
        assert_eq!(
            pane_b.tabs[pane_b.active_tab].id, tab_id,
            "pane B 의 active_tab 도 사용자가 보던 두 번째 탭이어야 한다"
        );
        assert_eq!(
            pane_b.tabs[pane_b.active_tab].focused_surface, pane_b_surface3_local,
            "그 탭의 focused_surface 도 정확히 그 surface 를 가리켜야 한다"
        );
    }

    /// 캡처해둔 surface 자체가 이번 op 로 사라졌으면(예: CloseSurface 로 그 surface 를
    /// 직접 닫음) 억지로 복원하지 않고 원격이 보낸 값 그대로 둬야 한다(무리한 복원 방지).
    #[test]
    fn focus_restore_is_noop_when_captured_surface_no_longer_exists() {
        let ids = IdGenerator::new();
        let mut map = HashMap::new();
        map.insert(1u32, 50u32);
        map.insert(2u32, 51u32);
        let mut term = HashSet::new();
        term.insert(50u32);
        term.insert(51u32);
        let tree = serde_json::json!({
            "id": 9, "name": "remote", "focused_pane": 10,
            "panes": [],
            "pane_layout": {
                "type": "Split", "direction": "horizontal", "ratio": 0.5,
                "first": { "type": "Leaf", "id": 10, "tabs": [
                    { "id": 100, "name": "Shell", "active": true, "focused_surface": 1,
                      "layout": { "type": "Leaf", "id": 1, "kind": "terminal" } }
                ] },
                "second": { "type": "Leaf", "id": 11, "tabs": [
                    { "id": 110, "name": "Shell", "active": true, "focused_surface": 2,
                      "layout": { "type": "Leaf", "id": 2, "kind": "terminal" } }
                ] }
            }
        });
        let mut ws = build_mirror_workspace(99, "remote", &tree, &ids, &map, &term);
        let untouched_focused_pane = ws.focused_pane;

        // 캡처된 surface(remote 3)는 이 map/tree 어디에도 없다 — 이미 닫힌 상태를 흉내.
        restore_focus_after_delta(&mut ws, Some(3), &map);

        assert_eq!(
            ws.focused_pane, untouched_focused_pane,
            "캡처된 surface 가 없으면 원격이 보낸 focused_pane 그대로 둬야 한다"
        );
    }
}
