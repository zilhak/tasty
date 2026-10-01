//! 원격 workspace를 로컬 mirror 트리로 표시하고 입력을 원격으로 전달한다.
//! 수신 이벤트와 주기 확인에서 출력을 적용한다. 창 없는 parked engine도 적용·정리에 포함한다.
//! 자동 연결 매핑은 auto_attach가 관리한다. docs/dev-guide/attach-behavior.md 참조.

mod agent_origin;
mod dispatch;
#[cfg(test)]
mod navigation_tests;

use crate::runtime::engine_access::EngineMut;
use dispatch::{AttachSource, Outcome, dispatch_attach};
use crate::remote::client_session::*;
use crate::remote::transport::*;

use std::collections::{HashMap, HashSet};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use winit::event_loop::EventLoopProxy;

use crate::ipc::client::StreamConnection;
use tasty_terminal::Terminal;

use crate::AppEvent;
use crate::app::App;
use crate::app::window_access::{EngineScanMut, engines_mut};
use crate::ipc::stream::{self, STREAM_PROTO, StreamControl, StreamTag, StructuralOp};
use crate::model::{
    DeferredPlugin, EmptySurface, ExplorerPanel, Pane, PaneNode, SplitDirection, Surface,
    SurfaceLayout, Tab, TerminalSurface, Workspace,
};
use crate::runtime::engine_session::EngineId;
use crate::view::ui::View as _;

/// 번들 git-viewer 매니페스트의 ID와 일치해야 한다.
const GIT_VIEWER_PLUGIN_ID: &str = "com.tasty.git-viewer";
const GIT_VIEWER_QUERY_RESULT_EVENT: &str = "git_viewer.query_result";

/// 서버의 is_attach_content_allowed와 같은 kind·소유자 쌍만 로컬 문서로 만든다.
const MARKDOWN_MIRROR_KIND: &str = "markdown";
const MARKDOWN_PLUGIN_ID: &str = "com.tasty.markdown";
const MARKDOWN_MIRROR_CONTENT_RESULT_EVENT: &str = "markdown_mirror.content_result";
const MARKDOWN_MIRROR_CHANGED_EVENT: &str = "markdown_mirror.changed";

/// mesh leaf를 AttachMeshSurface로 만들 때 필요한 표시 정보.
#[derive(Debug, Clone)]
struct MirrorMeshInfo {
    kind: String,
    plugin_id: String,
    display_name: String,
}


impl App {
    pub(crate) fn dispatch_pending_gui_attach(&mut self) {
        let mut reqs: Vec<(u16, u32)> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            reqs.append(&mut engine.remote.pending_gui_attach);
        }
        for (port, workspace) in reqs {
            self.try_dispatch_one_gui_attach_ipc(port, workspace);
        }

        // 사용자 요청만 성공 후 포커스를 이동한다. IPC 요청과 큐를 나눈다.
        let mut user_reqs: Vec<crate::core::GuiAttachUserReq> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            user_reqs.append(&mut engine.remote.pending_gui_attach_user);
        }
        for req in user_reqs {
            self.try_dispatch_one_gui_attach_user(req);
        }
    }

    fn try_dispatch_one_gui_attach_ipc(&mut self, port: u16, workspace: u32) {
        let own_port = self.hub.ipc_server.as_ref().map(|s| s.port());
        if let Outcome::Connected(Err(e)) =
            dispatch_attach(own_port, port, workspace, AttachSource::Ipc, || {
                self.start_gui_attach(port, workspace, None, None)
            })
        {
            tracing::warn!("gui attach failed (port={port}, ws={workspace}): {e}");
        }
    }

    fn try_dispatch_one_gui_attach_user(&mut self, req: crate::core::GuiAttachUserReq) {
        let own_port = self.hub.ipc_server.as_ref().map(|s| s.port());
        match dispatch_attach(
            own_port,
            req.port,
            req.workspace,
            AttachSource::User,
            || self.start_gui_attach(req.port, req.workspace, req.tunnel, None),
        ) {
            Outcome::RejectedSelf => {}
            Outcome::Connected(Ok(ws_id)) => self.focus_mirror_workspace(ws_id),
            Outcome::Connected(Err(e)) => tracing::warn!(
                "remote-attach failed (port={}, ws={}): {e}",
                req.port,
                req.workspace
            ),
        }
    }

    /// 원격 workspace를 로컬 mirror로 만들고 로컬 workspace ID를 반환한다.
    /// 연결과 핸드셰이크는 여기서 동기로 수행한다. SSH 터널 준비는 호출자가 마친다.
    pub(crate) fn start_gui_attach(
        &mut self,
        port: u16,
        workspace: u32,
        tunnel: Option<tasty_ssh::SshTunnel>,
        anchor_ws_id: Option<u32>,
    ) -> anyhow::Result<u32> {
        let (conn, client_id, write_half, name, surfaces, tree) =
            attach_handshake(port, workspace, "gui attach")?;

        let (frame_tx,frame_rx)=crate::remote::connection::channel();
        // 재연결해도 입력 forwarder가 새 sender를 볼 수 있도록 공유 핸들을 유지한다.
        

        let local_ws_id;
        let proxy;
        let remote_to_local: HashMap<u32, u32>;
        let markdown_locals: HashSet<u32>;
        let mut structure_ids = MirrorStructureIds::default();
        {
            let Some((main, mut engine)) = self.focused_pair_mut() else {
                anyhow::bail!("no focused window to host mirror workspace");
            };
            proxy = main.proxy.clone();
            let ids = engine.next_ids.clone();

            let mut mapping =
                merge_survivor_mapping(&HashMap::new(), &surfaces, &ids, &frame_tx, &mut engine);
            markdown_locals = mapping.markdown_ids();
            remote_to_local = std::mem::take(&mut mapping.remote_to_local);

            local_ws_id = ids.next_workspace();
            let mut ws = build_mirror_workspace(
                &mut structure_ids,
                &mut main.state.navigation,
                local_ws_id,
                &name,
                &tree,
                &ids,
                &remote_to_local,
                &mapping.terminals,
                &mapping.mesh,
                &mapping.explorer,
                &mut mapping.markdown,
            );
            install_mirror_fallbacks(&ws,&mut engine);
            ws.mirror=true;
            engine.push_mirror_workspace(ws);
            main.state.reconcile_presentation(engine.core);
            main.mark_dirty();
        }

        let output = MirrorOutbox::new(frame_tx.epoch());
        let disconnected = Arc::new(AtomicBool::new(false));

        spawn_attach_write_thread(
            write_half,
            frame_rx,
            disconnected.clone(),
            attach_wake(&proxy),
            "",
        );

        spawn_attach_reader_thread(
            conn,
            output.clone(),
            disconnected.clone(),
            attach_wake(&proxy),
            local_ws_id,
            "",
            mirror_event_from_control,
        );

        // heartbeat는 이 연결의 sender와 종료 신호를 사용한다. 재연결 시 새 스레드를 만든다.
        let raw_frame_tx =
            frame_tx.clone();
        spawn_attach_heartbeat_thread(raw_frame_tx, disconnected.clone());

        self.remote.sessions.push(AttachClientSession {
            transport:ClientTransport {output,disconnected,frame_tx,tunnel},
            state:ClientSessionState {
            structure_ids,
            local_workspace: local_ws_id,
            remote_to_local,
            phase:SessionState::Connected,
            client_id,
            remote_workspace: workspace,
            bulk_port: port,
            anchor_ws_id,
            op_seq: 0,
            pending_op_focus: HashMap::new(),
            agent_requests: Default::default(),
            next_delta_focus: None,
            last_forwarded_resize: HashMap::new(),
            remote_label: format!("127.0.0.1:{port}"),
            pending_list_dir_consumers: HashMap::new(),
            markdown_locals,
            resync_pending: None,
            resync_awaiting_window: false,
            }
        });
        tracing::info!(
            "gui attach: mirror workspace {local_ws_id} from 127.0.0.1:{port} (remote ws {workspace})"
        );
        Ok(local_ws_id)
    }

    /// 기존 mirror의 로컬 ID·scrollback·포커스를 보존하며 새 연결의 구조를 반영한다.
    /// 입력 forwarder는 공유 sender만 바꾸고 reader·writer·heartbeat는 다시 만든다.
    pub(crate) fn reconnect_session(
        &mut self,
        sess_idx: usize,
        port: u16,
        tunnel: Option<tasty_ssh::SshTunnel>,
    ) -> anyhow::Result<()> {
        let (workspace, local_workspace) = {
            let sess = &self.remote.sessions[sess_idx];
            (sess.state.remote_workspace, sess.state.local_workspace)
        };

        let (conn, client_id, write_half, name, surfaces, tree) =
            attach_handshake(port, workspace, "gui reconnect")?;

        // Retiring the prior epoch rejects its queued frames; survivors receive a new sink.
        let (shared_frame_tx,frame_rx)=crate::remote::connection::channel();

        let Some(wid) = self.find_main_with_workspace(local_workspace) else {
            anyhow::bail!(
                "mirror workspace {local_workspace} 가 더 이상 어느 창에도 없음 — 재연결 취소"
            );
        };
        let proxy;
        let removed_markdown: Vec<u32>;
        {
            let sess = &mut self.remote.sessions[sess_idx];
            sess.transport.frame_tx.retire();
            sess.transport.disconnected.store(true,Ordering::Release);
            sess.transport.frame_tx=shared_frame_tx.clone();

            let Some((main, mut engine)) = engines_mut!(self).window_pair(wid) else {
                anyhow::bail!("window {wid:?} 가 더 이상 MainView 가 아님 — 재연결 취소");
            };
            proxy = main.proxy.clone();
            let ids = engine.next_ids.clone();

            let old_focused_remote: Option<u32> = engine
                .workspaces()
                .into_iter()
                .find(|w| w.id == local_workspace)
                .and_then(|ws| {
                    capture_focused_remote(&main.state.navigation, ws, &sess.state.remote_to_local)
                });

            engine.remote.discard_connection_requests(&sess.state.remote_to_local,sess.state.local_workspace);
            let mut mapping = merge_survivor_mapping(
                &sess.state.remote_to_local,
                &surfaces,
                &ids,
                &shared_frame_tx,
                &mut engine,
            );
            sess.state.remote_to_local = std::mem::take(&mut mapping.remote_to_local);
            // 연결 사이에 빠진 출력을 연속된 스트림으로 읽지 않도록 표지를 바꾼다.
            for (&remote,&local) in &sess.state.remote_to_local {
                if let Some(terminal)=engine.runtime.terminals.get_mut(local) {
                    bind_mirror_input(terminal,remote,&shared_frame_tx,true);
                    terminal.renew_output_stream();
                }
            }
            // 옛 연결의 mesh 캐시와 구독 기록을 비워 full texture를 다시 요청한다.
            for &local in mapping.mesh.keys() {
                engine.remote.attach_mesh_frames.remove(local);
                main.attach_mesh_input.remove(&local);
            }
            let new_markdown = mapping.markdown_ids();
            removed_markdown = sess
                .markdown_locals
                .difference(&new_markdown)
                .copied()
                .collect();
            sess.state.markdown_locals = new_markdown;

            let Some(_) = engine
                .workspaces()
                .into_iter()
                .position(|w| w.id == local_workspace)
            else {
                anyhow::bail!(
                    "mirror workspace {local_workspace} 를 engine 에서 못 찾음 — 재연결 취소"
                );
            };
            let mut ws = build_mirror_workspace(
                &mut sess.state.structure_ids,
                &mut main.state.navigation,
                local_workspace,
                &name,
                &tree,
                &ids,
                &sess.state.remote_to_local,
                &mapping.terminals,
                &mapping.mesh,
                &mapping.explorer,
                &mut mapping.markdown,
            );
            install_mirror_fallbacks(&ws,&mut engine);
            ws.mirror=true;
            if !restore_focus_after_delta(
                &mut main.state.navigation,
                &mut ws,
                old_focused_remote,
                &sess.state.remote_to_local,
            ) {
                tracing::info!(
                    "gui reconnect: 이전 focus surface 를 재연결 후 트리에서 찾지 못함 — 원격 기본 focus 유지"
                );
            }
            engine
                .replace_mirror_workspace(ws)
                .unwrap_or_else(|_| panic!("mirror workspace disappeared"));
            main.state.reconcile_presentation(engine.core);
            main.state.toasts.push(
                crate::i18n::t("attach.toast.mirror_reconnected").to_string(),
                crate::adapters::ui::ToastKind::Success,
                crate::adapters::ui::ToastScope::Window,
            );
            main.mark_dirty();
        }
        destroy_mirror_markdown_surfaces(&mut self.plugin_manager, removed_markdown);

        let output = MirrorOutbox::new(shared_frame_tx.epoch());
        let disconnected = Arc::new(AtomicBool::new(false));

        spawn_attach_write_thread(
            write_half,
            frame_rx,
            disconnected.clone(),
            attach_wake(&proxy),
            "(재연결)",
        );

        spawn_attach_reader_thread(
            conn,
            output.clone(),
            disconnected.clone(),
            attach_wake(&proxy),
            local_workspace,
            "(재연결)",
            mirror_event_from_control,
        );

        let raw_frame_tx =
            shared_frame_tx.clone();
        spawn_attach_heartbeat_thread(raw_frame_tx, disconnected.clone());

        let sess = &mut self.remote.sessions[sess_idx];
        sess.transport.output = output;
        sess.transport.disconnected = disconnected;
        sess.state.client_id = client_id;
        sess.state.bulk_port = port;
        sess.transport.tunnel = tunnel;
        sess.state.op_seq = 0;
        sess.state.pending_op_focus.clear();
        sess.state.agent_requests.clear();
        sess.state.next_delta_focus = None;
        sess.state.last_forwarded_resize.clear();
        // 옛 연결의 목록 요청은 다시 응답하지 않는다. 소비자는 자체 timeout으로 실패 처리한다.
        sess.state.pending_list_dir_consumers.clear();
        sess.state.resync_pending = None;
        sess.state.resync_awaiting_window = false;
        sess.state.phase = SessionState::Connected;
        sess.state.remote_label = format!("127.0.0.1:{port}");
        // 원문은 바로 다시 받지 않는다. 변경 신호를 보내 플러그인이 재요청이나 stale 표시를 선택하게 한다.
        let markdown_locals: Vec<u32> = sess.state.markdown_locals.iter().copied().collect();
        for local in markdown_locals {
            push_markdown_changed(&mut self.plugin_manager, local);
        }
        tracing::info!(
            "gui attach: mirror workspace {local_workspace} 재연결 성공 (remote ws {workspace})"
        );
        Ok(())
    }

    /// 사용자가 연결한 mirror로만 포커스를 옮긴다. IPC·자동 연결은 호출하지 않는다.
    fn focus_mirror_workspace(&mut self, ws_id: u32) {
        for (_, main, engine) in self.engines_mut().window_pairs() {
            if let Some(idx) = engine
                .workspaces()
                .into_iter()
                .position(|ws| ws.id == ws_id)
            {
                main.state.set_active_workspace_index(engine.core, idx);
                main.mark_dirty();
                break;
            }
        }
    }

    /// 창이 있는 engine, parked engine 순서로 적용 대상을 찾은 뒤 출력 버퍼를 비운다.
    /// 대상이 없으면 버퍼를 유지한다. 고아 판정·정리도 같은 engine 범위를 확인해야 한다.
    pub(crate) fn apply_attach_client_output(&mut self) {
        if self.remote.sessions.is_empty() {
            return;
        }
        let mut dead: Vec<usize> = Vec::new();
        let mut reconnecting: Vec<usize> = Vec::new();
        let mut resyncing: Vec<usize> = Vec::new();
        for idx in 0..self.remote.sessions.len() {
            let (local_ws, disconnected, state, anchor_ws_id) = {
                let sess = &self.remote.sessions[idx];
                (
                    sess.state.local_workspace,
                    sess.transport.disconnected.load(Ordering::SeqCst),
                    sess.state.phase,
                    sess.state.anchor_ws_id,
                )
            };

            let host = mirror_output_host(
                self.find_main_with_workspace(local_ws),
                self.engines().parked_with_ids().map(|(id, e)| (id, e.core)),
                local_ws,
            );
            // delta 뒤의 출력도 갱신된 ID 매핑을 써야 하므로 세션을 복제하지 않고 나눠 빌린다.
            match host {
                Some(MirrorOutputHost::Window(wid)) => {
                    let sess = &mut self.remote.sessions[idx];
                    let mut pair = engines_mut!(self).window_pair(wid);
                    let mut mirror_host = pair
                        .as_mut()
                        .map(|(m, engine)| MirrorHost::windowed(&mut m.state, engine));
                    if let Some(host) = mirror_host.as_mut() {
                        resume_resync_in_window(sess, host);
                    }
                    let applied =
                        apply_pending_mirror_output(sess, mirror_host, &mut self.plugin_manager);
                    if applied && let Some((main, _)) = pair {
                        main.mark_dirty_from(crate::view::RepaintSource::AttachMirror);
                    }
                }
                Some(MirrorOutputHost::Parked(id)) => {
                    let sess = &mut self.remote.sessions[idx];
                    let (state, mut engine) = engines_mut!(self)
                        .parked_session(id)
                        .expect("mirror_output_host가 방금 찾은 parked engine");
                    apply_pending_mirror_output(
                        sess,
                        Some(MirrorHost::parked(state, &mut engine)),
                        &mut self.plugin_manager,
                    );
                }
                None => {}
            }
            // 같은 묶음에 손실 통지와 EOF가 올 수 있어 이벤트 적용 뒤 재attach 여부를 확인한다.
            let sess = &self.remote.sessions[idx];
            match disconnect_disposition(
                disconnected,
                state,
                sess.resync_released(),
                anchor_ws_id.is_some(),
            ) {
                DisconnectDisposition::Resync => resyncing.push(idx),
                DisconnectDisposition::Reconnect => reconnecting.push(idx),
                DisconnectDisposition::Cleanup => dead.push(idx),
                DisconnectDisposition::None => {}
            }
        }
        for &idx in &resyncing {
            if !self.resync_session(idx) {
                if self.remote.sessions[idx].state.anchor_ws_id.is_some() {
                    reconnecting.push(idx);
                } else {
                    dead.push(idx);
                }
            }
        }
        for &idx in &reconnecting {
            self.enter_reconnecting(idx);
        }
        dead.sort_unstable();
        for &idx in dead.iter().rev() {
            let sess = self.remote.sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess, true);
        }
    }

    /// 새 attach snapshot으로 손실 뒤 화면을 재동기화한다. 빠진 출력 이력을 복구하는 것은 아니다.
    fn resync_session(&mut self, idx: usize) -> bool {
        let (port, tunnel, frames, local_workspace) = {
            let sess = &mut self.remote.sessions[idx];
            (
                sess.state.bulk_port,
                sess.transport.tunnel.take(),
                sess.state.resync_pending.unwrap_or(0),
                sess.state.local_workspace,
            )
        };
        match self.reconnect_session(idx, port, tunnel) {
            Ok(()) => {
                tracing::info!(
                    "gui attach: mirror workspace {local_workspace} 재동기화 완료 — 프레임 {frames} 장 손실 뒤 재attach"
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    "gui attach: mirror workspace {local_workspace} 재동기화 실패 — 끊김으로 처리한다: {e}"
                );
                self.remote.sessions[idx].state.resync_pending = None;
                false
            }
        }
    }

    /// anchor가 있는 세션은 연결이 끊겨도 mirror를 남겨 자동 재연결을 기다린다.
    fn enter_reconnecting(&mut self, idx: usize) {
        let (anchor, local_workspace, markdown_locals) = {
            let sess = &mut self.remote.sessions[idx];
            sess.state.phase = SessionState::Reconnecting;
            sess.transport.frame_tx.retire();
            (
                sess.state.anchor_ws_id,
                sess.state.local_workspace,
                sess.state.markdown_locals.clone(),
            )
        };
        // 기다리던 원문 요청을 request_id=0으로 취소하고 표시 중인 원문도 끊김 상태로 바꾼다.
        for local in markdown_locals {
            push_markdown_content_result(
                &mut self.plugin_manager,
                &markdown_content_failure(local, 0, "mirror workspace disconnected"),
            );
        }
        if let Some(anchor) = anchor {
            self.remote.active.remove(&anchor);
            self.remote.pending_reactivation.insert(anchor);
        }
        for (_, main, engine) in self.engines_mut().window_pairs() {
            if engine
                .workspaces()
                .into_iter()
                .any(|ws| ws.id == local_workspace)
            {
                main.state.toasts.push(
                    crate::i18n::t("attach.toast.mirror_reconnecting").to_string(),
                    crate::adapters::ui::ToastKind::Warning,
                    crate::adapters::ui::ToastScope::Window,
                );
                main.mark_dirty();
                break;
            }
        }
        tracing::info!(
            "gui attach: mirror workspace {local_workspace} 재연결 대기(Reconnecting) 진입 (anchor {anchor:?})"
        );
    }

    /// mirror 자원을 정리하며 사용자 닫힌 항목 기록에는 넣지 않는다.
    /// from_disconnect일 때만 재활성화 대기 상태와 끊김 안내를 남긴다.
    fn cleanup_mirror_workspace(&mut self, sess: &AttachClientSession, from_disconnect: bool) {
        log_mirror_cleanup(sess, from_disconnect);
        // 창과 parked engine 모두 정리해야 창 복원 때 끊긴 mirror가 되살아나지 않는다.
        // 창을 순회하는 부분의 범위 일치는 자동 검증하지 않는다.
        // window_access::mirror_workspace_engine_alive의 검사 범위 설명을 참고한다.
        let mut removed = false;
        for (_, main, mut engine) in self.engines_mut().window_pairs() {
            if remove_mirror_workspace_from_engine(
                &mut engine,
                &mut main.state,
                sess.state.local_workspace,
                &sess.state.remote_to_local,
            ) {
                // 사용자가 직접 닫은 경우에는 끊김 toast를 표시하지 않는다.
                if from_disconnect {
                    main.state.toasts.push(
                        crate::i18n::t("attach.toast.mirror_disconnected").to_string(),
                        crate::adapters::ui::ToastKind::Warning,
                        crate::adapters::ui::ToastScope::Window,
                    );
                }
                main.mark_dirty();
                removed = true;
                break;
            }
        }
        if !removed {
            // parked 상태에는 표시할 창이 없어 toast를 쌓거나 redraw를 요청하지 않는다.
            remove_mirror_workspace_from_parked(
                self.engines_mut(),
                sess.state.local_workspace,
                &sess.state.remote_to_local,
            );
        }
        // 응답을 받을 수 없어진 git-viewer 요청을 취소한다.
        if from_disconnect {
            self.notify_git_viewer_mirror_lost();
        }
        destroy_mirror_markdown_surfaces(&mut self.plugin_manager, sess.state.markdown_locals.clone());
        // 사용자 닫기도 heartbeat를 멈춰 소켓이 불필요하게 유지되지 않게 한다.
        sess.transport.disconnected.store(true, Ordering::SeqCst);
        let _ = sess.send_frame(StreamTag::Detach, Vec::new()); // 종료 중 writer가 사라졌다면 전송 실패를 무시한다.
        if let Some(anchor) = sess.state.anchor_ws_id {
            self.remote.active.remove(&anchor);
            self.remote.reconnect.remove(&anchor);
            if from_disconnect {
                self.remote.pending_reactivation.insert(anchor);
            } else {
                self.remote.pending_reactivation.remove(&anchor);
            }
        }
    }

    /// mirror workspace가 창과 parked engine 어디에도 없으면 세션도 정리한다.
    /// 창이 없다는 사실만으로 parked 세션을 고아로 판단하지 않는다.
    pub(crate) fn detach_orphaned_mirror_sessions(&mut self) {
        if self.remote.sessions.is_empty() {
            return;
        }
        let orphaned: Vec<usize> = self
            .remote.sessions
            .iter()
            .enumerate()
            .map(|(idx, s)| (idx, s.state.local_workspace))
            .filter(|&(_, ws)| !self.mirror_workspace_engine_alive(ws))
            .map(|(idx, _)| idx)
            .collect();
        for &idx in orphaned.iter().rev() {
            let sess = self.remote.sessions.remove(idx);
            self.cleanup_mirror_workspace(&sess, false);
        }
    }

    /// 로컬 구조 변경 큐를 원격으로 보내며 결과는 회신과 delta로 적용한다.
    pub(crate) fn dispatch_pending_structural_forwards(&mut self) {
        let mut pending: Vec<crate::app::services::PendingStructuralForward> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.append(&mut engine.remote.pending_structural_forward);
        }
        for local_op in pending {
            self.forward_one_structural_op(local_op);
        }
    }

    /// 사용자 요청의 포커스 의도도 op_id별로 기록한다. 닫기 후보는 원격 ID로 변환한다.
    fn forward_one_structural_op(&mut self, pending: crate::app::services::PendingStructuralForward) {
        let crate::app::services::PendingStructuralForward {
            op: local_op,
            user_triggered,
            close_focus_candidates,
            ..
        } = &pending;
        let local_anchor = local_op.anchor_surface_id();
        let Some((sess, remote_anchor)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_anchor,
            "structural",
        ) else {
            return;
        };
        let Some(wire) = remote_structural_op(local_op, remote_anchor, &sess.state.remote_to_local)
        else {
            return;
        };
        let op_id = sess.state.op_seq;
        sess.state.op_seq += 1;

        if *user_triggered
            && let Some(intent) =
                pending_op_focus_for(local_op, close_focus_candidates, &sess.state.remote_to_local)
        {
            sess.state.pending_op_focus.insert(op_id, intent);
        }
        sess.state.agent_requests.note_structural_from(&pending, op_id);

        let payload = structural_op_payload(op_id, wire, *user_triggered);
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("structural forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
        }
    }

    /// resize 요청만 전송한다. 로컬 mirror grid는 서버의 Resize 회신으로 갱신한다.
    pub(crate) fn dispatch_pending_resize_forwards(&mut self) {
        let mut pending: Vec<(u32, usize, usize)> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            for (sid, (cols, rows)) in engine.remote.pending_resize_forward.drain() {
                pending.push((sid, cols, rows));
            }
        }
        for (local_sid, cols, rows) in pending {
            self.forward_one_resize(local_sid, cols, rows);
        }
    }

    fn forward_one_resize(&mut self, local_sid: u32, cols: usize, rows: usize) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_sid,
            "resize",
        ) else {
            return;
        };
        if sess.state.last_forwarded_resize.get(&remote_sid) == Some(&(cols, rows)) {
            return;
        }
        let payload = serde_json::to_vec(&StreamControl::ClientResize {
            surface_id: remote_sid,
            cols,
            rows,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("resize forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
            return;
        }
        sess.state.last_forwarded_resize.insert(remote_sid, (cols, rows));
    }

    /// 목록 요청을 원격으로 보낸다. 세션이 없으면 폐기하며 소비자는 자체 timeout으로 실패 처리한다.
    pub(crate) fn dispatch_pending_list_dir_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingListDirForward> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.append(&mut engine.remote.pending_list_dir_forward);
        }
        for req in pending {
            if let Err(e) =
                self.send_list_dir_request(req.local_ws_id, req.request_id, &req.dir, req.consumer)
            {
                tracing::warn!(
                    "list_dir_request send 실패 (mirror ws {}, request {}): {e}",
                    req.local_ws_id,
                    req.request_id
                );
            }
        }
    }

    /// 원격 git 요청을 보낼 수 없으면 플러그인에 즉시 실패 결과를 전달한다.
    pub(crate) fn dispatch_pending_git_query_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingGitQueryForward> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.append(&mut engine.remote.pending_git_query_forward);
        }
        for req in pending {
            let send_result = self.send_git_query_request(
                req.local_surface_id,
                req.request_id,
                req.kind,
                req.worktree_path.as_deref(),
                req.diff_path.as_deref(),
            );
            if let Err(e) = send_result {
                tracing::warn!(
                    "git_query_request send 실패 (local surface {}, request {}): {e}",
                    req.local_surface_id,
                    req.request_id
                );
                self.fail_pending_git_query(req.request_id, req.kind, &e.to_string());
            }
        }
    }

    /// 원격 원문 요청을 보낼 수 없으면 플러그인에 즉시 실패 결과를 전달한다.
    pub(crate) fn dispatch_pending_markdown_content_forwards(&mut self) {
        let mut pending: Vec<crate::core::PendingMarkdownContentForward> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.append(&mut engine.remote.pending_markdown_content_forward);
        }
        for req in pending {
            if let Err(e) = self.send_markdown_content_request(&req) {
                tracing::warn!(
                    "markdown_content_request send 실패 (local surface {}, request {}): {e}",
                    req.local_surface_id,
                    req.request_id
                );
                push_markdown_content_result(
                    &mut self.plugin_manager,
                    &markdown_content_failure(req.local_surface_id, req.request_id, &e.to_string()),
                );
            }
        }
    }

    fn fail_pending_git_query(
        &mut self,
        request_id: u64,
        kind: tasty_ipc::stream_hub::GitQueryKind,
        reason: &str,
    ) {
        self.broadcast_git_query_reply(serde_json::json!({
            "request_id": request_id,
            "ok": false,
            "kind": kind.as_wire_str(),
            "data": serde_json::Value::Null,
            "truncated": false,
            "reason": reason,
        }));
    }

    /// request_id=0으로 플러그인이 기다리는 조회를 취소한다.
    /// 호스트는 플러그인의 대기 ID를 몰라 다른 살아 있는 workspace의 조회도 취소될 수 있다.
    fn notify_git_viewer_mirror_lost(&mut self) {
        self.broadcast_git_query_reply(serde_json::json!({
            "request_id": 0,
            "ok": false,
            "kind": "",
            "data": serde_json::Value::Null,
            "truncated": false,
            "reason": "mirror workspace disconnected",
        }));
    }

    /// git-viewer에 결과를 전달하고 열린 인스턴스의 repaint를 예약한다.
    fn broadcast_git_query_reply(&mut self, payload: serde_json::Value) {
        let git_viewer_instances: Vec<u64> = match self.plugin_manager.as_mut() {
            Some(mgr) => {
                mgr.emit_host_event_to_plugin(
                    GIT_VIEWER_PLUGIN_ID,
                    GIT_VIEWER_QUERY_RESULT_EVENT,
                    &payload,
                    tasty_plugin_protocol::EventScope::System,
                );
                mgr.popup_instances()
                    .filter(|(_, inst)| inst.plugin_id == GIT_VIEWER_PLUGIN_ID)
                    .map(|(iid, _)| iid)
                    .collect()
            }
            None => Vec::new(),
        };
        if git_viewer_instances.is_empty() {
            return;
        }
        for main in self.main_windows_iter_mut() {
            for iid in &git_viewer_instances {
                main.state.plugin_mesh_popup_pending_repaint.insert(*iid);
            }
        }
    }

    pub(crate) fn dispatch_pending_mesh_context_forwards(&mut self) {
        let mut pending: Vec<(u32, crate::core::AttachMeshContextForward)> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.extend(engine.remote.pending_mesh_context_forward.drain());
        }
        for (local_sid, ctx) in pending {
            self.forward_one_mesh_context(local_sid, ctx);
        }
    }

    fn forward_one_mesh_context(
        &mut self,
        local_sid: u32,
        ctx: crate::core::AttachMeshContextForward,
    ) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_sid,
            "mesh context",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshContext {
            surface_id: remote_sid,
            width_px: ctx.width_px,
            height_px: ctx.height_px,
            pixels_per_point: ctx.pixels_per_point,
            theme: ctx.theme,
            focused: ctx.focused,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("mesh context forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
        }
    }

    pub(crate) fn dispatch_pending_mesh_input_forwards(&mut self) {
        let mut pending: Vec<(u32, tasty_plugin_protocol::protocol::RawInputWire)> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.extend(engine.remote.pending_mesh_input_forward.drain());
        }
        for (local_sid, input) in pending {
            self.forward_one_mesh_input(local_sid, input);
        }
    }

    fn forward_one_mesh_input(
        &mut self,
        local_sid: u32,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_sid,
            "mesh input",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshInput {
            surface_id: remote_sid,
            input,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("mesh input forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
        }
    }

    /// texture delta 연결이 끊겨 요청한 full frame 재전송을 원격에 전달한다.
    pub(crate) fn dispatch_pending_mesh_full_resend_forwards(&mut self) {
        let mut pending: Vec<u32> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.extend(engine.remote.pending_mesh_full_resend_forward.drain());
        }
        for local_sid in pending {
            self.forward_one_mesh_full_resend_request(local_sid);
        }
    }

    /// 실제 attention 해제 때만 기록된 큐를 전달한다. 포커스를 유지한다고 반복 전송하지 않는다.
    pub(crate) fn dispatch_pending_attention_clear_forwards(&mut self) {
        let mut pending: Vec<u32> = Vec::new();
        for mut engine in self.engines_mut().windows_and_pending() {
            pending.extend(engine.remote.pending_attention_clear_forward.drain());
        }
        for local_sid in pending {
            self.forward_one_attention_clear(local_sid);
        }
    }

    /// 해제 전송 실패는 로그를 남기고 폐기한다. 별도 재시도 큐는 없다.
    fn forward_one_attention_clear(&mut self, local_sid: u32) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_sid,
            "attention clear",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::ClientAttentionClear {
            surface_id: remote_sid,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("attention clear forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
        }
    }

    fn forward_one_mesh_full_resend_request(&mut self, local_sid: u32) {
        let Some((sess, remote_sid)) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            local_sid,
            "mesh full-resend",
        ) else {
            return;
        };
        let payload = serde_json::to_vec(&StreamControl::MeshFullResendRequest {
            surface_id: remote_sid,
        })
        .unwrap_or_default();
        if let Err(e) = sess.send_frame(StreamTag::Control, payload) {
            tracing::warn!("mesh full-resend forward: 전송 큐가 닫혀 요청을 보내지 못했다: {e}");
        }
    }
}

/// 에이전트의 닫기 요청을 원격 사용자 복원 스택에 넣지 않도록 origin을 전달한다.
fn forward_origin_of(user_triggered: bool) -> tasty_ipc::stream::ForwardOrigin {
    if user_triggered {
        tasty_ipc::stream::ForwardOrigin::User
    } else {
        tasty_ipc::stream::ForwardOrigin::Agent
    }
}

/// origin을 항상 포함한 구조 변경 요청 payload.
fn structural_op_payload(
    op_id: u64,
    op: tasty_ipc::stream::StructuralOp,
    user_triggered: bool,
) -> Vec<u8> {
    serde_json::to_vec(&StreamControl::StructuralOp {
        op_id,
        op,
        origin: Some(forward_origin_of(user_triggered)),
    })
    .unwrap_or_default()
}

/// 로컬 mirror ID로 만든 구조 변경을 원격 ID로 바꾼다. anchor는 호출자가 이미 찾았다.
/// MoveSurface의 target도 로컬 ID이므로 같은 세션의 매핑으로 바꾸고, 없으면 보내지 않는다.
/// 로컬 ID를 그대로 보내면 서버의 무관한 surface를 가리킬 수 있다.
fn remote_structural_op(
    local_op: &StructuralOp,
    remote_anchor: u32,
    remote_to_local: &HashMap<u32, u32>,
) -> Option<StructuralOp> {
    let wire = local_op.with_anchor_surface_id(remote_anchor);
    let Some(local_target) = local_op.move_target_surface_id() else {
        return Some(wire);
    };
    let Some(remote_target) = remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_target)
        .map(|(&r, _)| r)
    else {
        tracing::warn!(
            "structural forward: 이동 대상 로컬 surface {local_target} 가 같은 mirror 세션에 없어 요청을 버린다"
        );
        return None;
    };
    Some(wire.with_move_target_surface_id(remote_target))
}

fn find_mirror_session_and_remote_id<'a>(
    sessions: &'a mut [AttachClientSession],
    local_sid: u32,
    label: &str,
) -> Option<(&'a mut AttachClientSession, u32)> {
    let Some(sess) = sessions
        .iter_mut()
        .find(|s| s.state.remote_to_local.values().any(|&l| l == local_sid))
    else {
        tracing::warn!(
            "{label} forward: mirror 세션이 로컬 surface {local_sid} 를 갖지 않아 요청을 버린다"
        );
        return None;
    };
    let Some(remote_sid) = sess
        .remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
    else {
        tracing::warn!("{label} forward: 로컬 surface {local_sid} 의 원격 ID가 없어 요청을 버린다");
        return None;
    };
    Some((sess, remote_sid))
}



/// read/write timeout을 설정한다. 설정 실패는 경고만 남기며 연결은 계속한다.
/// writer 사본도 같은 소켓의 timeout 설정을 사용한다.




/// 이 engine에 해당 workspace가 있으면 mirror 자원을 함께 정리하고 활성 인덱스를 보정한다.
fn remove_mirror_workspace_from_engine(
    engine: &mut EngineMut<'_>,
    state: &mut crate::state::MainViewState,
    local_workspace: u32,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let Some(pos) = engine
        .workspaces()
        .into_iter()
        .position(|ws| ws.id == local_workspace)
    else {
        return false;
    };
    for &local in remote_to_local.values() {
        engine.runtime.surfaces.remove(&local);
        engine.runtime.terminals.remove(local);
        engine.forget_mirror_surface_busy(local);
        engine.forget_mirror_surface_attention(local);
        engine.forget_mirror_surface_cwd(local);
        engine.remote.attach_mesh_frames.remove(local);
        // 로컬 닫기 정리와 같이 soft 점유 등 이 surface의 점유 기록을 지운다.
        engine.forget_closed_surface(local);
    }
    engine.remove_workspace_at(pos);
    state.reconcile_presentation(engine);
    // mirror만 남았다면 원격 끊김 때문에 사용자 창을 닫는 대신 기본 workspace를 만든다.
    state.recreate_workspace_if_empty(engine, "mirror workspace cleanup");
    true
}

/// 출력 적용·고아 판정과 같은 parked 순회를 사용한다. 첫 항목만 확인해서는 안 된다.
fn remove_mirror_workspace_from_parked(
    mut engines: EngineScanMut<'_>,
    local_workspace: u32,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let parked = engines
        .reborrow()
        .parked_sessions_with_ids()
        .map(|(id, _, e)| (id, &*e.core));
    let Some(id) = find_parked_with_workspace(parked, local_workspace) else {
        return false;
    };
    let Some((state, mut engine)) = engines.parked_session(id) else {
        return false;
    };
    remove_mirror_workspace_from_engine(&mut engine, state, local_workspace, remote_to_local)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirrorOutputHost {
    Window(winit::window::WindowId),
    Parked(EngineId),
}

/// 창이 있는 engine을 우선하고 없으면 parked engine에서 찾는다. None이면 버퍼를 비우지 않는다.
fn mirror_output_host<'a>(
    windowed: Option<winit::window::WindowId>,
    parked: impl IntoIterator<Item = (EngineId, &'a crate::core::CoreState)>,
    local_workspace: u32,
) -> Option<MirrorOutputHost> {
    windowed.map(MirrorOutputHost::Window).or_else(|| {
        find_parked_with_workspace(parked, local_workspace).map(MirrorOutputHost::Parked)
    })
}

/// 출력 적용·정리·고아 판정이 공유하는 parked engine 검색. 여러 항목 모두 확인한다.
pub(super) fn find_parked_with_workspace<'a>(
    parked: impl IntoIterator<Item = (EngineId, &'a crate::core::CoreState)>,
    local_workspace: u32,
) -> Option<EngineId> {
    parked
        .into_iter()
        .find(|(_, engine)| engine.has_workspace(local_workspace))
        .map(|(id, _)| id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisconnectDisposition {
    None,
    Resync,
    Reconnect,
    Cleanup,
}

/// 연결별 종료 신호에는 한 번만 반응한다. 손실로 Detach를 보낸 세션은 anchor 없이도 재attach한다.
/// parked에서 Detach를 미룬 동안 별도로 끊기면 일반 끊김으로 처리한다.
fn disconnect_disposition(
    disconnected: bool,
    state: SessionState,
    resync_released: bool,
    has_anchor: bool,
) -> DisconnectDisposition {
    if !disconnected || state != SessionState::Connected {
        DisconnectDisposition::None
    } else if resync_released {
        DisconnectDisposition::Resync
    } else if has_anchor {
        DisconnectDisposition::Reconnect
    } else {
        DisconnectDisposition::Cleanup
    }
}

/// 창과 parked 상태의 공통 적용 대상. 창이 있어야 의미 있는 toast만 구별한다.
struct MirrorHost<'a, 'engine> {
    state: &'a mut crate::state::MainViewState,
    engine: &'a mut EngineMut<'engine>,
    windowed: bool,
}

impl<'a, 'engine> MirrorHost<'a, 'engine> {
    fn windowed(
        state: &'a mut crate::state::MainViewState,
        engine: &'a mut EngineMut<'engine>,
    ) -> Self {
        Self {
            state,
            engine,
            windowed: true,
        }
    }

    fn parked(
        state: &'a mut crate::state::MainViewState,
        engine: &'a mut EngineMut<'engine>,
    ) -> Self {
        Self {
            state,
            engine,
            windowed: false,
        }
    }

    /// 적용 대상이 준비된 뒤 버퍼를 비우고 순서대로 적용한다.
    fn drain_and_apply(
        &mut self,
        sess: &mut AttachClientSession,
        plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ) -> bool {
        let drained = sess.transport.output.drain();
        if drained.is_empty() {
            return false;
        }
        apply_mirror_events(sess, self, plugin_manager, drained);
        true
    }

    fn toast(&mut self, message: String, kind: crate::adapters::ui::ToastKind) {
        if self.windowed {
            self.state
                .toasts
                .push(message, kind, crate::adapters::ui::ToastScope::Window);
        } else {
            tracing::info!("attach mirror: parked engine 이라 toast 생략 — {message}");
        }
    }
}

/// 적용 대상이 없으면 버퍼를 유지해 다음 수신 이벤트나 주기 확인에서 다시 처리한다.
fn apply_pending_mirror_output(
    sess: &mut AttachClientSession,
    host: Option<MirrorHost<'_, '_>>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
) -> bool {
    let Some(mut host) = host else {
        return false;
    };
    host.drain_and_apply(sess, plugin_manager)
}

fn apply_mirror_events(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_, '_>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    events: Vec<MirrorEvent>,
) {
    let epoch=sess.transport.frame_tx.epoch();
    for ev in events {
        if !epoch.is_active() || !epoch.same(&sess.transport.frame_tx.epoch()) {break;}
        apply_one_mirror_event(sess, host, plugin_manager, ev);
    }
}

fn log_mirror_cleanup(sess: &AttachClientSession, from_disconnect: bool) {
    if from_disconnect {
        tracing::warn!(
            "attach mirror cleanup: local ws {} (remote ws {}, anchor {:?}) — 원격발 disconnect 로 정리",
            sess.state.local_workspace,
            sess.state.remote_workspace,
            sess.state.anchor_ws_id
        );
    } else {
        tracing::info!(
            "attach mirror cleanup: local ws {} (remote ws {}, anchor {:?}) — 사용자 close 로 정리",
            sess.state.local_workspace,
            sess.state.remote_workspace,
            sess.state.anchor_ws_id
        );
    }
}

/// 프레임을 순서대로 쓰며 write 실패는 연결 종료로 처리한다.
/// 일부만 쓴 프레임을 같은 소켓에서 재시도하면 수신 경계가 어긋날 수 있다.


/// 제어 프레임을 mirror 이벤트로 변환한다. 어느 파서에서도 인식하지 못하면 무시한다.
fn mirror_event_from_control(payload: &[u8]) -> Option<MirrorEvent> {
    match serde_json::from_slice::<StreamControl>(payload) {
        Ok(StreamControl::Resize {
            surface_id,
            cols,
            rows,
        }) => Some(MirrorEvent::Resize(surface_id, cols, rows)),
        Ok(StreamControl::Activity { surface_id, busy }) => {
            Some(MirrorEvent::Activity(surface_id, busy))
        }
        Ok(StreamControl::Attention { surface_id, kind }) => {
            Some(MirrorEvent::Attention(surface_id, kind))
        }
        Ok(StreamControl::Cwd { surface_id, cwd }) => Some(MirrorEvent::Cwd(surface_id, cwd)),
        // 무엇을 잃었는지 알 수 없는 연결 단위 통지라 세션 전체를 재동기화한다.
        Ok(StreamControl::Loss { frames }) => Some(MirrorEvent::Desynced { frames }),
        Ok(StreamControl::StructuralResult {
            ok: false,
            op_id,
            reason,
        }) => Some(MirrorEvent::StructuralFailed(op_id, reason)),
        Ok(StreamControl::StructuralResult {
            ok: true, op_id, ..
        }) => Some(MirrorEvent::StructuralSucceeded(op_id)),
        Ok(StreamControl::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        }) => Some(MirrorEvent::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        }),
        Ok(_) | Err(_) => parse_capture_result(payload)
            .or_else(|| parse_list_dir_result(payload))
            .or_else(|| parse_git_query_result(payload))
            .or_else(|| parse_markdown_content_result(payload))
            .or_else(|| parse_markdown_changed(payload)),
    }
}

/// 수신 이벤트를 버퍼에 쌓고 메인 루프를 깨운다. 수신 실패는 연결 종료로 전달한다.


/// 연결마다 heartbeat를 보낸다. 종료 신호를 확인하거나 큐 전송에 실패하면 끝난다.


/// 로컬 PTY 없이 mirror 터미널을 만든다. 입력은 별도 스레드로 원격에 전달한다.
fn make_mirror_surface(
    remote_id: u32,
    local_id: u32,
    cols: usize,
    rows: usize,
    frame_tx: &SharedFrameSender,
    engine: &mut EngineMut<'_>,
) {
    let mut mirror = Terminal::new_detached(cols, rows);
    bind_mirror_input(&mut mirror,remote_id,frame_tx,false);
    // mirror의 feed_bytes는 process의 lazy 동기화를 거치지 않아 옵저버 게이트를 여기서 초기화한다.
    mirror.set_output_events_enabled(engine.observer_router.wants(local_id));
    engine.runtime.terminals.insert(local_id, mirror, None);
}


fn pending_op_focus_for(
    op: &StructuralOp,
    close_focus_candidates: &[u32],
    remote_to_local: &HashMap<u32, u32>,
) -> Option<PendingOpFocus> {
    match op {
        StructuralOp::NewTab { .. }
        | StructuralOp::SplitSurface { .. }
        | StructuralOp::SplitPane { .. }
        | StructuralOp::RestoreClosedItem { .. } => Some(PendingOpFocus::NewResource),
        StructuralOp::CloseSurface { .. }
        | StructuralOp::CloseTab { .. }
        | StructuralOp::ClosePane { .. } => {
            let candidates: Vec<u32> = close_focus_candidates
                .iter()
                .filter_map(|local_sid| {
                    remote_to_local
                        .iter()
                        .find(|&(_, l)| l == local_sid)
                        .map(|(&r, _)| r)
                })
                .collect();
            if candidates.is_empty() {
                None
            } else {
                Some(PendingOpFocus::Close { candidates })
            }
        }
        _ => None,
    }
}

/// 기존 원격 surface의 로컬 ID·자원을 재사용하고 추가·삭제·kind 변경을 반영한다.
/// markdown은 기존 핸들을 공유해 구조 변경 때마다 문서를 다시 만들지 않는다.
fn merge_survivor_mapping(
    old_map: &HashMap<u32, u32>,
    surfaces: &[Value],
    ids: &crate::core::state::IdGenerator,
    frame_tx: &SharedFrameSender,
    engine: &mut EngineMut<'_>,
) -> SurvivorMapping {
    let mut new_map: HashMap<u32, u32> = HashMap::new();
    let mut terminal_locals: HashSet<u32> = HashSet::new();
    let mut mesh_locals: HashMap<u32, MirrorMeshInfo> = HashMap::new();
    let mut explorer_locals: HashMap<u32, std::path::PathBuf> = HashMap::new();
    let mut markdown_locals: MirrorMarkdownLeaves = HashMap::new();
    let mut newly_created_remote_ids: Vec<u32> = Vec::new();
    let markdown_available = markdown_mirror_available(engine);
    for s in surfaces {
        let remote_id = s.get("remote_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let role = s.get("role").and_then(|v| v.as_str());
        let is_terminal = role == Some("terminal");
        // 재구성할 실제 kind로 비교한다. 미등록 markdown이나 지원하지 않는 role은 empty다.
        let new_kind: &str = if is_terminal {
            "terminal"
        } else if role == Some("mesh") {
            s.get("kind").and_then(|v| v.as_str()).unwrap_or("mesh")
        } else if role == Some("explorer") {
            "explorer"
        } else if role == Some("markdown") && markdown_available {
            MARKDOWN_MIRROR_KIND
        } else {
            "empty"
        };
        let survivor_local = old_map.get(&remote_id).copied();
        let old_kind: Option<&'static str> =
            survivor_local.and_then(|l| engine.find_surface_by_id(l).map(|s| s.kind()));
        let local_id = match survivor_local {
            Some(l) => {
                // ID가 같아도 kind가 바뀌면 옛 자원은 정리한다. markdown destroy는 호출자가 맡는다.
                if old_kind != Some(new_kind) {
                    engine.runtime.surfaces.remove(&l);
                    if old_kind == Some("terminal") {
                        engine.runtime.terminals.remove(l);
                        engine.forget_mirror_surface_busy(l);
                    }
                    // cwd는 terminal뿐 아니라 explorer·markdown에도 있어 이전 kind와 함께 지운다.
                    engine.forget_mirror_surface_cwd(l);
                    // 새 frame이 올 때까지 이전 kind의 화면을 그리지 않게 한다.
                    engine.remote.attach_mesh_frames.remove(l);
                    if is_terminal {
                        let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                        let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                        make_mirror_surface(remote_id, l, cols, rows, frame_tx, engine);
                    }
                }
                l
            }
            None => {
                let l = ids.next_surface();
                if is_terminal {
                    let cols = s.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as usize;
                    let rows = s.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as usize;
                    make_mirror_surface(remote_id, l, cols, rows, frame_tx, engine);
                }
                newly_created_remote_ids.push(remote_id);
                l
            }
        };
        if is_terminal {
            terminal_locals.insert(local_id);
        } else if role == Some("mesh") {
            let kind = s
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("mesh")
                .to_string();
            let plugin_id = s
                .get("plugin_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let display_name = s
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind.clone());
            mesh_locals.insert(
                local_id,
                MirrorMeshInfo {
                    display_name,
                    kind,
                    plugin_id,
                },
            );
        } else if role == Some("explorer") {
            let root = s
                .get("root")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            explorer_locals.insert(local_id, root);
        } else if new_kind == MARKDOWN_MIRROR_KIND {
            if old_kind == Some(MARKDOWN_MIRROR_KIND) {
                markdown_locals.insert(local_id, MARKDOWN_MIRROR_KIND.to_string());
            } else if let Some(surface) = create_mirror_markdown_surface(s, local_id, engine.core) {
                markdown_locals.insert(local_id, surface.kind().to_string());
                engine.runtime.surfaces.insert(local_id, surface);
            }
        } else if role == Some("markdown")
            && !engine.runtime.surface_registry.contains(MARKDOWN_MIRROR_KIND)
        {
            // kind 등록을 기다렸다가 표시 시 실제화한다. 다른 플러그인이 같은 이름을
            // 이미 등록했으면 허용된 소유자가 아니므로 placeholder로 기다리지 않는다.
            let surface=deferred_mirror_markdown_surface(s,local_id);
            markdown_locals.insert(local_id,surface.kind().to_string());
            engine.runtime.surfaces.insert(local_id,surface);
        }
        // The descriptor tree never owns or clones a kind instance. Keep survivors in
        // the existing runtime collection, replacing only instances changed by the server.
        if is_terminal {
            engine.runtime.surfaces.entry(local_id).or_insert_with(||Box::new(TerminalSurface{id:local_id}));
        } else if let Some(info)=mesh_locals.get(&local_id) {
            engine.runtime.surfaces.insert(local_id,Box::new(crate::model::AttachMeshSurface::new(local_id,&info.kind,info.plugin_id.clone(),info.display_name.clone())));
        } else if let Some(root)=explorer_locals.get(&local_id) {
            engine.runtime.surfaces.insert(local_id,Box::new(ExplorerPanel::new(local_id,root.clone())));
        } else if !markdown_locals.contains_key(&local_id) {
            engine.runtime.surfaces.insert(local_id,Box::new(EmptySurface::new(local_id)));
        }
        new_map.insert(remote_id, local_id);
    }

    for (&remote_id, &local_id) in old_map.iter() {
        if !new_map.contains_key(&remote_id) {
            engine.runtime.surfaces.remove(&local_id);
            engine.runtime.terminals.remove(local_id);
            engine.forget_mirror_surface_busy(local_id);
            engine.forget_mirror_surface_attention(local_id);
            engine.forget_mirror_surface_cwd(local_id);
            engine.remote.attach_mesh_frames.remove(local_id);
            // 원격이 닫은 surface도 로컬 닫기처럼 soft 점유 등 점유 기록을 남기지 않는다.
            engine.forget_closed_surface(local_id);
        }
    }

    SurvivorMapping {
        remote_to_local: new_map,
        terminals: terminal_locals,
        mesh: mesh_locals,
        explorer: explorer_locals,
        markdown: markdown_locals,
        newly_created_remote_ids,
    }
}

fn install_mirror_fallbacks(workspace:&Workspace, engine:&mut EngineMut<'_>) {
    for id in workspace.all_surface_ids() {
        engine.runtime.surfaces.entry(id).or_insert_with(||Box::new(EmptySurface::new(id)));
    }
}

type MirrorMarkdownLeaves = HashMap<u32, String>;

struct SurvivorMapping {
    remote_to_local: HashMap<u32, u32>,
    terminals: HashSet<u32>,
    mesh: HashMap<u32, MirrorMeshInfo>,
    explorer: HashMap<u32, std::path::PathBuf>,
    markdown: MirrorMarkdownLeaves,
    /// 새로 매핑한 원격 surface는 사용자 new-tab/split의 포커스 후보가 된다.
    newly_created_remote_ids: Vec<u32>,
}

impl SurvivorMapping {
    fn markdown_ids(&self) -> HashSet<u32> {
        self.markdown.keys().copied().collect()
    }
}

/// kind가 허용된 markdown 플러그인에 등록됐는지 확인한다.
fn markdown_mirror_available(engine: &crate::core::CoreState) -> bool {
    engine.runtime.surface_registry
        .get_live(MARKDOWN_MIRROR_KIND)
        .is_some_and(|def| {
            matches!(
                &def.source,
                crate::runtime::surface_registry::KindSource::Plugin(p) if p == MARKDOWN_PLUGIN_ID
            )
        })
}

/// 원격 경로는 remote.file로 전달한다. file을 쓰면 플러그인이 로컬 경로로 읽는다.
fn create_mirror_markdown_surface(
    descriptor: &Value,
    local_id: u32,
    engine: &crate::core::CoreState,
) -> Option<Box<dyn Surface>> {
    let params = mirror_markdown_params(descriptor);
    match engine.create_surface_via_registry(MARKDOWN_MIRROR_KIND, local_id, None, &params) {
        Ok(surface) => Some(surface),
        Err(e) => {
            tracing::warn!(
                "attach mirror: markdown surface {local_id} 생성 실패 — 빈 surface: {e}"
            );
            None
        }
    }
}

/// 생성과 deferred 복원이 같은 remote params를 사용한다.
fn mirror_markdown_params(descriptor: &Value) -> Value {
    let file = descriptor
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let display_name = descriptor
        .get("display_name")
        .and_then(|v| v.as_str())
        .unwrap_or(MARKDOWN_MIRROR_KIND);
    serde_json::json!({
        "display_name": display_name,
        "remote": { "file": file },
    })
}

fn deferred_mirror_markdown_surface(descriptor: &Value, local_id: u32) -> Box<dyn Surface> {
    Box::new(EmptySurface::new_deferred_plugin(
        local_id,
        DeferredPlugin {
            kind: MARKDOWN_MIRROR_KIND.to_string(),
            snapshot: mirror_markdown_params(descriptor),
        },
    ))
}

/// mirror 삭제는 일반 lifecycle 큐를 거치지 않아 사라진 문서를 플러그인에 직접 알린다.
fn destroy_mirror_markdown_surfaces(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ids: impl IntoIterator<Item = u32>,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    for id in ids {
        mgr.destroy_remote_surface(id, Some(MARKDOWN_MIRROR_KIND));
    }
}

/// 이 이벤트의 surface ID는 로컬 ID다.
fn push_markdown_changed(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    local_surface_id: u32,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    mgr.emit_host_event_to_plugin(
        MARKDOWN_PLUGIN_ID,
        MARKDOWN_MIRROR_CHANGED_EVENT,
        &serde_json::json!({ "surface_id": local_surface_id }),
        tasty_plugin_protocol::EventScope::System,
    );
}

/// payload.surface_id는 플러그인이 아는 로컬 ID여야 한다.
fn push_markdown_content_result(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    payload: &Value,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    mgr.emit_host_event_to_plugin(
        MARKDOWN_PLUGIN_ID,
        MARKDOWN_MIRROR_CONTENT_RESULT_EVENT,
        payload,
        tasty_plugin_protocol::EventScope::System,
    );
}

/// 요청을 보낼 수 없거나 연결이 끊기면 실패를 합성한다. request_id=0은 대기 요청 취소다.
fn markdown_content_failure(local_surface_id: u32, request_id: u64, reason: &str) -> Value {
    serde_json::json!({
        "surface_id": local_surface_id,
        "request_id": request_id,
        "ok": false,
        "file": Value::Null,
        "source": Value::Null,
        "truncated": false,
        "reason": reason,
    })
}

/// 손실 통지마다 스트림 표지를 바꾼다. 이미 재attach 대기 중이면 손실 수를 누적한다.
/// parked 상태에서는 창을 다시 찾을 때까지 Detach를 미룬다.
fn begin_resync(sess: &mut AttachClientSession, host: &mut MirrorHost<'_, '_>, frames: u64) {
    for &local in sess.state.remote_to_local.values() {
        if let Some(t) = host.engine.runtime.terminals.get_mut(local) {
            t.renew_output_stream();
        }
    }
    if let Some(total) = sess.state.resync_pending.as_mut() {
        *total += frames;
        return;
    }
    sess.state.resync_pending = Some(frames);
    if !host.windowed {
        sess.state.resync_awaiting_window = true;
        tracing::warn!(
            "attach mirror: mirror workspace {} — 서버가 이 연결의 프레임 {frames} 장을 버렸다. 창이 없어(parked) 재attach 를 창이 돌아올 때까지 미룬다",
            sess.state.local_workspace
        );
        return;
    }
    tracing::warn!(
        "attach mirror: mirror workspace {} — 서버가 이 연결의 프레임 {frames} 장을 버렸다. 옛 연결을 놓고 재attach 한다",
        sess.state.local_workspace
    );
    release_for_resync(sess, host);
}

fn resume_resync_in_window(sess: &mut AttachClientSession, host: &mut MirrorHost<'_, '_>) {
    if !sess.state.resync_awaiting_window || !host.windowed {
        return;
    }
    sess.state.resync_awaiting_window = false;
    tracing::info!(
        "attach mirror: mirror workspace {} — 창이 돌아왔다. 미뤄 둔 재attach 를 시작한다(손실 {} 장)",
        sess.state.local_workspace,
        sess.state.resync_pending.unwrap_or(0)
    );
    release_for_resync(sess, host);
}

/// Detach 뒤 EOF를 확인하면 재attach한다. 큐 전송 실패도 끊김 처리를 기다린다.
fn release_for_resync(sess: &mut AttachClientSession, host: &mut MirrorHost<'_, '_>) {
    if let Err(e) = sess.send_frame(StreamTag::Detach, Vec::new()) {
        tracing::warn!("attach mirror: 재동기화용 Detach 를 큐에 못 넣었다 — 끊김을 기다린다: {e}");
    }
    host.toast(
        crate::i18n::t("attach.toast.mirror_desynced").to_string(),
        crate::adapters::ui::ToastKind::Warning,
    );
}

fn apply_one_mirror_event(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_, '_>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    ev: MirrorEvent,
) {
    match ev {
        MirrorEvent::Desynced { frames } => begin_resync(sess, host, frames),
        MirrorEvent::Data(remote_id, bytes) => {
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id)
                && let Some(t) = host.engine.runtime.terminals.get_mut(local)
            {
                t.feed_bytes(&bytes);
            }
        }
        MirrorEvent::Resize(remote_id, cols, rows) => {
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id)
                && let Some(t) = host.engine.runtime.terminals.get_mut(local)
            {
                t.resize(cols, rows);
            }
        }
        MirrorEvent::Activity(remote_id, busy) => {
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id) {
                host.engine.set_mirror_surface_busy(local, busy);
            }
        }
        MirrorEvent::Cwd(remote_id, cwd) => {
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id) {
                host.engine.set_mirror_surface_cwd(local, cwd);
            }
        }
        MirrorEvent::Attention(remote_id, kind) => {
            // 서버 상태를 반영할 때 로컬 해제 요청을 다시 forward하지 않는다.
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id) {
                host.engine.set_mirror_surface_attention(
                    local,
                    kind.map(crate::core::AttentionKind::from_wire),
                );
            }
        }
        MirrorEvent::StructuralFailed(op_id, reason) => {
            agent_origin::apply_structural_failed(sess, host, op_id, reason)
        }
        MirrorEvent::StructuralSucceeded(op_id) => {
            // 성공한 요청의 포커스 의도는 다음 delta가 한 번 소비한다.
            sess.state.agent_requests.forget_structural(op_id);
            if let Some(intent) = sess.state.pending_op_focus.remove(&op_id) {
                sess.state.next_delta_focus = Some(intent);
            }
        }
        MirrorEvent::StructuralDelta {
            workspace_id,
            tree,
            surfaces,
        } => {
            let pending_focus = sess.state.next_delta_focus.take();
            let removed_markdown = apply_mirror_structural_delta(
                &mut host.state.navigation,
                sess,
                host.engine,
                workspace_id,
                &tree,
                &surfaces,
                pending_focus,
            );
            host.state.reconcile_presentation(host.engine);
            destroy_mirror_markdown_surfaces(plugin_manager, removed_markdown);
        }
        MirrorEvent::CaptureResult { ok, path, reason } => {
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
            host.toast(msg, kind);
        }
        MirrorEvent::ListDirResult {
            request_id,
            ok,
            dir,
            entries,
            truncated,
            reason,
        } => {
            apply_list_dir_result_event(
                sess, host, request_id, ok, dir, entries, truncated, reason,
            );
        }
        MirrorEvent::GitQueryResult {
            request_id,
            ok,
            kind,
            data,
            truncated,
            reason,
        } => {
            apply_git_query_result_event(
                plugin_manager,
                host.state,
                request_id,
                ok,
                kind,
                data,
                truncated,
                reason,
            );
        }
        MirrorEvent::MarkdownContentResult {
            request_id,
            surface_id,
            ok,
            file,
            source,
            truncated,
            reason,
        } => {
            apply_markdown_content_result_event(
                sess,
                host,
                plugin_manager,
                request_id,
                surface_id,
                ok,
                file,
                source,
                truncated,
                reason,
            );
        }
        MirrorEvent::MarkdownChanged { surface_id } => {
            // 이 세션이 mirror하지 않는 문서의 신호는 무시한다.
            if let Some(local) = markdown_mirror_local(sess, surface_id) {
                push_markdown_changed(plugin_manager, local);
            }
        }
        MirrorEvent::Mesh(remote_id, generation, frame_seq, full, bytes) => {
            if let Some(&local) = sess.state.remote_to_local.get(&remote_id) {
                host.engine
                    .remote.attach_mesh_frames
                    .update(local, bytes, generation, frame_seq, full);
            }
        }
    }
}

/// 요청 때 기록한 소비자로 목록을 전달한다. 요청 기록이 없으면 오래된 회신으로 보고 무시한다.
fn apply_list_dir_result_event(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_, '_>,
    request_id: u64,
    ok: bool,
    dir: Option<String>,
    entries: Option<Vec<crate::core::fs_list::DirEntryInfo>>,
    truncated: bool,
    reason: Option<String>,
) {
    let consumer = sess
        .pending_list_dir_consumers
        .remove(&request_id)
        .flatten();
    if let Some(surface_id) = consumer {
        let result = if ok {
            Ok(entries.unwrap_or_default())
        } else {
            Err(reason.unwrap_or_default())
        };
        if let Some(panel) = host
            .engine
            .find_surface_by_id(surface_id)
            .and_then(|s| s.as_any().downcast_ref::<crate::model::ExplorerPanel>())
        {
            let is_err = result.is_err();
            host.state
                .explorer_views
                .apply_remote_list_dir_result(surface_id, request_id, panel, result);
            if !is_err && truncated {
                host.toast(
                    crate::i18n::t("explorer.state.remote_listing_truncated").to_string(),
                    crate::adapters::ui::ToastKind::Warning,
                );
            }
        }
        return;
    }
    // 아직 열린 picker가 같은 요청을 기다릴 때만 반영한다.
    let Some(picker) = host.state.dialogs.file_picker.as_mut() else {
        return;
    };
    let is_pending = matches!(
        &picker.load,
        crate::state::FpLoadState::Loading { request_id: rid, .. } if *rid == request_id
    );
    if !is_pending {
        return;
    }
    if ok {
        let es = entries.unwrap_or_default();
        if let Some(d) = dir {
            picker.current_dir = d;
        }
        picker.load = if es.is_empty() {
            crate::state::FpLoadState::Empty
        } else {
            crate::state::FpLoadState::Loaded
        };
        picker.entries = es;
        if picker.remote_host.is_none() {
            picker.remote_host = Some(sess.state.remote_label.clone());
        }
        if truncated {
            host.toast(
                crate::i18n::t("filepicker.remote_listing_truncated").to_string(),
                crate::adapters::ui::ToastKind::Warning,
            );
        }
    } else {
        let reason_str = reason.unwrap_or_default();
        picker.load = if reason_str == "permission denied" {
            crate::state::FpLoadState::ErrorPerm(reason_str)
        } else {
            crate::state::FpLoadState::ErrorConn(reason_str)
        };
    }
}

fn apply_git_query_result_event(
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    state: &mut crate::state::MainViewState,
    request_id: u64,
    ok: bool,
    kind: String,
    data: Option<Value>,
    truncated: bool,
    reason: Option<String>,
) {
    let Some(mgr) = plugin_manager.as_mut() else {
        return;
    };
    let payload = serde_json::json!({
        "request_id": request_id,
        "ok": ok,
        "kind": kind,
        "data": data,
        "truncated": truncated,
        "reason": reason,
    });
    mgr.emit_host_event_to_plugin(
        GIT_VIEWER_PLUGIN_ID,
        GIT_VIEWER_QUERY_RESULT_EVENT,
        &payload,
        tasty_plugin_protocol::EventScope::System,
    );
    // 결과 이벤트만으로는 렌더 입력이 바뀌지 않아 플러그인 repaint도 요청한다.
    for (iid, inst) in mgr.popup_instances() {
        if inst.plugin_id == GIT_VIEWER_PLUGIN_ID {
            state.plugin_mesh_popup_pending_repaint.insert(iid);
        }
    }
}

fn markdown_mirror_local(sess: &AttachClientSession, remote_surface_id: u32) -> Option<u32> {
    let &local = sess.state.remote_to_local.get(&remote_surface_id)?;
    sess.state.markdown_locals.contains(&local).then_some(local)
}

/// 원격 ID를 로컬 markdown ID로 바꿔 회신한다. leaf가 사라졌거나 kind가 바뀌었으면 무시한다.
#[allow(clippy::too_many_arguments)] // reason: wire 회신 필드를 풀어 받는다(GitQueryResult 적용과 같은 형태)
fn apply_markdown_content_result_event(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_, '_>,
    plugin_manager: &mut Option<crate::plugin::PluginManager>,
    request_id: u64,
    remote_surface_id: u32,
    ok: bool,
    file: Option<String>,
    source: Option<String>,
    truncated: bool,
    reason: Option<String>,
) {
    let agent_origin = sess.state.agent_requests.take_markdown(request_id);
    let Some(local) = markdown_mirror_local(sess, remote_surface_id) else {
        return;
    };
    let payload = serde_json::json!({
        "surface_id": local,
        "request_id": request_id,
        "ok": ok,
        "file": file,
        "source": source,
        "truncated": truncated,
        "reason": reason,
    });
    push_markdown_content_result(plugin_manager, &payload);
    if ok && truncated {
        agent_origin::notify_markdown_truncated(host, local, agent_origin);
    }
}

/// 새 구조를 반영하되 살아남은 surface의 로컬 ID·자원과 사용자의 포커스를 보존한다.
/// 순수 로컬 포커스 이동은 서버에 전달하지 않으므로 원격 포커스를 그대로 덮어쓰지 않는다.
/// 사라진 로컬 markdown ID를 반환해 호출자가 플러그인에 destroy를 보낼 수 있게 한다.
fn apply_mirror_structural_delta(
    navigation: &mut crate::state::navigation::NavigationState,
    sess: &mut AttachClientSession,
    engine: &mut EngineMut<'_>,
    workspace_id: u32,
    tree: &Value,
    surfaces: &[Value],
    pending_focus: Option<PendingOpFocus>,
) -> Vec<u32> {
    let ids = engine.next_ids.clone();

    let old_focused_remote: Option<u32> = engine
        .workspaces()
        .into_iter()
        .find(|w| w.id == sess.state.local_workspace)
        .and_then(|ws| capture_focused_remote(navigation, ws, &sess.state.remote_to_local));

    let mut mapping = merge_survivor_mapping(
        &sess.state.remote_to_local,
        surfaces,
        &ids,
        &sess.transport.frame_tx,
        engine,
    );
    let newly_created_remote_ids = std::mem::take(&mut mapping.newly_created_remote_ids);

    sess.state.remote_to_local = std::mem::take(&mut mapping.remote_to_local);
    let new_markdown = mapping.markdown_ids();
    let removed_markdown: Vec<u32> = sess
        .markdown_locals
        .difference(&new_markdown)
        .copied()
        .collect();
    sess.state.markdown_locals = new_markdown;

    if let Some(pos) = engine
        .workspaces()
        .into_iter()
        .position(|w| w.id == sess.state.local_workspace)
    {
        let name = engine
            .workspace_at(pos)
            .expect("workspace index is valid")
            .name
            .clone();
        let mut ws = build_mirror_workspace(
            &mut sess.state.structure_ids,
            navigation,
            sess.state.local_workspace,
            &name,
            tree,
            &ids,
            &sess.state.remote_to_local,
            &mapping.terminals,
            &mapping.mesh,
            &mapping.explorer,
            &mut mapping.markdown,
        );
        install_mirror_fallbacks(&ws,engine);
        ws.mirror=true;

        // 사용자가 새 surface를 만들었다면 옛 포커스 복원보다 새 surface 선택을 우선한다.
        let mut focus_handled = false;
        if matches!(pending_focus, Some(PendingOpFocus::NewResource))
            && let Some(&new_local) = newly_created_remote_ids
                .first()
                .and_then(|rid| sess.state.remote_to_local.get(rid))
        {
            focus_handled = set_focus_to_surface(navigation, &ws, new_local);
        }
        if !focus_handled {
            let restored = restore_focus_after_delta(
                navigation,
                &mut ws,
                old_focused_remote,
                &sess.state.remote_to_local,
            );
            // 옛 포커스가 사라졌으면 닫기 전에 구한 인접 후보를 시도한다. 후보도 없으면 원격 값을 유지한다.
            if !restored && let Some(PendingOpFocus::Close { candidates }) = &pending_focus {
                for &remote_cand in candidates {
                    if let Some(&local_cand) = sess.state.remote_to_local.get(&remote_cand)
                        && set_focus_to_surface(navigation, &ws, local_cand)
                    {
                        break;
                    }
                }
            }
        }

        engine
            .replace_mirror_workspace(ws)
            .unwrap_or_else(|_| panic!("mirror workspace disappeared"));
    } else {
        tracing::warn!(
            "structural delta: mirror workspace {} (remote {workspace_id}) 를 찾지 못해 갱신을 건너뛴다",
            sess.state.local_workspace
        );
    }
    removed_markdown
}

fn restore_focus_after_delta(
    navigation: &mut crate::state::navigation::NavigationState,
    ws: &mut Workspace,
    old_focused_remote: Option<u32>,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let Some(remote_sid) = old_focused_remote else {
        return false;
    };
    let Some(&new_local_sid) = remote_to_local.get(&remote_sid) else {
        return false;
    };
    set_focus_to_surface(navigation, ws, new_local_sid)
}

fn set_focus_to_surface(
    navigation: &mut crate::state::navigation::NavigationState,
    ws: &Workspace,
    local_sid: u32,
) -> bool {
    let Some((pane_id, tab_id)) = find_pane_and_tab_for_surface(ws, local_sid) else {
        return false;
    };
    navigation.select_pane(ws, pane_id);
    if let Some(pane) = ws.pane_layout().find_pane(pane_id)
        && let Some(tab_index) = pane.tabs.iter().position(|t| t.id == tab_id)
    {
        navigation.select_tab(pane, tab_id);
        navigation.select_surface(&pane.tabs[tab_index], local_sid);
        true
    } else {
        false
    }
}

/// Surviving remote surface IDs restore the active branch after a subtree moves.
fn capture_focused_remote(
    navigation: &crate::state::navigation::NavigationState,
    ws: &Workspace,
    remote_to_local: &HashMap<u32, u32>,
) -> Option<u32> {
    let pane = ws.pane_layout().find_pane(navigation.pane_id(ws)?)?;
    let tab = pane.tabs.get(navigation.tab_index(pane))?;
    let local_sid = navigation.surface_id(tab)?;
    remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
}

/// 아직 engine에 넣지 않은 새 workspace에서도 찾을 수 있도록 범위를 workspace 하나로 제한한다.
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

#[allow(clippy::too_many_arguments)] // reason: mirror pane 파서 컨텍스트 전체
fn build_pane_from_json(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    p: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
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
        let layout = build_layout(
            navigation,
            &layout_json,
            &MirrorLayoutSources {
                ids,
                map,
                term,
                mesh,
                explorer,
            },
            markdown,
        )
        .unwrap_or_else(|| SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(ids.next_surface(),"empty")));
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
            .unwrap_or(crate::i18n::t("attach.tab_title_fallback"))
            .to_string();
        if t.get("active").and_then(|v| v.as_bool()).unwrap_or(false) {
            active_tab = i;
        }
        let remote_tab = t.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let tab_id = if remote_tab == 0 {
            ids.next_tab()
        } else {
            *structure_ids
                .remote_tabs
                .entry(remote_tab)
                .or_insert_with(|| ids.next_tab())
        };
        let tab = Tab {
            id: tab_id,
            name: tab_name,
            explicit_name: None,
            layout_opt: Some(layout),
            surface_titles: Default::default(),
        };
        navigation.initialize_surface(&tab, focused_surface);
        tabs.push(tab);
    }
    if tabs.is_empty() {
        let sid = ids.next_surface();
        tabs.push(Tab {
            id: ids.next_tab(),
            name: crate::i18n::t("attach.tab_title_fallback").to_string(),
            explicit_name: None,
            layout_opt: Some(SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(sid,"empty"))),
            surface_titles: Default::default(),
        });
    }
    let remote_pane = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let pane_id = if remote_pane == 0 {
        ids.next_pane()
    } else {
        *structure_ids
            .panes
            .entry(remote_pane)
            .or_insert_with(|| ids.next_pane())
    };
    let pane = Pane { id: pane_id, tabs };
    navigation.initialize_tab(&pane, active_tab);
    pane
}

/// pane 트리를 읽으며 원격→로컬 pane ID를 기록해 focused_pane을 변환한다.
#[allow(clippy::too_many_arguments)] // reason: mirror 트리 재귀 파서 컨텍스트 전체
fn build_pane_node(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    node: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
    pane_id_map: &mut HashMap<u32, u32>,
) -> Option<PaneNode> {
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote_pane = node.get("id").and_then(|v| v.as_u64())? as u32;
            let pane = build_pane_from_json(
                structure_ids,
                navigation,
                node,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
            );
            pane_id_map.insert(remote_pane, pane.id);
            Some(PaneNode::Leaf(pane))
        }
        "Split" => {
            let direction = match node.get("direction").and_then(|v| v.as_str()) {
                Some("vertical") => SplitDirection::Vertical,
                _ => SplitDirection::Horizontal,
            };
            let ratio = node.get("ratio").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let first = build_pane_node(
                structure_ids,
                navigation,
                node.get("first")?,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
                pane_id_map,
            )?;
            let second = build_pane_node(
                structure_ids,
                navigation,
                node.get("second")?,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
                pane_id_map,
            )?;
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

/// pane_layout이 있으면 방향·비율을 복원한다. 없거나 파싱에 실패하면 평면 panes를
/// 가로 분할로 연결한다. 각 tab의 surface layout은 두 경로에서 같은 파서를 사용한다.
#[allow(clippy::too_many_arguments)] // reason: mirror workspace 재구성 컨텍스트 전체
fn build_mirror_workspace(
    structure_ids: &mut MirrorStructureIds,
    navigation: &mut crate::state::navigation::NavigationState,
    ws_id: u32,
    name: &str,
    tree: &Value,
    ids: &crate::core::state::IdGenerator,
    map: &HashMap<u32, u32>,
    term: &HashSet<u32>,
    mesh: &HashMap<u32, MirrorMeshInfo>,
    explorer: &HashMap<u32, std::path::PathBuf>,
    markdown: &mut MirrorMarkdownLeaves,
) -> Workspace {
    let remote_focused_pane = tree
        .get("focused_pane")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    if let Some(layout_json) = tree.get("pane_layout").filter(|v| !v.is_null()) {
        let mut pane_id_map = HashMap::new();
        if let Some(node) = build_pane_node(
            structure_ids,
            navigation,
            layout_json,
            ids,
            map,
            term,
            mesh,
            explorer,
            markdown,
            &mut pane_id_map,
        ) {
            let focused_local_pane = pane_id_map
                .get(&remote_focused_pane)
                .copied()
                .unwrap_or_else(|| node.first_pane().map(|p| p.id).unwrap_or(0));
            let mut ws = Workspace::from_restored(ws_id, name.to_string(), String::new(), node);
            ws.mirror = true;
            navigation.initialize_pane(&ws, focused_local_pane);
            structure_ids.retain_workspace(&ws);
            return ws;
        }
    }

    let panes_json = tree
        .get("panes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut local_panes: Vec<(u32, Pane)> = Vec::new();
    for p in &panes_json {
        let remote_pane = p.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        local_panes.push((
            remote_pane,
            build_pane_from_json(
                structure_ids,
                navigation,
                p,
                ids,
                map,
                term,
                mesh,
                explorer,
                markdown,
            ),
        ));
    }

    if local_panes.is_empty() {
        let sid = ids.next_surface();
        let pane = Pane::new_with_surface(
            ids.next_pane(),
            ids.next_tab(),
            crate::i18n::t("attach.tab_title_fallback").to_string(),
            Box::new(EmptySurface::new(sid)),
        );
        let mut ws =
            Workspace::from_restored(ws_id, name.to_string(), String::new(), PaneNode::Leaf(pane));
        ws.mirror = true;
        structure_ids.retain_workspace(&ws);
        return ws;
    }

    let focused_local_pane = local_panes
        .iter()
        .find(|(rp, _)| *rp == remote_focused_pane)
        .map(|(_, p)| p.id)
        .unwrap_or(local_panes[0].1.id);

    // 평면 목록만 보낸 서버에서는 원래 pane 배치를 알 수 없어 가로로 연결한다.
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

    let mut ws = Workspace::from_restored(ws_id, name.to_string(), String::new(), node);
    ws.mirror = true;
    navigation.initialize_pane(&ws, focused_local_pane);
    structure_ids.retain_workspace(&ws);
    ws
}

struct MirrorLayoutSources<'a> {
    ids: &'a crate::core::state::IdGenerator,
    map: &'a HashMap<u32, u32>,
    term: &'a HashSet<u32>,
    mesh: &'a HashMap<u32, MirrorMeshInfo>,
    explorer: &'a HashMap<u32, std::path::PathBuf>,
}

fn build_layout(
    navigation: &mut crate::state::navigation::NavigationState,
    node: &Value,
    sources: &MirrorLayoutSources<'_>,
    markdown: &mut MirrorMarkdownLeaves,
) -> Option<SurfaceLayout> {
    let MirrorLayoutSources {
        ids,
        map,
        term,
        mesh,
        explorer,
    } = *sources;
    match node.get("type").and_then(|v| v.as_str())? {
        "Leaf" => {
            let remote = node.get("id").and_then(|v| v.as_u64())? as u32;
            let local = map
                .get(&remote)
                .copied()
                .unwrap_or_else(|| ids.next_surface());
            let kind = if term.contains(&local) {"terminal"}
                else if let Some(info)=mesh.get(&local) {info.kind.as_str()}
                else if explorer.contains_key(&local) {"explorer"}
                else if let Some(kind)=markdown.get(&local) {kind.as_str()}
                else {"empty"};
            Some(SurfaceLayout::Leaf(crate::model::SurfaceDescriptor::new(local,kind)))
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
            let first = build_layout(navigation, node.get("first")?, sources, markdown)?;
            let second = build_layout(navigation, node.get("second")?, sources, markdown)?;
            let node_id = crate::model::SplitNodeId::allocate();
            navigation.split_hints.insert(node_id, focus_second);
            Some(SurfaceLayout::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
                node_id,
            })
        }
        _ => None,
    }
}

/// 원격은 client_id로도 구별하므로 업로드 ID는 프로세스 안에서 구별되면 된다.
static NEXT_CAPTURE_UPLOAD_ID: AtomicU64 = AtomicU64::new(1);

fn next_capture_upload_id() -> u64 {
    NEXT_CAPTURE_UPLOAD_ID.fetch_add(1, Ordering::Relaxed)
}

/// bulk ID도 원격 client_id와 함께 사용한다.
static NEXT_BULK_TRANSFER_ID: AtomicU64 = AtomicU64::new(1);

fn next_bulk_transfer_id() -> u64 {
    NEXT_BULK_TRANSFER_ID.fetch_add(1, Ordering::Relaxed)
}

/// binary sub-header를 더해도 프레임 상한을 넘지 않는 payload 크기.
const BULK_CHUNK_RAW_LEN: usize = stream::MAX_FRAME_LEN as usize - stream::BULK_CHUNK_HEADER_LEN;

/// 이미지 업로드 결과 처리가 원격 정책 거절과 전송 오류를 구별하는 접두사다.
/// 거절이면 Dismiss만, 나머지 오류면 Retry를 제공하므로 문구를 임의로 바꾸지 않는다.
pub(crate) const BULK_REJECT_PREFIX: &str = "remote rejected bulk upload: ";

/// base64 팽창과 JSON 오버헤드를 포함해 Control 프레임 상한 안에 들도록 여유를 둔다.
const CAPTURE_CHUNK_RAW_LEN: usize = 700 * 1024;

#[derive(serde::Deserialize)]
struct CaptureResultWire {
    ok: bool,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

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

/// modified_unix는 epoch 초이며 DirEntryInfo의 SystemTime으로 변환한다.
#[derive(serde::Deserialize)]
struct ListDirEntryWire {
    name: String,
    is_dir: bool,
    size: u64,
    #[serde(default)]
    modified_unix: Option<u64>,
    #[serde(default)]
    ext: String,
}

#[derive(serde::Deserialize)]
struct ListDirResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    dir: Option<String>,
    #[serde(default)]
    entries: Option<Vec<ListDirEntryWire>>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

fn parse_list_dir_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("list_dir_result") {
        return None;
    }
    let wire: ListDirResultWire = serde_json::from_value(value).ok()?;
    let entries = wire.entries.map(|es| {
        es.into_iter()
            .map(|e| crate::core::fs_list::DirEntryInfo {
                path: wire
                    .dir
                    .as_deref()
                    .map(|d| std::path::Path::new(d).join(&e.name))
                    .unwrap_or_else(|| std::path::PathBuf::from(&e.name)),
                name: e.name,
                is_dir: e.is_dir,
                size: e.size,
                modified: e
                    .modified_unix
                    .map(|secs| std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs)),
                ext: e.ext,
            })
            .collect()
    });
    Some(MirrorEvent::ListDirResult {
        request_id: wire.request_id,
        ok: wire.ok,
        dir: wire.dir,
        entries,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

/// kind별 데이터는 flatten으로 받고 호스트가 해석하지 않은 채 git-viewer로 전달한다.
#[derive(serde::Deserialize)]
struct GitQueryResultWire {
    request_id: u64,
    ok: bool,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    truncated_status: bool,
    #[serde(default)]
    truncated_log: bool,
    #[serde(default)]
    truncated_diff: bool,
    #[serde(default)]
    reason: Option<String>,
    #[serde(flatten)]
    rest: serde_json::Map<String, serde_json::Value>,
}

fn parse_git_query_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("git_query_result") {
        return None;
    }
    let wire: GitQueryResultWire = serde_json::from_value(value).ok()?;
    let truncated = wire.truncated_status || wire.truncated_log || wire.truncated_diff;
    let data = wire.ok.then(|| Value::Object(wire.rest));
    Some(MirrorEvent::GitQueryResult {
        request_id: wire.request_id,
        ok: wire.ok,
        kind: wire.kind,
        data,
        truncated,
        reason: wire.reason,
    })
}

#[derive(serde::Deserialize)]
struct MarkdownContentResultWire {
    request_id: u64,
    surface_id: u32,
    ok: bool,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    truncated: bool,
    #[serde(default)]
    reason: Option<String>,
}

fn parse_markdown_content_result(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("markdown_content_result") {
        return None;
    }
    let wire: MarkdownContentResultWire = serde_json::from_value(value).ok()?;
    Some(MirrorEvent::MarkdownContentResult {
        request_id: wire.request_id,
        surface_id: wire.surface_id,
        ok: wire.ok,
        file: wire.file,
        source: wire.source,
        truncated: wire.truncated,
        reason: wire.reason,
    })
}

fn parse_markdown_changed(payload: &[u8]) -> Option<MirrorEvent> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    if value.get("event").and_then(|v| v.as_str()) != Some("markdown_changed") {
        return None;
    }
    let surface_id = u32::try_from(value.get("surface_id")?.as_u64()?).ok()?;
    Some(MirrorEvent::MarkdownChanged { surface_id })
}

impl App {
    /// 캡처를 원격에 올리고 원격 클립보드에 경로를 넣도록 요청한다.
    /// StreamControl enum 밖의 capture_chunk/capture_commit 이벤트를 사용한다.
    pub(crate) fn forward_capture_to_remote_clipboard(
        &mut self,
        local_ws_id: u32,
        file_name: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .remote.sessions
            .iter()
            .find(|s| s.state.local_workspace == local_ws_id)
        else {
            anyhow::bail!("no attach session for mirror workspace {local_ws_id}");
        };
        let frame_tx = sess.transport.frame_tx.clone();
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
            send_capture_control_frame(&frame_tx, &msg)?;
        }
        let commit = serde_json::json!({
            "event": "capture_commit",
            "upload_id": upload_id,
            "file_name": file_name,
        });
        send_capture_control_frame(&frame_tx, &commit)
    }

    /// 목록 소비자는 wire에 없으므로 요청 ID에 기록한다. None은 picker, Some은 explorer다.
    pub(crate) fn send_list_dir_request(
        &mut self,
        local_ws_id: u32,
        request_id: u64,
        dir: &str,
        consumer: Option<u32>,
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .remote.sessions
            .iter_mut()
            .find(|s| s.state.local_workspace == local_ws_id)
        else {
            anyhow::bail!("no attach session for mirror workspace {local_ws_id}");
        };
        let msg = serde_json::json!({
            "event": "list_dir_request",
            "request_id": request_id,
            "dir": dir,
        });
        let result = send_capture_control_frame(&sess.transport.frame_tx, &msg);
        if result.is_ok() {
            sess.state.pending_list_dir_consumers.insert(request_id, consumer);
        }
        result
    }

    /// 원격 surface ID를 보내 서버가 실제 cwd에서 Git 정보를 찾게 한다.
    pub(crate) fn send_git_query_request(
        &mut self,
        local_surface_id: u32,
        request_id: u64,
        kind: tasty_ipc::stream_hub::GitQueryKind,
        worktree_path: Option<&str>,
        diff_path: Option<&str>,
    ) -> anyhow::Result<()> {
        let Some(sess) = self
            .remote.sessions
            .iter()
            .find(|s| s.state.remote_to_local.values().any(|&l| l == local_surface_id))
        else {
            anyhow::bail!("no attach session for mirror surface {local_surface_id}");
        };
        let Some(remote_sid) = sess
            .remote_to_local
            .iter()
            .find(|&(_, &l)| l == local_surface_id)
            .map(|(&r, _)| r)
        else {
            anyhow::bail!("no remote surface id for mirror surface {local_surface_id}");
        };
        let msg = serde_json::json!({
            "event": "git_query_request",
            "request_id": request_id,
            "surface_id": remote_sid,
            "kind": kind.as_wire_str(),
            "worktree_path": worktree_path,
            "diff_path": diff_path,
        });
        send_capture_control_frame(&sess.transport.frame_tx, &msg)
    }

    pub(crate) fn send_markdown_content_request(
        &mut self,
        req: &crate::core::PendingMarkdownContentForward,
    ) -> anyhow::Result<()> {
        let (local_surface_id, request_id) = (req.local_surface_id, req.request_id);
        let Some(sess) = self
            .remote.sessions
            .iter_mut()
            .find(|s| s.state.markdown_locals.contains(&local_surface_id))
        else {
            anyhow::bail!("no attach session holds mirror markdown surface {local_surface_id}");
        };
        let Some(remote_sid) = sess
            .remote_to_local
            .iter()
            .find(|&(_, &l)| l == local_surface_id)
            .map(|(&r, _)| r)
        else {
            anyhow::bail!("no remote surface id for mirror surface {local_surface_id}");
        };
        if sess.state.phase != SessionState::Connected {
            anyhow::bail!("mirror session is reconnecting");
        }
        let msg = serde_json::json!({
            "event": "markdown_content_request",
            "request_id": request_id,
            "surface_id": remote_sid,
        });
        send_capture_control_frame(&sess.transport.frame_tx, &msg)?;
        sess.state.agent_requests.note_markdown_from(req, request_id);
        Ok(())
    }
}

fn send_capture_control_frame(
    frame_tx: &SharedFrameSender,
    msg: &serde_json::Value,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(msg)?;
    frame_tx.send(OutFrame {
            tag: StreamTag::Control,
            payload,
        })
        .map_err(|_| anyhow::anyhow!("attach write queue closed (write thread gone)"))?;
    Ok(())
}

impl App {
    /// 워커가 세션 전체를 빌리지 않도록 접속 포트와 원격 workspace ID만 꺼낸다.
    pub(crate) fn bulk_target_for(&self, local_ws_id: u32) -> Option<(u16, u32)> {
        self.remote.sessions
            .iter()
            .find(|s| s.state.local_workspace == local_ws_id)
            .map(|s| (s.state.bulk_port, s.state.remote_workspace))
    }
}

/// 파일을 transfer_id·seq 헤더가 있는 청크로 나눈다. 빈 입력은 청크 없이 begin·commit만 보낸다.
fn bulk_chunk_frames(transfer_id: u64, bytes: &[u8]) -> Vec<Vec<u8>> {
    bytes
        .chunks(BULK_CHUNK_RAW_LEN)
        .enumerate()
        .map(|(seq, part)| stream::encode_bulk_chunk(transfer_id, seq as u32, part))
        .collect()
}

/// 기존 workspace 점유에 연결된 bulk 채널을 연다. timeout 설정 실패는 경고만 남긴다.
fn open_bulk_connection(port: u16, remote_ws: u32) -> anyhow::Result<StreamConnection> {
    let sock = TcpStream::connect(("127.0.0.1", port))?;
    if let Err(e) = sock.set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("bulk upload: failed to set read timeout: {e}");
    }
    if let Err(e) = sock.set_write_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("bulk upload: failed to set write timeout: {e}");
    }
    let (mut conn, _client_id) = StreamConnection::open_bulk(sock, STREAM_PROTO, remote_ws)?;
    declare_bulk_loss_notify(&mut conn);
    Ok(conn)
}

/// 결과가 유실되면 기다리기만 하지 않도록 손실 통지를 요청한다. 구 서버는 무시할 수 있다.
fn declare_bulk_loss_notify(conn: &mut StreamConnection) {
    match serde_json::to_vec(&StreamControl::ClientLossNotify {}) {
        Ok(declare) => {
            if let Err(e) = conn.send(StreamTag::Control, &declare) {
                tracing::warn!("bulk upload: loss-notify declaration was not sent: {e}");
            }
        }
        Err(e) => tracing::warn!("bulk upload: loss-notify declaration did not serialize: {e}"),
    }
}

/// begin·청크·commit을 순서대로 보내고 시작 및 각 청크 뒤에 누적 전송 바이트를 알린다.
fn send_bulk_payload(
    conn: &mut StreamConnection,
    transfer_id: u64,
    file_name: &str,
    bytes: &[u8],
    on_progress: impl Fn(u64, u64),
) -> anyhow::Result<()> {
    let begin = StreamControl::BulkBegin {
        transfer_id,
        filename: file_name.to_string(),
        total_size: bytes.len() as u64,
    };
    conn.send(StreamTag::Control, &serde_json::to_vec(&begin)?)?;

    let total = bytes.len() as u64;
    on_progress(0, total);
    let mut sent: u64 = 0;
    for framed in bulk_chunk_frames(transfer_id, bytes) {
        let part_len = (framed.len() - stream::BULK_CHUNK_HEADER_LEN) as u64;
        conn.send(StreamTag::Data, &framed)?;
        sent += part_len;
        on_progress(sent, total);
    }

    let commit = StreamControl::BulkCommit { transfer_id };
    conn.send(StreamTag::Control, &serde_json::to_vec(&commit)?)?;
    Ok(())
}

/// 해당 transfer_id의 결과를 기다린다. Ping이나 다른 응답은 무시하므로 전체 대기 기한은 없다.
/// 소켓 timeout이 설정됐다면 개별 읽기에 적용된다.
fn await_bulk_result(conn: &mut StreamConnection, transfer_id: u64) -> anyhow::Result<String> {
    loop {
        let frame = conn.recv()?;
        match frame.tag {
            StreamTag::Control => {
                match serde_json::from_slice::<StreamControl>(&frame.payload) {
                    Ok(StreamControl::BulkResult {
                        transfer_id: tid,
                        ok,
                        path,
                        reason,
                    }) if tid == transfer_id => {
                        if let Err(e) = conn.detach() {
                            tracing::debug!("bulk upload: detach after result failed: {e}");
                        }
                        return if ok {
                            path.ok_or_else(|| {
                                anyhow::anyhow!("bulk result ok but carried no path")
                            })
                        } else {
                            Err(anyhow::anyhow!(
                                "{BULK_REJECT_PREFIX}{}",
                                reason.unwrap_or_else(|| "unknown".to_string())
                            ))
                        };
                    }
                    // 서버가 저장했는지 알 수 없어 자동 재시도하지 않는다.
                    // 정책 거절 접두사는 붙이지 않아 사용자가 재시도를 선택할 수 있게 한다.
                    Ok(StreamControl::Loss { frames }) => {
                        if let Err(e) = conn.detach() {
                            tracing::debug!("bulk upload: detach after a loss notice failed: {e}");
                        }
                        anyhow::bail!(
                            "bulk upload aborted: the remote dropped {frames} frame(s) of this transfer's result channel, so whether the file was saved is unknown"
                        );
                    }
                    _ => {}
                }
            }
            StreamTag::Detach => {
                anyhow::bail!("remote detached before delivering bulk result");
            }
            _ => {}
        }
    }
}

/// 원격 업로드 전체를 동기로 수행한다. on_progress는 시작과 각 청크 전송 뒤 호출한다.
pub(crate) fn upload_file_over_bulk(
    port: u16,
    remote_ws: u32,
    file_name: &str,
    bytes: &[u8],
    on_progress: impl Fn(u64, u64),
) -> anyhow::Result<String> {
    let transfer_id = next_bulk_transfer_id();
    let mut conn = open_bulk_connection(port, remote_ws)?;
    send_bulk_payload(&mut conn, transfer_id, file_name, bytes, on_progress)?;
    await_bulk_result(&mut conn, transfer_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::engine_registry::EngineRegistry;
    use crate::core::state::IdGenerator;
    use crate::runtime::engine_session::EngineSession;

    /// 순수 함수 시험용 parked 항목. registry 없이 id·View 복원 자료·engine만 묶는다.
    struct ParkedEngine {
        view_restore: crate::state::MainViewState,
        session: EngineSession,
    }

    impl ParkedEngine {
        fn from_test_state(
            (view_restore, core_state): (crate::state::MainViewState, EngineSession),
        ) -> Self {
            Self {
                view_restore,
                session: core_state,
            }
        }
    }

    #[test]
    fn an_agent_close_is_forwarded_with_the_agent_origin() {
        let queued = |user_triggered| crate::app::services::PendingStructuralForward {
            op: tasty_ipc::stream::StructuralOp::CloseSurface { surface_id: 9 },
            user_triggered,
            close_focus_candidates: Vec::new(),
            silent_failure: false,
        };
        let origin_on_wire = |p: crate::app::services::PendingStructuralForward| {
            let payload = structural_op_payload(3, p.op, p.user_triggered);
            let v: serde_json::Value = serde_json::from_slice(&payload).expect("json");
            assert_eq!(v["event"], "structural_op");
            v["origin"].clone()
        };
        assert_eq!(origin_on_wire(queued(false)), "agent");
        assert_eq!(origin_on_wire(queued(true)), "user");
    }
    use crate::ipc::stream::SplitAxis;

    /// 공용 정리 본문을 검사한다. 두 호출 경로의 연결 여부까지 검증하는 시험은 아니다.
    #[test]
    fn remove_mirror_workspace_clears_terminal_busy_and_mesh() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let ws_id = 9_000u32;
        let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
        let remote_surface = 42u32;

        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            pane_id,
            tab_id,
            local_surface,
        );
        mirror_ws.mirror = true;
        engine.push_mirror_workspace(mirror_ws);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        engine.set_mirror_surface_busy(local_surface, true);
        engine.set_mirror_surface_cwd(local_surface, Some("/srv/remote".to_string()));
        engine
            .remote.attach_mesh_frames
            .update(local_surface, vec![1, 2, 3], 0, 0, true);
        state.set_active_workspace_index(&engine, engine.workspaces().len() - 1);

        let remote_to_local = HashMap::from([(remote_surface, local_surface)]);
        assert!(remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            ws_id,
            &remote_to_local
        ));

        assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
        assert!(
            !engine.runtime.terminals.contains(local_surface),
            "mirror 터미널 제거"
        );
        assert!(
            !engine.is_surface_busy(local_surface),
            "mirror busy 엔트리 제거"
        );
        assert!(
            engine.remote.attach_mesh_frames.get(local_surface).is_none(),
            "mesh 프레임 캐시 제거"
        );
        assert!(
            engine.remote.mirror_surface_cwd.is_empty(),
            "mirror cwd 엔트리 제거"
        );
        assert_eq!(
            state.active_workspace_index(&engine),
            engine.workspaces().len() - 1,
            "제거로 out-of-range 가 된 active_workspace 클램프"
        );
    }

    #[test]
    fn removing_the_only_mirror_workspace_recreates_a_default_workspace() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let ws_id = 9_000u32;
        let local_surface = 9_003u32;
        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            9_001,
            9_002,
            local_surface,
        );
        mirror_ws.mirror = true;
        engine.replace_local_workspaces(Vec::new());
        engine.push_mirror_workspace(mirror_ws);
        state.set_active_workspace_index(&engine, 0);

        assert!(remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            ws_id,
            &HashMap::from([(42u32, local_surface)]),
        ));

        assert_eq!(
            engine.workspaces().len(),
            1,
            "기본 워크스페이스가 다시 생긴다"
        );
        assert!(!engine.has_workspace(ws_id));
        assert_eq!(state.active_workspace_index(&engine), 0);
        assert!(!state.active_workspace(&engine).mirror);
    }

    #[test]
    fn remove_mirror_workspace_leaves_unrelated_engine_untouched() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let local_surface = 9_003u32;
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        let before = engine.workspaces().len();

        let remote_to_local = HashMap::from([(42u32, local_surface)]);
        assert!(!remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            9_000,
            &remote_to_local
        ));
        assert_eq!(engine.workspaces().len(), before);
        assert!(engine.runtime.terminals.contains(local_surface));
    }

    /// mirror가 두 번째 parked engine에 있어야 첫 항목만 검사하는 오류를 잡을 수 있다.
    #[test]
    fn cleanup_scans_all_parked_engines_for_the_mirror_workspace() {
        let ws_id = 9_000u32;
        let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
        let remote_to_local = HashMap::from([(42u32, local_surface)]);

        let mut reg = EngineRegistry::default();
        let ids: Vec<EngineId> = (0..2)
            .map(|_| {
                let (state, engine_session) = crate::state::tests::test_state();
                reg.park_for_test(state, engine_session)
            })
            .collect();
        let untouched_ws_count = reg.get(ids[0]).expect("engine").workspaces().len();

        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            pane_id,
            tab_id,
            local_surface,
        );
        mirror_ws.mirror = true;
        {
            let (state, mut engine) = reg.parked_session_mut(ids[1]).expect("parked 항목");
            engine.push_mirror_workspace(mirror_ws);
            engine
                .runtime
                .terminals
                .insert(local_surface, Terminal::new_detached(80, 24), None);
            engine.set_mirror_surface_busy(local_surface, true);
            engine
                .remote.attach_mesh_frames
                .update(local_surface, vec![1, 2, 3], 0, 0, true);
            state.set_active_workspace_index(engine.core, engine.workspaces().len() - 1);
        }

        assert!(remove_mirror_workspace_from_parked(
            EngineScanMut::from_fields(&mut HashMap::new(), &mut reg),
            ws_id,
            &remote_to_local
        ));

        let (state, engine) = reg.parked_session_mut(ids[1]).expect("parked 항목");
        assert!(!engine.has_workspace(ws_id), "mirror 워크스페이스 행 제거");
        assert!(
            !engine.runtime.terminals.contains(local_surface),
            "mirror 터미널 제거"
        );
        assert!(
            !engine.is_surface_busy(local_surface),
            "mirror busy 엔트리 제거"
        );
        assert!(
            engine.remote.attach_mesh_frames.get(local_surface).is_none(),
            "mesh 프레임 캐시 제거"
        );
        assert_eq!(
            state.active_workspace_index(engine.core),
            engine.workspaces().len() - 1
        );
        assert_eq!(
            reg.get(ids[0]).expect("engine").workspaces().len(),
            untouched_ws_count,
            "무관한 parked engine 은 건드리지 않는다"
        );
    }

    #[test]
    fn cleanup_parked_scan_reports_false_when_absent() {
        let mut reg = EngineRegistry::default();
        for _ in 0..2 {
            let (state, engine_session) = crate::state::tests::test_state();
            reg.park_for_test(state, engine_session);
        }
        let remote_to_local = HashMap::from([(42u32, 9_003u32)]);
        assert!(!remove_mirror_workspace_from_parked(
            EngineScanMut::from_fields(&mut HashMap::new(), &mut reg),
            9_000,
            &remote_to_local
        ));
    }

    #[test]
    fn bulk_chunk_frames_roundtrip_and_reassembly() {
        let transfer_id = 0xABCD_1234_5678_9F01u64;
        // 여러 프레임에 걸쳐 다시 조립되는지 확인할 크기다.
        let total = BULK_CHUNK_RAW_LEN * 2 + 777;
        let data: Vec<u8> = (0..total).map(|i| (i % 251) as u8).collect();

        let frames = bulk_chunk_frames(transfer_id, &data);
        assert_eq!(frames.len(), 3, "2.5 청크 = 3 파트");

        let mut reassembled = Vec::new();
        for (expected_seq, framed) in frames.iter().enumerate() {
            assert!(framed.len() <= stream::MAX_FRAME_LEN as usize);
            let (tid, seq, part) =
                stream::decode_bulk_chunk(framed).expect("valid bulk chunk header");
            assert_eq!(tid, transfer_id);
            assert_eq!(seq as usize, expected_seq);
            reassembled.extend_from_slice(part);
        }
        assert_eq!(reassembled, data, "재조립 바이트가 원본과 동일");
    }

    #[test]
    fn bulk_chunk_frames_empty_is_zero_chunks() {
        assert!(bulk_chunk_frames(1, &[]).is_empty());
    }

    #[test]
    fn bulk_chunk_frames_exact_boundary_is_single_chunk() {
        let data = vec![7u8; BULK_CHUNK_RAW_LEN];
        let frames = bulk_chunk_frames(9, &data);
        assert_eq!(frames.len(), 1);
        let (_, seq, part) = stream::decode_bulk_chunk(&frames[0]).unwrap();
        assert_eq!(seq, 0);
        assert_eq!(part.len(), BULK_CHUNK_RAW_LEN);
    }

    #[test]
    fn build_layout_preserves_split_and_remaps_ids() {
        let mut navigation = crate::state::navigation::NavigationState::default();

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
        let layout = build_layout(
            &mut navigation,
            &node,
            &MirrorLayoutSources {
                ids: &ids,
                map: &map,
                term: &term,
                mesh: &HashMap::new(),
                explorer: &HashMap::new(),
            },
            &mut HashMap::new(),
        )
        .expect("layout");
        match layout {
            SurfaceLayout::Split {
                direction,
                ratio,
                node_id,
                first,
                second,
            } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert!((ratio - 0.3).abs() < 1e-6);
                assert!(navigation.split_hints.get(&node_id).copied().unwrap());
                assert_eq!(first.first_surface_id(), Some(5));
                assert_eq!(second.first_surface_id(), Some(6));
                assert_eq!(first.find_surface(5).unwrap().kind(), "terminal");
                assert_ne!(second.find_surface(6).unwrap().kind(), "terminal");
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn build_layout_constructs_explorer_panel_from_explorer_map() {
        let mut navigation = crate::state::navigation::NavigationState::default();

        let ids = IdGenerator::new();
        let map = HashMap::from([(200u32, 9u32)]);
        let term = HashSet::new();
        let mesh = HashMap::new();
        let explorer = HashMap::from([(9u32, std::path::PathBuf::from("/remote/project"))]);
        let node = serde_json::json!({ "type": "Leaf", "id": 200, "kind": "explorer" });
        let layout = build_layout(
            &mut navigation,
            &node,
            &MirrorLayoutSources {
                ids: &ids,
                map: &map,
                term: &term,
                mesh: &mesh,
                explorer: &explorer,
            },
            &mut HashMap::new(),
        )
        .expect("layout");
        let SurfaceLayout::Leaf(surface) = layout else {
            panic!("expected Leaf");
        };
        assert_eq!(surface.kind(), "explorer");
        let panel = surface
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .expect("ExplorerPanel");
        assert_eq!(panel.id, 9);
        assert_eq!(
            panel.current_root(),
            std::path::Path::new("/remote/project")
        );
    }

    #[test]
    fn build_mirror_workspace_single_pane_tab() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        assert_eq!(ws.id, 99);
        assert_eq!(ws.all_surface_ids(), vec![50]);
    }

    #[test]
    fn build_mirror_workspace_preserves_survivor_and_inserts_new_leaf() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let ids = IdGenerator::new();
        let survivor_local = 50u32;
        let mut map = HashMap::new();
        map.insert(1u32, survivor_local);
        let new_local = ids.next_surface(); // 역반영이 신규에 발급하는 것과 동형.
        map.insert(2u32, new_local);
        let mut term = HashSet::new();
        term.insert(survivor_local);
        term.insert(new_local);
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
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
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

    #[test]
    fn build_mirror_workspace_empty_tree_fallback() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let ids = IdGenerator::new();
        let map = HashMap::new();
        let term = HashSet::new();
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            1,
            "remote",
            &serde_json::Value::Null,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        assert_eq!(ws.id, 1);
        assert_eq!(ws.all_surface_ids().len(), 1);
    }

    #[test]
    fn build_mirror_workspace_preserves_vertical_pane_split() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        match ws.pane_layout() {
            PaneNode::Split {
                direction,
                ratio,
                second,
                ..
            } => {
                assert_eq!(*direction, SplitDirection::Vertical);
                assert!((*ratio - 0.3).abs() < 0.001);
                if let PaneNode::Leaf(p) = second.as_ref() {
                    assert_eq!(navigation.pane_id(&ws).unwrap(), p.id);
                } else {
                    panic!("expected second to be Leaf");
                }
            }
            _ => panic!("expected Split, got Leaf"),
        }
    }

    #[test]
    fn build_mirror_workspace_falls_back_to_horizontal_chain_without_pane_layout_field() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        });
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
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

    /// pane B의 두 번째 탭에 있는 surface를 원격 ID로 되찾는다.
    #[test]
    fn capture_focused_remote_finds_remote_id_of_locally_focused_surface() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        assert_eq!(
            capture_focused_remote(&navigation, &ws, &map),
            Some(3),
            "focused_pane=pane B, active_tab=tab2(remote 3) 를 정확히 되짚어야 한다"
        );
    }

    /// 원격 포커스와 다른 로컬 포커스를 기억해 구조 재구성 뒤 복원한다.
    #[test]
    fn focus_restore_keeps_client_on_pane_b_after_structural_delta_from_pane_a() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let ids = IdGenerator::new();

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
        let mut before_ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &before_tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );

        let pane_b_surface3_local = *map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&before_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist");
        navigation.select_pane(&before_ws, pane_b_id);
        let pane_b = before_ws
            .pane_layout_mut()
            .find_pane_mut(pane_b_id)
            .expect("pane B exists");
        let tab_index = pane_b
            .tabs
            .iter()
            .position(|t| t.id == tab_id)
            .expect("tab exists");
        navigation.goto_tab(pane_b, tab_index);
        navigation.select_surface(&pane_b.tabs[tab_index], pane_b_surface3_local);

        let old_focused_remote = capture_focused_remote(&navigation, &before_ws, &map);
        assert_eq!(old_focused_remote, Some(3));

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
        let mut after_ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &after_tree,
            &ids,
            &after_map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );

        // Stable local pane/tab IDs preserve the live selection during rebuild.
        assert_eq!(navigation.pane_id(&after_ws), Some(pane_b_id));
        assert_eq!(
            capture_focused_remote(&navigation, &after_ws, &after_map),
            Some(3)
        );

        restore_focus_after_delta(
            &mut navigation,
            &mut after_ws,
            old_focused_remote,
            &after_map,
        );

        let pane_b_surface3_local = *after_map.get(&3).unwrap();
        let (pane_b_id, tab_id) = find_pane_and_tab_for_surface(&after_ws, pane_b_surface3_local)
            .expect("pane B tab2 surface must exist in rebuilt tree");
        assert_eq!(
            navigation.pane_id(&after_ws).unwrap(),
            pane_b_id,
            "복원 후 focus 는 pane A 가 아니라 사용자가 실제로 보던 pane B 에 있어야 한다"
        );
        let pane_b = after_ws
            .pane_layout()
            .find_pane(pane_b_id)
            .expect("pane B exists");
        assert_eq!(
            pane_b.tabs[navigation.tab_index(pane_b)].id,
            tab_id,
            "pane B 의 active_tab 도 사용자가 보던 두 번째 탭이어야 한다"
        );
        assert_eq!(
            navigation
                .surface_id(&pane_b.tabs[navigation.tab_index(pane_b)])
                .unwrap(),
            pane_b_surface3_local,
            "그 탭의 focused_surface 도 정확히 그 surface 를 가리켜야 한다"
        );
    }

    #[test]
    fn focus_restore_is_noop_when_captured_surface_no_longer_exists() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        let mut ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        let untouched_focused_pane = navigation.pane_id(&ws).unwrap();

        restore_focus_after_delta(&mut navigation, &mut ws, Some(3), &map);

        assert_eq!(
            navigation.pane_id(&ws).unwrap(),
            untouched_focused_pane,
            "캡처된 surface 가 없으면 원격이 보낸 focused_pane 그대로 둬야 한다"
        );
    }

    #[test]
    fn set_focus_to_surface_updates_pane_tab_surface_or_reports_false() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
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
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            99,
            "remote",
            &tree,
            &ids,
            &map,
            &term,
            &HashMap::new(),
            &HashMap::new(),
            &mut HashMap::new(),
        );
        let local_b = *map.get(&2).unwrap();

        assert!(set_focus_to_surface(&mut navigation, &ws, local_b));
        let (pane_b_id, tab_b_id) =
            find_pane_and_tab_for_surface(&ws, local_b).expect("pane B exists");
        assert_eq!(navigation.pane_id(&ws).unwrap(), pane_b_id);
        let pane_b = ws.pane_layout().find_pane(pane_b_id).unwrap();
        assert_eq!(pane_b.tabs[navigation.tab_index(pane_b)].id, tab_b_id);
        assert_eq!(
            navigation
                .surface_id(&pane_b.tabs[navigation.tab_index(pane_b)])
                .unwrap(),
            local_b
        );

        assert!(
            !set_focus_to_surface(&mut navigation, &ws, 12345),
            "존재하지 않는 surface 는 false"
        );
    }

    #[test]
    fn remote_structural_op_maps_the_move_target_to_its_remote_id() {
        // remote → local. 로컬 ID가 다른 원격 surface ID와 겹치는 배치다.
        let map: HashMap<u32, u32> = [(40, 3), (41, 4), (3, 9)].into_iter().collect();
        let local = StructuralOp::MoveSurface {
            source_surface_id: 4,
            target_surface_id: 3,
        };
        let wire = remote_structural_op(&local, 41, &map).expect("target mapped");
        assert_eq!(
            wire,
            StructuralOp::MoveSurface {
                source_surface_id: 41,
                target_surface_id: 40,
            },
            "target 도 원격 ID 로 보내야 한다 — 로컬 3 을 그대로 보내면 원격 surface 3 을 가리킨다"
        );
    }

    #[test]
    fn remote_structural_op_drops_a_move_whose_target_is_not_mirrored() {
        let map: HashMap<u32, u32> = [(41, 4)].into_iter().collect();
        let local = StructuralOp::MoveSurface {
            source_surface_id: 4,
            target_surface_id: 7,
        };
        assert_eq!(remote_structural_op(&local, 41, &map), None);
    }

    #[test]
    fn remote_structural_op_only_swaps_the_anchor_for_other_ops() {
        let local = StructuralOp::CloseSurface { surface_id: 4 };
        assert_eq!(
            remote_structural_op(&local, 41, &HashMap::new()),
            Some(StructuralOp::CloseSurface { surface_id: 41 })
        );
    }

    #[test]
    fn pending_op_focus_for_new_tab_and_split_is_new_resource() {
        let map = HashMap::new();
        for op in [
            StructuralOp::NewTab {
                anchor_surface_id: 1,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            StructuralOp::SplitSurface {
                surface_id: 1,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            StructuralOp::SplitPane {
                anchor_surface_id: 1,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            StructuralOp::RestoreClosedItem {
                anchor_surface_id: 1,
            },
        ] {
            assert!(matches!(
                pending_op_focus_for(&op, &[], &map),
                Some(PendingOpFocus::NewResource)
            ));
        }
    }

    #[test]
    fn pending_op_focus_for_close_translates_candidates_or_none() {
        let mut map = HashMap::new();
        map.insert(7u32, 70u32); // remote 7 -> local 70
        map.insert(8u32, 71u32); // remote 8 -> local 71

        let op = StructuralOp::CloseSurface { surface_id: 1 };
        match pending_op_focus_for(&op, &[70, 71], &map) {
            Some(PendingOpFocus::Close { candidates }) => {
                assert_eq!(candidates, vec![7, 8]);
            }
            other => panic!("expected Close{{candidates}}, got {other:?}"),
        }

        assert!(pending_op_focus_for(&op, &[999], &map).is_none());
        assert!(pending_op_focus_for(&op, &[], &map).is_none());
    }

    #[test]
    fn pending_op_focus_for_non_target_ops_is_none() {
        let mut map = HashMap::new();
        map.insert(7u32, 70u32);
        let op = StructuralOp::MoveTab {
            anchor_surface_id: 1,
            from_index: 0,
            to_index: 1,
        };
        assert!(pending_op_focus_for(&op, &[70], &map).is_none());
    }

    #[test]
    fn merge_survivor_mapping_prefers_server_display_name_and_falls_back_to_kind() {
        let ids = IdGenerator::new();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let surfaces = vec![
            serde_json::json!({
                "remote_id": 10,
                "role": "mesh",
                "kind": "markdown",
                "plugin_id": "com.tasty.markdown",
                "display_name": "README.md",
            }),
            serde_json::json!({
                "remote_id": 11,
                "role": "mesh",
                "kind": "image",
                "plugin_id": "com.tasty.image",
            }),
        ];

        let mapping =
            merge_survivor_mapping(&HashMap::new(), &surfaces, &ids, &frame_tx, &mut engine);
        let mesh = &mapping.mesh;

        let local_10 = mapping.remote_to_local[&10];
        let local_11 = mapping.remote_to_local[&11];
        assert_eq!(mesh[&local_10].display_name, "README.md");
        assert_eq!(
            mesh[&local_11].display_name, "image",
            "display_name 필드가 없으면 kind 로 fallback 해야 한다"
        );
    }

    #[test]
    fn merge_survivor_mapping_cleans_up_stale_terminal_on_convert_to_mesh() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        // 별도 발급기를 만들면 기본 workspace의 ID와 충돌하므로 engine의 발급기를 공유한다.
        let ids = engine.next_ids.clone();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let surfaces_v1 = vec![serde_json::json!({
            "remote_id": 10, "role": "terminal", "cols": 80, "rows": 24,
        })];
        let mut m1 =
            merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine);
        let map1 = m1.remote_to_local.clone();
        let local_10 = map1[&10];
        assert!(
            engine.runtime.terminals.get(local_10).is_some(),
            "최초 terminal survivor 는 Terminal 을 만들어야 한다"
        );
        engine
            .remote.attach_mesh_frames
            .update(local_10, vec![1, 2, 3], 1, 1, true);

        // 다음 병합이 이전 kind를 조회할 수 있도록 먼저 실제 트리에 반영한다.
        let tree = serde_json::json!({
            "id": 9, "name": "mirror", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "Shell", "active": true, "focused_surface": 10,
                    "layout": { "type": "Leaf", "id": 10, "kind": "terminal" }
                } ]
            } ]
        });
        let mut ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            999,
            "mirror",
            &tree,
            &ids,
            &map1,
            &m1.terminals,
            &m1.mesh,
            &m1.explorer,
            &mut m1.markdown,
        );
        ws.mirror = true;
        engine.push_mirror_workspace(ws);

        let surfaces_v2 = vec![serde_json::json!({
            "remote_id": 10,
            "role": "mesh",
            "kind": "markdown",
            "plugin_id": "com.tasty.markdown",
            "display_name": "a.md",
        })];
        let m2 = merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine);
        let (map2, term2, mesh2, new2) = (
            &m2.remote_to_local,
            &m2.terminals,
            &m2.mesh,
            &m2.newly_created_remote_ids,
        );

        assert_eq!(
            map2[&10], local_10,
            "local id 는 convert 후에도 유지돼야 한다"
        );
        assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
        assert!(
            !term2.contains(&local_10),
            "markdown 으로 바뀐 뒤에는 더 이상 terminal_locals 에 없어야 한다"
        );
        assert!(
            mesh2.contains_key(&local_10),
            "mesh_locals 에는 새로 등록돼야 한다"
        );
        assert!(
            engine.runtime.terminals.get(local_10).is_none(),
            "옛 Terminal 객체는 즉시 제거돼야 한다"
        );
        assert!(
            engine.remote.attach_mesh_frames.get(local_10).is_none(),
            "옛(terminal 시절의 무의미한) mesh frame 캐시도 제거돼야 한다"
        );
    }

    /// 원격이 닫은 mirror surface는 로컬 닫기처럼 attach 점유 기록도 남기지 않는다.
    /// forwarded terminal.kill이 남긴 soft 점유가 이 경로로 사라져야 한다.
    #[test]
    fn merge_survivor_mapping_forgets_the_occupancy_of_a_remotely_closed_surface() {
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let ids = engine.next_ids.clone();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));
        let parent = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];

        let surfaces_v1 = vec![serde_json::json!({
            "remote_id": 10, "role": "terminal", "cols": 80, "rows": 24,
        })];
        let m1 =
            merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine);
        let local_10 = m1.remote_to_local[&10];
        engine
            .occupy_soft(local_10, parent, None)
            .expect("soft 점유");

        let m2 = merge_survivor_mapping(&m1.remote_to_local, &[], &ids, &frame_tx, &mut engine);
        assert!(m2.remote_to_local.is_empty());
        assert!(
            engine.live.occupancy.occupancy_of(local_10).is_none(),
            "원격이 닫은 surface 의 soft 점유가 남았다"
        );
    }

    #[test]
    fn removing_a_mirror_workspace_forgets_the_occupancy_of_its_surfaces() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let parent = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let event = crate::app::services::apply_create_workspace_inner(
            &mut engine,
            crate::app::services::WorkspaceCreationParams::terminal(),
        )
        .expect("workspace 생성");
        let crate::app::command::CoreEvent::WorkspaceCreated { index, .. } = event else {
            panic!("expected WorkspaceCreated");
        };
        engine.make_mirror_fixture(index);
        let ws_id = engine
            .workspace_at(index)
            .expect("workspace index is valid")
            .id;
        let local = engine
            .workspace_at(index)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        engine.occupy_soft(local, parent, None).expect("soft 점유");

        let map = HashMap::from([(99u32, local)]);
        assert!(remove_mirror_workspace_from_engine(
            &mut engine,
            &mut state,
            ws_id,
            &map
        ));
        assert!(
            engine.live.occupancy.occupancy_of(local).is_none(),
            "정리된 mirror surface 의 soft 점유가 남았다"
        );
    }

    #[test]
    fn merge_survivor_mapping_creates_terminal_when_mesh_survivor_converts_to_terminal() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        // 기본 workspace와 ID가 충돌하지 않도록 engine의 발급기를 공유한다.
        let ids = engine.next_ids.clone();
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let surfaces_v1 = vec![serde_json::json!({
            "remote_id": 20,
            "role": "mesh",
            "kind": "markdown",
            "plugin_id": "com.tasty.markdown",
            "display_name": "a.md",
        })];
        let mut m1 =
            merge_survivor_mapping(&HashMap::new(), &surfaces_v1, &ids, &frame_tx, &mut engine);
        let map1 = m1.remote_to_local.clone();
        let local_20 = map1[&20];
        assert!(
            engine.runtime.terminals.get(local_20).is_none(),
            "mesh survivor 는 애초에 Terminal 이 없어야 한다"
        );

        let tree = serde_json::json!({
            "id": 9, "name": "mirror", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "a.md", "active": true, "focused_surface": 20,
                    "layout": { "type": "Leaf", "id": 20, "kind": "markdown" }
                } ]
            } ]
        });
        let mut ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            999,
            "mirror",
            &tree,
            &ids,
            &map1,
            &m1.terminals,
            &m1.mesh,
            &m1.explorer,
            &mut m1.markdown,
        );
        ws.mirror = true;
        engine.push_mirror_workspace(ws);
        // terminal이 아니었던 surface의 이전 cwd도 지워야 한다.
        engine.set_mirror_surface_cwd(local_20, Some("/srv/remote/docs".to_string()));

        let surfaces_v2 = vec![serde_json::json!({
            "remote_id": 20, "role": "terminal", "cols": 80, "rows": 24,
        })];
        let m2 = merge_survivor_mapping(&map1, &surfaces_v2, &ids, &frame_tx, &mut engine);
        let (map2, term2, new2) = (
            &m2.remote_to_local,
            &m2.terminals,
            &m2.newly_created_remote_ids,
        );

        assert_eq!(
            map2[&20], local_20,
            "local id 는 convert 후에도 유지돼야 한다"
        );
        assert!(new2.is_empty(), "survivor 는 신규 취급되면 안 된다");
        assert!(term2.contains(&local_20));
        assert!(
            !engine.remote.mirror_surface_cwd.contains_key(&local_20),
            "kind 전환은 비-terminal 출발이어도 옛 cwd 를 버린다"
        );
        assert!(
            engine.runtime.terminals.get(local_20).is_some(),
            "mesh → terminal convert 는 새 Terminal 을 만들어야 한다(안 그러면 입력이 안 감)"
        );
    }

    #[test]
    fn cwd_push_applies_as_remote_origin_and_null_clears_it() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let ws_id = 9_000u32;
        let (pane_id, tab_id, local_surface) = (9_001u32, 9_002u32, 9_003u32);
        let remote_surface = 42u32;
        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            pane_id,
            tab_id,
            local_surface,
        );
        mirror_ws.mirror = true;
        engine.push_mirror_workspace(mirror_ws);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        let mut sess = test_session(ws_id, HashMap::from([(remote_surface, local_surface)]));
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        {
            let mut host = MirrorHost::parked(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::Cwd(
                    remote_surface,
                    Some("/srv/remote/proj".to_string()),
                )],
            );
        }
        assert_eq!(
            engine.surface_cwd(local_surface),
            Some(crate::core::state::SurfaceCwd::Remote(
                crate::core::state::RemoteCwd::new("/srv/remote/proj")
            ))
        );
        assert_eq!(
            state.resolve_inherit_cwd_from_surface(&engine.as_ref(), local_surface),
            None,
            "원격 cwd를 로컬 실행 경로로 사용하면 안 된다"
        );

        {
            let mut host = MirrorHost::parked(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::Cwd(remote_surface, None)],
            );
        }
        assert!(
            engine.remote.mirror_surface_cwd.is_empty(),
            "null push 는 옛 원격 경로를 남기지 않는다"
        );
    }

    #[test]
    fn a_loss_notice_becomes_a_desync_event() {
        let payload = serde_json::to_vec(&StreamControl::Loss { frames: 7 }).unwrap();
        assert!(
            matches!(
                mirror_event_from_control(&payload),
                Some(MirrorEvent::Desynced { frames: 7 })
            ),
            "Loss 가 재동기화 이벤트로 옮겨지지 않았다"
        );
    }

    /// wire에서 파싱한 실패 사유가 사용자 안내까지 유지되는지 확인한다.
    #[test]
    fn a_structural_failure_reason_reaches_the_toast_verbatim() {
        // 다른 시험의 전역 번역 초기화와 경쟁하지 않도록 기준 문구를 읽기 전에 초기화한다.
        crate::i18n::init("en");
        let toast_for = |reason: Option<&str>| {
            let payload = serde_json::to_vec(&StreamControl::StructuralResult {
                op_id: 0,
                ok: false,
                reason: reason.map(str::to_string),
            })
            .unwrap();
            let ev = mirror_event_from_control(&payload).expect("실패 회신은 이벤트가 된다");
            let mut sess = test_session(9_000, HashMap::new());
            let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
            let (mut state, mut engine_session) = crate::state::tests::test_state();
            let mut engine = engine_session.borrow_mut();
            {
                let mut host = MirrorHost::windowed(&mut state, &mut engine);
                apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, vec![ev]);
            }
            let messages: Vec<String> = state
                .toasts
                .messages()
                .into_iter()
                .map(str::to_string)
                .collect();
            messages
        };
        let base = crate::i18n::t("attach.toast.mirror_structural_forward_failed").to_string();

        assert_eq!(
            toast_for(Some("unknown surface kind: definitely-not-registered")),
            vec![format!(
                "{base} (unknown surface kind: definitely-not-registered)"
            )],
            "원격 사유가 원문 그대로 괄호 안에 실려야 한다"
        );
        assert_eq!(
            toast_for(None),
            vec![base],
            "wire 에 사유가 없으면 괄호 없는 기본 문구다"
        );
    }

    #[test]
    fn a_desync_detaches_once_renews_the_stream_and_sums_later_notices() {
        use tasty_terminal::{OUTPUT_RETENTION_MAX_BYTES, OutputCursor, OutputReadRequest};

        let _home = crate::test_support::TastyHomeGuard::new();
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let (remote_surface, local_surface) = (42u32, 9_003u32);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        let mut sess = test_session(9_000, HashMap::from([(remote_surface, local_surface)]));
        let (tx, frames_out) = std::sync::mpsc::channel::<OutFrame>();
        sess.transport.frame_tx = Arc::new(Mutex::new(tx));
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        let read = |engine: &crate::runtime::engine_access::EngineRef<'_>, expect: Option<String>| {
            engine
                .runtime
                .terminals
                .get(local_surface)
                .expect("mirror terminal")
                .read_output(&OutputReadRequest {
                    from: OutputCursor::At(0),
                    max_bytes: OUTPUT_RETENTION_MAX_BYTES,
                    strip_ansi: false,
                    expect_stream: expect,
                })
        };
        {
            let mut host = MirrorHost::parked(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::Data(remote_surface, b"before".to_vec())],
            );
        }
        let before = read(&engine.as_ref(), None).expect("read").stream;

        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![
                    MirrorEvent::Desynced { frames: 3 },
                    MirrorEvent::Desynced { frames: 2 },
                ],
            );
        }
        assert_eq!(
            sess.state.resync_pending,
            Some(5),
            "기다리는 동안의 통지는 합산한다"
        );
        let sent: Vec<OutFrame> = frames_out.try_iter().collect();
        assert_eq!(
            sent.len(),
            1,
            "재attach 를 위해 옛 연결을 놓는 것은 한 번이다"
        );
        assert_eq!(sent[0].tag, StreamTag::Detach);
        assert!(
            read(&engine.as_ref(), Some(before)).is_err(),
            "손실 전 표지로 읽으면 stream 불일치여야 한다"
        );
        assert_eq!(
            sess.state.phase,
            SessionState::Connected,
            "재attach 는 EOF 를 본 뒤에 건다 — 통지만으로 상태를 바꾸지 않는다"
        );
    }

    /// 창 없는 수동 mirror는 재attach할 창이 생길 때까지 옛 연결을 유지한다.
    #[test]
    fn a_loss_on_a_parked_mirror_without_an_anchor_waits_for_a_window_instead_of_closing() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let (remote_surface, local_surface) = (42u32, 9_003u32);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        let mut sess = test_session(9_000, HashMap::from([(remote_surface, local_surface)]));
        assert!(
            sess.state.anchor_ws_id.is_none(),
            "전제: 수동 attach(anchor 없음)"
        );
        let (tx, frames_out) = std::sync::mpsc::channel::<OutFrame>();
        sess.transport.frame_tx = Arc::new(Mutex::new(tx));
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        {
            let mut host = MirrorHost::parked(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::Desynced { frames: 4 }],
            );
        }
        assert_eq!(sess.state.resync_pending, Some(4));
        assert!(sess.state.resync_awaiting_window);
        assert!(
            frames_out.try_iter().next().is_none(),
            "parked 에서 옛 연결을 놓으면 재attach 가 창을 못 찾아 mirror 가 정리된다"
        );
        assert_eq!(
            disconnect_disposition(
                sess.transport.disconnected.load(Ordering::SeqCst),
                sess.state.phase,
                sess.resync_released(),
                sess.state.anchor_ws_id.is_some(),
            ),
            DisconnectDisposition::None
        );
        assert_eq!(
            disconnect_disposition(
                true,
                sess.state.phase,
                sess.resync_released(),
                sess.state.anchor_ws_id.is_some(),
            ),
            DisconnectDisposition::Cleanup
        );

        let toasts_before = state.toasts.len();
        {
            let mut host = MirrorHost::parked(&mut state, &mut engine);
            resume_resync_in_window(&mut sess, &mut host);
        }
        assert!(
            frames_out.try_iter().next().is_none(),
            "아직 창이 없으면 놓지 않는다"
        );
        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            resume_resync_in_window(&mut sess, &mut host);
        }
        let sent: Vec<OutFrame> = frames_out.try_iter().collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].tag, StreamTag::Detach);
        assert!(!sess.state.resync_awaiting_window);
        assert_eq!(state.toasts.len(), toasts_before + 1, "재동기화 안내 toast");
        assert_eq!(
            disconnect_disposition(
                true,
                sess.state.phase,
                sess.resync_released(),
                sess.state.anchor_ws_id.is_some(),
            ),
            DisconnectDisposition::Resync,
            "Detach를 보낸 뒤 EOF를 받으면 재attach한다"
        );
    }

    #[test]
    fn a_bulk_transfer_declares_loss_notify_and_aborts_on_a_loss_notice() {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let server = std::thread::spawn(move || {
            let (sock, _) = listener.accept().expect("accept");
            let mut writer = sock.try_clone().expect("clone");
            let mut reader = BufReader::new(sock);
            let mut line = String::new();
            reader.read_line(&mut line).expect("stream.open line");
            let ack = serde_json::to_vec(&tasty_ipc::stream::StreamAck {
                ok: true,
                client_id: Some(5),
                proto: STREAM_PROTO,
                error: None,
            })
            .expect("ack");
            stream::write_frame(&mut writer, StreamTag::Control, &ack).expect("write ack");
            let declared = stream::read_frame(&mut reader).expect("declaration");
            let loss = serde_json::to_vec(&StreamControl::Loss { frames: 1 }).expect("loss");
            stream::write_frame(&mut writer, StreamTag::Control, &loss).expect("write loss");
            declared
        });

        let mut conn = open_bulk_connection(port, 3).expect("bulk connection");
        let err = await_bulk_result(&mut conn, 11).expect_err("결과를 모르면 성공이 아니다");
        let declared = server.join().expect("server");
        assert_eq!(declared.tag, StreamTag::Control);
        assert!(
            matches!(
                serde_json::from_slice::<StreamControl>(&declared.payload),
                Ok(StreamControl::ClientLossNotify {})
            ),
            "bulk 연결이 손실 통지를 선언해야 한다"
        );
        let msg = err.to_string();
        assert!(msg.contains("aborted"), "중단으로 보고해야 한다: {msg}");
        assert!(
            !msg.starts_with(BULK_REJECT_PREFIX),
            "원격 거부로 보이면 재시도가 막힌다: {msg}"
        );
    }

    /// writer 없는 시험 세션. 입력 전송은 실패해도 forwarder가 다음 입력을 기다린다.
    pub(super) fn test_session(
        local_workspace: u32,
        remote_to_local: HashMap<u32, u32>,
    ) -> AttachClientSession {
        let (tx, _rx) = std::sync::mpsc::channel::<OutFrame>();
        AttachClientSession {
            structure_ids: Default::default(),
            local_workspace,
            remote_to_local,
            output: MirrorOutbox::new(),
            disconnected: Arc::new(AtomicBool::new(false)),
            frame_tx: Arc::new(Mutex::new(tx)),
            state: SessionState::Connected,
            client_id: 1,
            remote_workspace: 7,
            bulk_port: 0,
            tunnel: None,
            anchor_ws_id: None,
            op_seq: 0,
            pending_op_focus: HashMap::new(),
            agent_requests: Default::default(),
            next_delta_focus: None,
            last_forwarded_resize: HashMap::new(),
            remote_label: "127.0.0.1:0".to_string(),
            pending_list_dir_consumers: HashMap::new(),
            markdown_locals: HashSet::new(),
            resync_pending: None,
            resync_awaiting_window: false,
        }
    }

    /// 실제 kind 등록 경로를 사용하며 플러그인 프로세스 대신 채널 수신자로 명령을 확인한다.
    fn register_markdown_kind(
        engine: &crate::core::CoreState,
        plugin_id: &str,
    ) -> std::sync::mpsc::Receiver<crate::plugin_bridge::host_cmd::HostCmd> {
        let decl: crate::plugin::manifest::SurfaceKindDecl =
            serde_json::from_value(serde_json::json!({
                "kind": "markdown",
                "display_name_i18n_key": "surface.kind.markdown",
                "rendering": "webview",
            }))
            .expect("test SurfaceKindDecl");
        let (tx, rx) = std::sync::mpsc::channel();
        crate::plugin_bridge::remote_kind::register_remote_kind(
            &engine.runtime.surface_registry,
            plugin_id,
            &decl,
            tx,
        );
        rx
    }

    fn created_surfaces(
        rx: &std::sync::mpsc::Receiver<crate::plugin_bridge::host_cmd::HostCmd>,
    ) -> Vec<(u32, Value)> {
        rx.try_iter()
            .filter_map(|cmd| match cmd {
                crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceCreated {
                    surface_id,
                    params,
                    ..
                } => Some((surface_id, params)),
                _ => None,
            })
            .collect()
    }

    fn markdown_descriptor(remote_id: u32) -> Value {
        serde_json::json!({
            "remote_id": remote_id,
            "role": "markdown",
            "file": "/remote/docs/README.md",
            "display_name": "README.md",
        })
    }

    fn single_leaf_tree(remote_id: u32) -> Value {
        serde_json::json!({
            "id": 9, "name": "mirror", "focused_pane": 7,
            "panes": [ {
                "id": 7,
                "tabs": [ {
                    "id": 3, "name": "README.md", "active": true, "focused_surface": remote_id,
                    "layout": { "type": "Leaf", "id": remote_id, "kind": "markdown" }
                } ]
            } ]
        })
    }

    #[test]
    fn merge_survivor_mapping_builds_local_markdown_surface_for_markdown_role() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
        let ids = engine.next_ids.clone();
        let (tx, _frames) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let mut mapping = merge_survivor_mapping(
            &HashMap::new(),
            &[markdown_descriptor(30)],
            &ids,
            &frame_tx,
            &mut engine,
        );
        let local = mapping.remote_to_local[&30];
        assert_eq!(mapping.markdown_ids(), HashSet::from([local]));

        let created = created_surfaces(&rx);
        assert_eq!(created.len(), 1, "plugin 에 surface.create 가 한 번 간다");
        assert_eq!(created[0].0, local);
        assert_eq!(created[0].1["remote"]["file"], "/remote/docs/README.md");
        assert!(
            created[0].1.get("file").is_none(),
            "원격 경로를 `file` 로 실으면 plugin 이 client 로컬 파일을 읽는다"
        );

        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            999,
            "mirror",
            &single_leaf_tree(30),
            &ids,
            &mapping.remote_to_local,
            &mapping.terminals,
            &mapping.mesh,
            &mapping.explorer,
            &mut mapping.markdown,
        );
        let pane = ws.pane_layout().first_pane().expect("pane");
        let leaf = pane.tabs[0]
            .layout_if_initialized()
            .and_then(|l| l.find_surface(local))
            .expect("markdown leaf");
        assert_eq!(
            leaf.kind(),
            "markdown",
            "빈 surface 가 아니라 markdown surface"
        );
    }

    /// 다른 소유자의 markdown kind는 사용하지 않고 빈 surface로 둔다.
    #[test]
    fn markdown_role_stays_empty_when_another_plugin_owns_the_kind() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let rx = register_markdown_kind(&engine, "com.example.other-markdown");
        let ids = engine.next_ids.clone();
        let (tx, _frames) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let mut mapping = merge_survivor_mapping(
            &HashMap::new(),
            &[markdown_descriptor(30)],
            &ids,
            &frame_tx,
            &mut engine,
        );
        assert!(mapping.markdown.is_empty());
        assert!(created_surfaces(&rx).is_empty());
        let local = mapping.remote_to_local[&30];
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            999,
            "mirror",
            &single_leaf_tree(30),
            &ids,
            &mapping.remote_to_local,
            &mapping.terminals,
            &mapping.mesh,
            &mapping.explorer,
            &mut mapping.markdown,
        );
        let pane = ws.pane_layout().first_pane().expect("pane");
        let leaf = pane.tabs[0]
            .layout_if_initialized()
            .and_then(|l| l.find_surface(local))
            .expect("leaf");
        assert_eq!(leaf.kind(), "empty");
        assert!(!pane.tabs[0].is_surface_deferred(local));
    }

    #[test]
    fn markdown_role_waits_for_the_plugin_kind_and_reifies_as_a_mirror_document() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let ids = engine.next_ids.clone();
        let (tx, _frames) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let mut mapping = merge_survivor_mapping(
            &HashMap::new(),
            &[markdown_descriptor(30)],
            &ids,
            &frame_tx,
            &mut engine,
        );
        let local = mapping.remote_to_local[&30];
        assert_eq!(
            mapping.markdown_ids(),
            HashSet::from([local]),
            "placeholder 도 이 세션의 markdown leaf 로 센다 — destroy·끊김 통지 대상"
        );
        let ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            999,
            "mirror",
            &single_leaf_tree(30),
            &ids,
            &mapping.remote_to_local,
            &mapping.terminals,
            &mapping.mesh,
            &mapping.explorer,
            &mut mapping.markdown,
        );
        assert!(
            ws.pane_layout().first_pane().expect("pane").tabs[0].is_surface_deferred(local),
            "kind 가 없으면 kind 대기 placeholder"
        );
        engine.push_mirror_workspace(ws);

        assert!(
            !engine.reify_plugin_surface(local),
            "kind 등록 전에는 실제화되지 않는다"
        );
        let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
        assert!(engine.reify_plugin_surface(local));

        let leaf = engine.find_surface_by_id(local).expect("leaf");
        assert_eq!(leaf.kind(), "markdown");
        let restored: Vec<Value> = rx
            .try_iter()
            .filter_map(|cmd| match cmd {
                crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceRestored {
                    surface_id,
                    data,
                    ..
                } if surface_id == local => Some(data),
                _ => None,
            })
            .collect();
        assert_eq!(restored.len(), 1, "plugin 에 surface.restore 가 한 번 간다");
        assert_eq!(restored[0]["remote"]["file"], "/remote/docs/README.md");
        assert_eq!(restored[0]["display_name"], "README.md");
        assert!(restored[0].get("file").is_none());
    }

    #[test]
    fn structural_delta_reuses_markdown_survivor_and_reports_removed_ones() {
        let mut navigation = crate::state::navigation::NavigationState::default();
        let mut structure_ids = MirrorStructureIds::default();
        let waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let rx = register_markdown_kind(&engine, MARKDOWN_PLUGIN_ID);
        let ids = engine.next_ids.clone();
        let (tx, _frames) = std::sync::mpsc::channel::<OutFrame>();
        let frame_tx: SharedFrameSender = Arc::new(Mutex::new(tx));

        let mut m1 = merge_survivor_mapping(
            &HashMap::new(),
            &[markdown_descriptor(30)],
            &ids,
            &frame_tx,
            &mut engine,
        );
        let local = m1.remote_to_local[&30];
        let ws_id = 999;
        let mut ws = build_mirror_workspace(
            &mut structure_ids,
            &mut navigation,
            ws_id,
            "mirror",
            &single_leaf_tree(30),
            &ids,
            &m1.remote_to_local,
            &m1.terminals,
            &m1.mesh,
            &m1.explorer,
            &mut m1.markdown,
        );
        ws.mirror = true;
        engine.push_mirror_workspace(ws);
        assert_eq!(created_surfaces(&rx).len(), 1);
        let webview_url = engine
            .find_surface_by_id(local)
            .and_then(|s| {
                s.as_any()
                    .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
            })
            .map(|rs| Arc::clone(&rs.webview_url))
            .expect("RemoteSurface");

        let mut sess = test_session(ws_id, m1.remote_to_local.clone());
        sess.state.markdown_locals = HashSet::from([local]);

        let removed = apply_mirror_structural_delta(
            &mut navigation,
            &mut sess,
            &mut engine,
            7,
            &single_leaf_tree(30),
            &[markdown_descriptor(30)],
            None,
        );
        assert!(removed.is_empty());
        assert!(
            created_surfaces(&rx).is_empty(),
            "survivor 에 surface.create 를 다시 보내면 문서가 로딩부터 다시 시작한다"
        );
        let shared = engine
            .find_surface_by_id(local)
            .and_then(|s| {
                s.as_any()
                    .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
            })
            .expect("still a RemoteSurface");
        assert!(Arc::ptr_eq(&shared.webview_url, &webview_url));

        let removed = apply_mirror_structural_delta(
            &mut navigation,
            &mut sess,
            &mut engine,
            7,
            &single_leaf_tree(30),
            &[serde_json::json!({ "remote_id": 30, "role": "terminal", "cols": 80, "rows": 24 })],
            None,
        );
        assert_eq!(removed, vec![local]);
        assert!(sess.state.markdown_locals.is_empty());
    }

    #[test]
    fn parse_markdown_content_result_reads_both_shapes_and_ignores_other_events() {
        let ok = serde_json::json!({
            "event": "markdown_content_result", "request_id": 4, "surface_id": 30,
            "ok": true, "file": "/r/a.md", "source": "# hi", "truncated": true,
        });
        match parse_markdown_content_result(&serde_json::to_vec(&ok).unwrap()) {
            Some(MirrorEvent::MarkdownContentResult {
                request_id: 4,
                surface_id: 30,
                ok: true,
                file,
                source,
                truncated: true,
                reason: None,
            }) => {
                assert_eq!(file.as_deref(), Some("/r/a.md"));
                assert_eq!(source.as_deref(), Some("# hi"));
            }
            _ => panic!("success shape not parsed"),
        }
        let failed = serde_json::json!({
            "event": "markdown_content_result", "request_id": 5, "surface_id": 30,
            "ok": false, "reason": "permission denied",
        });
        match parse_markdown_content_result(&serde_json::to_vec(&failed).unwrap()) {
            Some(MirrorEvent::MarkdownContentResult {
                ok: false, reason, ..
            }) => assert_eq!(reason.as_deref(), Some("permission denied")),
            _ => panic!("failure shape not parsed"),
        }
        let other = serde_json::json!({ "event": "git_query_result", "request_id": 1 });
        assert!(parse_markdown_content_result(&serde_json::to_vec(&other).unwrap()).is_none());
    }

    #[test]
    fn parse_markdown_changed_reads_the_remote_id_and_ignores_other_events() {
        let changed = serde_json::json!({ "event": "markdown_changed", "surface_id": 30 });
        assert!(matches!(
            parse_markdown_changed(&serde_json::to_vec(&changed).unwrap()),
            Some(MirrorEvent::MarkdownChanged { surface_id: 30 })
        ));
        let result = serde_json::json!({
            "event": "markdown_content_result", "request_id": 4, "surface_id": 30, "ok": true,
        });
        assert!(parse_markdown_changed(&serde_json::to_vec(&result).unwrap()).is_none());
        assert!(parse_markdown_content_result(&serde_json::to_vec(&changed).unwrap()).is_none());
    }

    #[test]
    fn markdown_mirror_local_maps_only_markdown_leaves() {
        let mut sess = test_session(1, HashMap::from([(30, 300), (31, 310)]));
        sess.state.markdown_locals.insert(300);
        assert_eq!(markdown_mirror_local(&sess, 30), Some(300));
        assert_eq!(markdown_mirror_local(&sess, 31), None, "터미널 leaf");
        assert_eq!(
            markdown_mirror_local(&sess, 99),
            None,
            "이 세션이 mirror 하지 않는 문서"
        );
    }

    fn parked_ids(
        parked: &[ParkedEngine],
    ) -> impl Iterator<Item = (EngineId, &crate::core::CoreState)> {
        parked.iter().map(|p| (p.session.id, &p.session.core_state))
    }

    /// 첫 항목만 보는 오류를 잡도록 mirror는 두 번째 parked engine에만 둔다.
    fn parked_with_mirror(ws_id: u32, local_surface: u32) -> Vec<ParkedEngine> {
        let mut parked: Vec<ParkedEngine> = (0..2)
            .map(|_| ParkedEngine::from_test_state(crate::state::tests::test_state()))
            .collect();
        let mut mirror_ws = Workspace::new_with_terminal_marker(
            ws_id,
            "mirror".to_string(),
            9_001,
            9_002,
            local_surface,
        );
        mirror_ws.mirror = true;
        let mut engine = parked[1].session.borrow_mut();
        engine.push_mirror_workspace(mirror_ws);
        engine
            .runtime
            .terminals
            .insert(local_surface, Terminal::new_detached(80, 24), None);
        parked
    }

    #[test]
    fn mirror_output_host_prefers_window_then_parked_then_none() {
        let ws_id = 9_000u32;
        let parked = parked_with_mirror(ws_id, 9_003);
        let wid = winit::window::WindowId::from(7u64);
        assert_eq!(
            mirror_output_host(Some(wid), parked_ids(&parked), ws_id),
            Some(MirrorOutputHost::Window(wid)),
            "창 있는 engine 이 있으면 그쪽"
        );
        assert_eq!(
            mirror_output_host(None, parked_ids(&parked), ws_id),
            Some(MirrorOutputHost::Parked(parked[1].session.id)),
            "창이 없으면 mirror 를 든 parked engine(두 번째)"
        );
        assert_eq!(
            mirror_output_host(None, parked_ids(&parked), 424_242),
            None,
            "어느 engine 에도 없으면 None — drain 하지 않는다"
        );
    }

    #[test]
    fn parked_engine_receives_mirror_data_and_structural_delta() {
        let ws_id = 9_000u32;
        let (survivor_remote, survivor_local, new_remote) = (42u32, 9_003u32, 43u32);
        let mut parked = parked_with_mirror(ws_id, survivor_local);
        let untouched_ws_count = parked[0].session.core_state.workspaces().len();
        let mut sess = test_session(ws_id, HashMap::from([(survivor_remote, survivor_local)]));

        let tree = serde_json::json!({
            "id": 7, "name": "mirror", "focused_pane": 70,
            "panes": [ {
                "id": 70,
                "tabs": [ {
                    "id": 30, "name": "Shell", "active": true, "focused_surface": survivor_remote,
                    "layout": {
                        "type": "Split", "direction": "vertical", "ratio": 0.5,
                        "focus_second": false,
                        "first": { "type": "Leaf", "id": survivor_remote, "kind": "terminal" },
                        "second": { "type": "Leaf", "id": new_remote, "kind": "terminal" }
                    }
                } ]
            } ]
        });
        let surfaces = vec![
            serde_json::json!({ "remote_id": survivor_remote, "role": "terminal", "cols": 80, "rows": 24 }),
            serde_json::json!({ "remote_id": new_remote, "role": "terminal", "cols": 80, "rows": 24 }),
        ];
        let events = vec![
            MirrorEvent::Data(survivor_remote, b"hello-parked".to_vec()),
            MirrorEvent::StructuralDelta {
                workspace_id: 7,
                tree,
                surfaces,
            },
            MirrorEvent::Data(new_remote, b"world-new".to_vec()),
        ];

        let id = find_parked_with_workspace(parked_ids(&parked), ws_id)
            .expect("mirror 를 든 parked engine");
        let pidx = parked
            .iter()
            .position(|p| p.session.id == id)
            .expect("찾은 id의 항목");
        {
            let ParkedEngine {
                view_restore: state,
                session,
            } = &mut parked[pidx];
            let mut engine = session.borrow_mut();
            let mut host = MirrorHost::parked(state, &mut engine);
            let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, events);
        }

        let engine = parked[pidx].session.as_ref();
        let survivor = engine
            .runtime
            .terminals
            .get(survivor_local)
            .expect("survivor mirror 터미널은 delta 뒤에도 같은 local id 로 남는다");
        assert!(
            survivor.screen_text(false).contains("hello-parked"),
            "parked 동안 도착한 Data 가 mirror grid 에 남아야 한다: {:?}",
            survivor.screen_text(false)
        );
        let new_local = *sess
            .remote_to_local
            .get(&new_remote)
            .expect("delta 가 새 remote surface 를 매핑에 넣어야 한다(desync 방지)");
        let fresh = engine
            .runtime
            .terminals
            .get(new_local)
            .expect("delta 가 새 mirror 터미널을 만들어야 한다");
        assert!(
            fresh.screen_text(false).contains("world-new"),
            "delta 이후의 Data 가 갱신된 매핑으로 새 터미널에 라우팅돼야 한다: {:?}",
            fresh.screen_text(false)
        );
        let ws = engine
            .workspaces()
            .into_iter()
            .find(|w| w.id == ws_id)
            .expect("mirror 워크스페이스는 같은 local id 로 교체된다");
        let sids = ws.all_surface_ids();
        assert!(
            sids.contains(&survivor_local) && sids.contains(&new_local),
            "{sids:?}"
        );
        assert_eq!(
            parked[0].session.core_state.workspaces().len(),
            untouched_ws_count,
            "무관한 parked engine 은 건드리지 않는다"
        );
    }

    /// None 분기와 아래 실제 적용 시험을 함께 검사한다.
    /// host 없이 take_for를 호출할 수 없다는 API 제약은 이 실행 시험의 검출 범위와 별개다.
    #[test]
    fn no_host_leaves_the_mirror_buffer_untouched() {
        let ws_id = 9_000u32;
        let mut sess = test_session(ws_id, HashMap::new());
        sess.transport.output.peek().extend([
            MirrorEvent::Data(1, b"a".to_vec()),
            MirrorEvent::Resize(1, 10, 5),
        ]);
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        let applied = apply_pending_mirror_output(&mut sess, None, &mut plugin_manager);

        assert!(!applied, "적용 대상이 없으면 적용했다고 보고하지 않는다");
        let buf = sess.transport.output.peek();
        assert_eq!(
            buf.len(),
            2,
            "host 가 없으면 버퍼는 그대로 남아 다음 호출이 다시 시도한다"
        );
        assert!(matches!(buf[0], MirrorEvent::Data(1, ref b) if b == b"a"));
        assert!(matches!(buf[1], MirrorEvent::Resize(1, 10, 5)));
    }

    #[test]
    fn a_host_drains_and_applies_the_mirror_buffer() {
        let ws_id = 9_000u32;
        let local_surface = 9_003u32;
        let remote_surface = 42u32;
        let mut parked = parked_with_mirror(ws_id, local_surface);
        let mut sess = test_session(ws_id, HashMap::from([(remote_surface, local_surface)]));
        sess.transport.output
            .peek()
            .push(MirrorEvent::Data(remote_surface, b"applied-here".to_vec()));
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;

        let id = find_parked_with_workspace(parked_ids(&parked), ws_id)
            .expect("mirror 를 든 parked engine");
        let pidx = parked
            .iter()
            .position(|p| p.session.id == id)
            .expect("찾은 id의 항목");
        let applied = {
            let ParkedEngine {
                view_restore: state,
                session,
            } = &mut parked[pidx];
            let mut engine = session.borrow_mut();
            apply_pending_mirror_output(
                &mut sess,
                Some(MirrorHost::parked(state, &mut engine)),
                &mut plugin_manager,
            )
        };

        assert!(applied);
        assert!(sess.transport.output.peek().is_empty(), "적용했으면 버퍼는 비워진다");
        let term = parked[pidx]
            .session
            .runtime
            .terminals
            .get(local_surface)
            .expect("mirror 터미널");
        assert!(term.screen_text(false).contains("applied-here"));
    }

    #[test]
    fn parked_host_does_not_stack_toasts_but_windowed_does() {
        let ws_id = 9_000u32;
        let mut sess = test_session(ws_id, HashMap::new());
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        let failure = || vec![MirrorEvent::StructuralFailed(0, Some("nope".to_string()))];

        let (mut parked_state, mut parked_engine_session) = crate::state::tests::test_state();
        let mut parked_engine = parked_engine_session.borrow_mut();
        {
            let mut host = MirrorHost::parked(&mut parked_state, &mut parked_engine);
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
        }
        assert_eq!(
            parked_state.toasts.len(),
            0,
            "창이 없는 engine 에는 toast 를 쌓지 않는다"
        );

        let (mut win_state, mut win_engine_session) = crate::state::tests::test_state();
        let mut win_engine = win_engine_session.borrow_mut();
        {
            let mut host = MirrorHost::windowed(&mut win_state, &mut win_engine);
            apply_mirror_events(&mut sess, &mut host, &mut plugin_manager, failure());
        }
        assert_eq!(
            win_state.toasts.len(),
            1,
            "창이 있으면 같은 이벤트가 toast 를 낸다 — 게이트가 창 유무로만 갈린다"
        );
    }

    /// resize 전후의 출력 순서를 보존하며 버퍼를 비워야 한다.
    #[test]
    fn take_for_takes_everything_in_arrival_order() {
        let buf = MirrorOutbox::new();
        buf.peek().extend([
            MirrorEvent::Data(1, b"a".to_vec()),
            MirrorEvent::Resize(1, 10, 5),
            MirrorEvent::Data(1, b"b".to_vec()),
        ]);
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let host = MirrorHost::parked(&mut state, &mut engine);
        let drained = buf.drain();
        assert!(matches!(drained[0], MirrorEvent::Data(1, ref b) if b == b"a"));
        assert!(matches!(drained[1], MirrorEvent::Resize(1, 10, 5)));
        assert!(matches!(drained[2], MirrorEvent::Data(1, ref b) if b == b"b"));
        assert_eq!(drained.len(), 3);
        assert!(buf.peek().is_empty(), "꺼낸 뒤 버퍼는 비어 있다");
    }
}

fn attach_wake(proxy:&EventLoopProxy<AppEvent>)->Arc<dyn Fn()+Send+Sync> {
    let proxy=proxy.clone();
    Arc::new(move|| {if let Err(error)=proxy.send_event(AppEvent::AttachClientData) {tracing::debug!("remote wake after event loop closed: {error}");}})
}

fn bind_mirror_input(mirror:&mut Terminal,remote_id:u32,frame_tx:&SharedFrameSender,reconnect:bool) {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    if reconnect {mirror.replace_external_connection(tx);} else {mirror.set_input_sink(tx);}
    let frame_tx = frame_tx.clone();
    // 프레임 상한에서 mux 헤더를 뺀 크기로 나눠 paste 순서를 유지한다.
    // 매번 현재 sender를 읽고, 연결이 끊겨 전송에 실패해도 forwarder는 다음 입력을 기다린다.
    // 실패한 청크는 재전송하지 않으며 터미널 sink가 닫히면 스레드도 종료한다.
    std::thread::spawn(move || {
        const MAX_BODY: usize = (stream::MAX_FRAME_LEN as usize) - 4;
        for chunk in rx {
            for part in chunk.chunks(MAX_BODY) {
                let framed = stream::encode_mux(remote_id, part);
                if frame_tx
                    .send(OutFrame {
                        tag: StreamTag::Data,
                        payload: framed,
                    })
                    .is_err()
                {
                    continue;
                }
            }
        }
    });
}
