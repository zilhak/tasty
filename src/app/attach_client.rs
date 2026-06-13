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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

use tasty_cli::stream::StreamConnection;
use tasty_terminal::Terminal;

use crate::AppEvent;
use crate::app::App;
use crate::ipc::stream::{self, STREAM_PROTO, StreamTag};
use crate::model::{
    EmptySurface, Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, TerminalSurface,
    Workspace,
};
use crate::view::ui::View as _;

/// reader thread 가 누적하고 메인 스레드의 apply 가 drain 하는 원격 출력 버퍼.
/// 항목은 `(remote_surface_id, bytes)`.
pub(crate) type RemoteOutputBuffer = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

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
    #[allow(dead_code)]
    client_id: u32,
    /// 단계 7 — 자동 attach 의 SSH 터널 핸들. 세션이 살아있는 동안 보관해 Drop(자식
    /// ssh kill)을 막는다. 수동 트리거(`attach.into_gui`)·loopback 은 None.
    #[allow(dead_code)]
    tunnel: Option<tasty_cli::ssh::SshTunnel>,
    /// 단계 7 — 이 mirror 를 띄운 매핑된(anchor) 로컬 워크스페이스 id. 세션 정리 시
    /// `auto_attach_active` 에서 제거해 재활성 시 재attach 가능하게 한다. 수동 None.
    anchor_ws_id: Option<u32>,
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
        for (port, workspace) in reqs {
            if let Err(e) = self.start_gui_attach(port, workspace, None, None) {
                tracing::warn!("gui attach failed (port={port}, ws={workspace}): {e}");
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
    pub(crate) fn start_gui_attach(
        &mut self,
        port: u16,
        workspace: u32,
        tunnel: Option<tasty_cli::ssh::SshTunnel>,
        anchor_ws_id: Option<u32>,
    ) -> anyhow::Result<()> {
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
                    let mut mirror = Terminal::new_detached(cols, rows);
                    // 입력 forward: focus mirror 로의 send_bytes → sink → encode_mux →
                    // 원격 PTY(서버 holder+workspace 검증). keyboard.rs 무변경.
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
                    // Mirror emit 은 process() 밖(feed_bytes)이라 process 진입의
                    // lazy 게이트 동기화가 닿지 않는다 — 옵저버가 먼저 등록된
                    // 경우를 위해 insert 시점에 게이트를 직접 초기화한다.
                    mirror.set_output_events_enabled(engine.observer_router.wants(local_id));
                    engine.terminals.insert(local_id, mirror);
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
            // client mirror 표식 — 사이드바 dot 을 항상 하늘색으로 표시(로컬 ws 와 구분).
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
                                    buf.push((sid, payload.to_vec()));
                                }
                                // 실시간 갱신: 데이터가 오는 즉시 메인 루프를 깨워 mirror 에
                                // 적용한다(로컬 PTY 의 TerminalOutput wake 와 동형).
                                let _ = proxy.send_event(AppEvent::AttachClientData);
                            }
                            StreamTag::Detach => {
                                disconnected.store(true, Ordering::SeqCst);
                                let _ = proxy.send_event(AppEvent::AttachClientData);
                                break;
                            }
                            StreamTag::Control => {
                                if String::from_utf8_lossy(&frame.payload)
                                    .contains("force_detached")
                                {
                                    disconnected.store(true, Ordering::SeqCst);
                                    let _ = proxy.send_event(AppEvent::AttachClientData);
                                    break;
                                }
                            }
                            StreamTag::Ping => {}
                        },
                        Err(_) => {
                            disconnected.store(true, Ordering::SeqCst);
                            let _ = proxy.send_event(AppEvent::AttachClientData);
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
        });
        tracing::info!(
            "gui attach: mirror workspace {local_ws_id} from 127.0.0.1:{port} (remote ws {workspace})"
        );
        Ok(())
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
                let drained: Vec<(u32, Vec<u8>)> = match sess.output.lock() {
                    Ok(mut b) => std::mem::take(&mut *b),
                    Err(_) => Vec::new(),
                };
                (
                    drained,
                    sess.local_workspace,
                    sess.disconnected.load(Ordering::SeqCst),
                )
            };

            if !drained.is_empty() {
                // remote→local 매핑은 세션 보유. mirror terminal 이 있는 main view 에 feed.
                let map = self.attach_client_sessions[idx].remote_to_local.clone();
                for main in self.main_windows_iter_mut() {
                    if main
                        .core_state
                        .workspaces
                        .iter()
                        .any(|ws| ws.id == local_ws)
                    {
                        for (remote_id, bytes) in &drained {
                            if let Some(&local) = map.get(remote_id)
                                && let Some(t) = main.core_state.terminals.get_mut(local)
                            {
                                t.feed_bytes(bytes);
                            }
                        }
                        main.mark_dirty();
                        break;
                    }
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
            let _ = stream::write_frame(&mut *w, StreamTag::Detach, &[]);
        }
        // 단계 7 — 자동 attach 였다면 anchor 게이트 해제(재활성 시 재attach 가능).
        if let Some(anchor) = sess.anchor_ws_id {
            self.auto_attach_active.remove(&anchor);
        }
        // 터널 핸들(sess.tunnel)은 여기서 Drop → 자식 ssh kill(고아 터널 방지).
    }
}

/// 디스크립터 `tree`(`to_attach_tree_json`)로 로컬 mirror Workspace 를 재구성한다.
///
/// pane 단위 분할 배치는 디스크립터에 없으므로(`to_attach_tree_json` 이 pane 을 평면
/// 리스트로 emit) 다중 pane 은 horizontal split chain 으로 best-effort 재구성한다.
/// 각 pane 의 tab 별 `SurfaceLayout`(분할 방향/비율)은 `to_tree_json_full` 로 보존돼
/// 정확히 재현된다. remote leaf id 는 `map` 으로 로컬 id 치환.
fn build_mirror_workspace(
    ws_id: u32,
    name: &str,
    tree: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
) -> Workspace {
    let panes_json = tree
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let remote_focused_pane = tree
        .get("focused_pane")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let mut local_panes: Vec<(u32, Pane)> = Vec::new();
    for p in &panes_json {
        let remote_pane = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
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
        local_panes.push((
            remote_pane,
            Pane {
                id: ids.next_pane(),
                tabs,
                active_tab,
                tab_scroll_offset: 0.0,
            },
        ));
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

    // PaneNode: 1개=Leaf, 다중=horizontal split chain(best-effort — 배치 정보 부재).
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
}
