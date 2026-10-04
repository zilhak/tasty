//! Apply ordered incoming events to windowed or parked mirror engines.

#[cfg(test)]
mod tests;
use super::agent_origin;
use super::connection::find_parked_with_workspace;
use super::navigation::{capture_focused_remote, restore_focus_after_delta, set_focus_to_surface};
use super::projection::{build_mirror_workspace, lease_mirror_ids, mirror_id_needs};
use super::resources::{
    destroy_mirror_markdown_surfaces, install_mirror_fallbacks, push_markdown_changed,
    push_markdown_content_result,
};
use super::survivors::merge_survivor_mapping;
use crate::app::App;
use crate::app::attach_client::{GIT_VIEWER_PLUGIN_ID, GIT_VIEWER_QUERY_RESULT_EVENT};
use crate::app::window_access::engines_mut;
use crate::ipc::stream::StreamTag;
use crate::runtime::engine_access::EngineMut;
use crate::runtime::engine_session::EngineId;
use crate::view::ui::View as _;
use serde_json::Value;
use std::sync::atomic::Ordering;
use tasty_remote::client_session::{
    AttachClientSession, MirrorEvent, PendingOpFocus, SessionState,
};

impl App {
    /// 창이 있는 engine, parked engine 순서로 적용 대상을 찾은 뒤 출력 버퍼를 비운다.
    /// 대상이 없으면 버퍼를 유지한다. 고아 판정·정리도 같은 engine 범위를 확인해야 한다.
    pub(crate) fn apply_attach_client_output(&mut self) {
        self.poll_pending_mirror_installs();
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
            // A handshake/ID reservation continuation owns this epoch until installation or failure.
            // Its original transport (including the tunnel) must not be taken a second time.
            if self
                .state
                .mirror_attempts
                .installing_epoch(sess.state.local_workspace, &sess.transport.frame_tx.epoch())
            {
                continue;
            }
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
    let epoch = sess.transport.frame_tx.epoch();
    let mut remaining = events.into_iter();
    while let Some(ev) = remaining.next() {
        if !epoch.is_active() || !epoch.same(&sess.transport.frame_tx.epoch()) {
            break;
        }
        if let MirrorEvent::StructuralDelta { tree, surfaces, .. } = &ev {
            let admission = mirror_id_needs(tree, surfaces.len(), false).and_then(|needed| {
                host.engine
                    .runtime
                    .ids
                    .ensure(&needed)
                    .map_err(anyhow::Error::new)
            });
            if let Err(error) = admission {
                if matches!(
                    error.downcast_ref::<crate::runtime::id_reservations::ReservationError>(),
                    Some(crate::runtime::id_reservations::ReservationError::Pending)
                ) {
                    let mut tail = vec![ev];
                    tail.extend(remaining);
                    sess.transport.output.restore_front(tail);
                    (host.engine.runtime.waker)();
                } else {
                    tracing::warn!("mirror structural identity reservation failed: {error}");
                    sess.transport.frame_tx.retire();
                    sess.transport.disconnected.store(true, Ordering::Release);
                }
                return;
            }
        }
        apply_one_mirror_event(sess, host, plugin_manager, ev);
    }
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

fn show_mirror_capture_result(
    host: &mut MirrorHost<'_, '_>,
    ok: bool,
    path: Option<String>,
    reason: Option<String>,
) {
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
        MirrorEvent::StructuralFailed(op_id, reason) => agent_origin::apply_structural_failed(
            sess,
            |message, kind| host.toast(message, kind),
            op_id,
            reason,
        ),
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
            match removed_markdown {
                Ok(removed) => {
                    host.state.reconcile_presentation(host.engine);
                    destroy_mirror_markdown_surfaces(plugin_manager, removed);
                }
                Err(error) => {
                    tracing::warn!(
                        "mirror delta could not obtain its fixed identity reservation: {error}"
                    );
                    begin_resync(sess, host, 1);
                }
            }
        }
        MirrorEvent::CaptureResult { ok, path, reason } => {
            show_mirror_capture_result(host, ok, path, reason);
        }
        MirrorEvent::ListDirResult {
            request_id,
            ok,
            dir,
            entries,
            truncated,
            reason,
        } => {
            let entries = entries.map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| crate::core::fs_list::DirEntryInfo {
                        path: dir
                            .as_deref()
                            .map(|dir| std::path::Path::new(dir).join(&entry.name))
                            .unwrap_or_else(|| std::path::PathBuf::from(&entry.name)),
                        name: entry.name,
                        is_dir: entry.is_dir,
                        size: entry.size,
                        modified: entry.modified,
                        ext: entry.ext,
                    })
                    .collect()
            });
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
                    .remote
                    .attach_mesh_frames
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
        .state
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
        agent_origin::notify_markdown_truncated(
            |message, kind| host.toast(message, kind),
            local,
            agent_origin,
        );
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
) -> anyhow::Result<Vec<u32>> {
    let ids = lease_mirror_ids(
        &engine.runtime.ids,
        &engine.runtime.waker,
        tree,
        surfaces.len(),
        false,
    )?;

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
    )?;
    let newly_created_remote_ids = std::mem::take(&mut mapping.newly_created_remote_ids);

    sess.state.remote_to_local = std::mem::take(&mut mapping.remote_to_local);
    let new_markdown = mapping.markdown_ids();
    let removed_markdown: Vec<u32> = sess
        .state
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
        )?;
        install_mirror_fallbacks(&ws, &mut engine.runtime.surfaces);
        ws.mirror = true;

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
    Ok(removed_markdown)
}
