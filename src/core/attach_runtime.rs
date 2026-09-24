//! attach 점유를 터미널 출력·입력, mesh·문서 조회, 구조 변경과 파일 전송에 연결한다.
//! GUI와 헤드리스 메인 루프가 StreamHub의 수신 결과를 이 모듈에 전달한다.

use std::collections::HashMap;
use std::thread;

use crate::core::Core;
use crate::core::CoreState;
use crate::core::attach::{AttachClientId, AttachError};
use crate::model::{AttachSurfaceClass, SurfaceId, WorkspaceId};
use tasty_ipc::stream::{StreamControl, StreamFrame, StreamTag, StructuralOp, encode_mux};
use tasty_ipc::stream_hub::{PushResult, StreamHub};

impl CoreState {
    /// surface를 점유하고 화면 snapshot과 이후 출력을 보낸다. 거절하면 attach_error와 Detach를 보낸다.
    /// hub는 연결을 등록한 허브여야 한다. forwarder는 채널 EOF 또는 다음 push의 끊김 결과로 종료한다.
    pub fn attach_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        if !self.terminals.contains(surface_id) && !self.is_surface_deferred(surface_id) {
            if let Some((kind, plugin_id)) = self.find_mesh_surface_info(surface_id)
                && crate::core::surface_registry::egui_mesh::is_egui_mesh_allowed(&kind, &plugin_id)
            {
                self.attach_mesh_surface_for_stream(surface_id, client_id, hub);
                return;
            }
            reject_attach(hub, client_id, "not_found", None);
            return;
        }

        match self.attach.acquire(surface_id, client_id) {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }

        self.ensure_surface_initialized(surface_id);

        let Some(terminal) = self.terminals.get_mut(surface_id) else {
            let _ = self.attach.release(surface_id, client_id); // 이미 해제됐으면 추가 처리가 필요 없다.
            reject_attach(hub, client_id, "spawn_failed", None);
            return;
        };

        let cols = terminal.cols();
        let rows = terminal.rows();
        // 메인 루프가 터미널을 단독 소유하므로 snapshot과 tap 등록 사이에 ingest가 없다.
        // 이후 허브에서 발생할 수 있는 전송 손실까지 막는 것은 아니다.
        let snapshot = terminal.snapshot_as_vt();
        let tap_rx = terminal.add_output_tap();
        let resize_rx = terminal.add_resize_tap();

        let attached = serde_json::json!({
            "event": "attached",
            "surface_id": surface_id,
            "cols": cols,
            "rows": rows,
        });
        let attached_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&attached).unwrap_or_default(),
        );
        let _ = hub.push(client_id, attached_frame); // 실패한 통지는 재시도하지 않는다.
        let _ = hub.push(client_id, StreamFrame::new(StreamTag::Data, snapshot)); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.

        let hub2 = hub.clone();
        thread::spawn(move || {
            for chunk in tap_rx {
                match hub2.push(client_id, StreamFrame::new(StreamTag::Data, chunk)) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        let hub3 = hub.clone();
        thread::spawn(move || {
            for (cols, rows) in resize_rx {
                let msg = StreamControl::Resize {
                    surface_id,
                    cols,
                    rows,
                };
                let frame = StreamFrame::new(
                    StreamTag::Control,
                    serde_json::to_vec(&msg).unwrap_or_default(),
                );
                match hub3.push(client_id, frame) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        tracing::debug!("attach: surface {surface_id} -> client {client_id}");
    }

    /// mesh surface는 PTY snapshot·tap 없이 점유와 attached 통지만 처리한다.
    /// 실제 구독은 후속 MeshContext 요청을 받은 App 계층에서 시작한다.
    fn attach_mesh_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        match self.attach.acquire(surface_id, client_id) {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }
        let attached = serde_json::json!({
            "event": "attached",
            "surface_id": surface_id,
            "kind": "mesh",
        });
        let attached_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&attached).unwrap_or_default(),
        );
        let _ = hub.push(client_id, attached_frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
        tracing::debug!("attach: mesh surface {surface_id} -> client {client_id}");
    }

    /// 비터미널은 surface lock 없이 workspace 멤버로만 점유될 수도 있어 두 경로를 확인한다.
    fn mesh_holder_matches(&self, surface_id: SurfaceId, client_id: AttachClientId) -> bool {
        self.attach.holder(surface_id) == Some(client_id)
            || self.attach.workspace_holder_of(surface_id) == Some(client_id)
    }

    /// 점유 client의 mesh 구독·geometry·theme·focus 요청을 반영한다. 점유가 다르면 false다.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_attached_mesh_context(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        width_px: u32,
        height_px: u32,
        pixels_per_point: f32,
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        focused: bool,
    ) -> bool {
        if !self.mesh_holder_matches(surface_id, client_id) {
            return false;
        }
        self.mesh_mirror.upsert(
            surface_id,
            client_id,
            width_px,
            height_px,
            pixels_per_point,
            theme,
            focused,
        );
        true
    }

    /// 점유를 확인한 뒤 전체 texture 재전송을 요청한다.
    pub fn apply_attached_mesh_full_resend(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
    ) -> bool {
        if !self.mesh_holder_matches(surface_id, client_id) {
            return false;
        }
        self.mesh_mirror.request_full_resend(surface_id)
    }

    /// 점유를 확인한 뒤 mesh mirror에서 온 입력을 쌓는다.
    pub fn apply_attached_mesh_input(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) -> bool {
        if !self.mesh_holder_matches(surface_id, client_id) {
            return false;
        }
        self.mesh_mirror.push_input(surface_id, input)
    }

    /// 점유한 surface의 PTY로 입력을 보낸다. 서버 로컬 입력 차단은 우회한다.
    /// 해당 점유나 터미널이 없으면 false다.
    pub fn feed_attached_input(&mut self, client_id: AttachClientId, bytes: &[u8]) -> bool {
        let Some(surface_id) = self.attach.surface_held_by(client_id) else {
            return false;
        };
        if let Some(terminal) = self.terminals.get_mut(surface_id) {
            terminal.send_bytes(bytes);
            true
        } else {
            false
        }
    }

    /// workspace의 모든 멤버를 점유하고 트리·surface 정보를 보낸다.
    /// 터미널에는 surface ID를 붙인 snapshot·출력·resize 전송을 등록한다.
    /// mesh·explorer·markdown은 각 role로, 지원하지 않는 종류는 placeholder로 보낸다.
    pub fn attach_workspace_for_stream(
        &mut self,
        workspace_id: u32,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        let Some(idx) = self.find_workspace_index_for_id(workspace_id) else {
            reject_attach(hub, client_id, "workspace_not_found", None);
            return;
        };
        let class = self.workspaces[idx].classify_attach_surfaces();
        // 화면을 복제할 수 없는 멤버도 workspace 점유에 포함한다.
        let members: Vec<SurfaceId> = class
            .terminals
            .iter()
            .chain(class.non_terminals.iter())
            .chain(class.explorers.iter().map(|(sid, _)| sid))
            .chain(class.mesh_candidates.iter().map(|(sid, _, _)| sid))
            .chain(class.content_candidates.iter().map(|(sid, _, _, _)| sid))
            .copied()
            .collect();

        match self
            .attach
            .acquire_workspace(workspace_id, &class.terminals, &members, client_id)
        {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }

        for &sid in &class.terminals {
            self.ensure_surface_initialized(sid);
        }

        let descriptor = self.build_workspace_descriptor(idx, workspace_id, &class);
        let descriptor_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&descriptor).unwrap_or_default(),
        );
        let _ = hub.push(client_id, descriptor_frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.

        for &sid in &class.terminals {
            self.tap_surface_for_stream(sid, client_id, hub);
        }

        let (mesh_whitelisted, _mesh_rejected) = mesh_mirror_candidates(&class);
        let (content_whitelisted, content_rejected) = content_mirror_candidates(&class);
        tracing::debug!(
            "attach: workspace {workspace_id} -> client {client_id} ({} terminals, {} mesh, {} content, {} placeholders)",
            class.terminals.len(),
            mesh_whitelisted.len(),
            content_whitelisted.len(),
            class.non_terminals.len()
                + (class.mesh_candidates.len() - mesh_whitelisted.len())
                + content_rejected.len(),
        );
    }

    /// workspace 연결에 터미널 snapshot과 출력·resize tap을 등록한다.
    /// 메인 루프가 단독 소유하므로 snapshot과 tap 사이에 ingest가 없다.
    /// forwarder는 채널 EOF 또는 다음 push의 끊김 결과로 종료한다.
    pub(crate) fn tap_surface_for_stream(
        &mut self,
        sid: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        let Some(terminal) = self.terminals.get_mut(sid) else {
            return;
        };
        let snapshot = terminal.snapshot_as_vt();
        let tap_rx = terminal.add_output_tap();
        let resize_rx = terminal.add_resize_tap();
        let snapshot_frame = StreamFrame::new(StreamTag::Data, encode_mux(sid, &snapshot));
        let _ = hub.push(client_id, snapshot_frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
        let hub2 = hub.clone();
        thread::spawn(move || {
            for chunk in tap_rx {
                match hub2.push(
                    client_id,
                    StreamFrame::new(StreamTag::Data, encode_mux(sid, &chunk)),
                ) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        let hub3 = hub.clone();
        thread::spawn(move || {
            for (cols, rows) in resize_rx {
                let msg = StreamControl::Resize {
                    surface_id: sid,
                    cols,
                    rows,
                };
                let frame = StreamFrame::new(
                    StreamTag::Control,
                    serde_json::to_vec(&msg).unwrap_or_default(),
                );
                match hub3.push(client_id, frame) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });
    }

    /// 새 workspace 멤버의 점유를 등록한다. 로컬 생성이면 delta를 먼저 보내고 tap한다.
    /// forward 실행 중에는 호출자가 delta 뒤에 tap하므로 여기서는 tap을 생략한다.
    /// notifier가 없어도 점유는 등록한다.
    pub(crate) fn tap_new_workspace_member(
        &mut self,
        workspace_id: WorkspaceId,
        surface_id: SurfaceId,
        is_terminal: bool,
    ) {
        if !self
            .attach
            .add_workspace_member(workspace_id, surface_id, is_terminal)
        {
            return;
        }
        if self.attach.is_auto_tap_suppressed() {
            return;
        }
        // client가 ID 매핑을 만든 뒤 snapshot을 받도록 delta를 먼저 보낸다.
        self.attach.mark_structure_changed(workspace_id);
        self.push_structure_changes();
        if !is_terminal {
            return;
        }
        let Some(holder) = self.attach.workspace_holder(workspace_id) else {
            return;
        };
        let Some(hub) = self.attach.notifier() else {
            return;
        };
        self.tap_surface_for_stream(surface_id, holder, &hub);
    }

    /// 해당 workspace를 점유한 client의 입력만 지정 터미널로 보낸다.
    pub fn feed_attached_workspace_input(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        bytes: &[u8],
    ) -> bool {
        let Some(ws) = self.attach.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.attach.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        if let Some(terminal) = self.terminals.get_mut(remote_surface_id) {
            terminal.send_bytes(bytes);
            true
        } else {
            false
        }
    }

    /// 점유를 확인한 뒤 실제 PTY 크기 변경을 시도한다. 대상·점유가 없으면 false다.
    /// true가 크기 변화를 뜻하지는 않는다. 변화가 있으면 기존 resize tap이 통지한다.
    pub fn apply_attached_workspace_resize(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        cols: usize,
        rows: usize,
    ) -> bool {
        let Some(ws) = self.attach.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.attach.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        if let Some(terminal) = self.terminals.get_mut(remote_surface_id) {
            terminal.resize(cols, rows);
            true
        } else {
            false
        }
    }

    /// mirror에서 사용자가 읽은 attention을 지운다. 해당 workspace의 holder만 요청할 수 있다.
    /// 반환값은 대상과 점유가 맞았는지이며, 실제로 지운 레코드가 있었는지는 구별하지 않는다.
    pub fn apply_attached_attention_clear(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
    ) -> bool {
        let Some(ws) = self.attach.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.attach.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        // 서버 surface에는 mirror의 해제 forward를 다시 쌓지 않는다.
        self.clear_attention(remote_surface_id);
        true
    }

    /// busy 폴링의 변화분을 attach client에 보낸다. Terminal tap 대신 기존 폴링 타이머를 사용한다.
    pub fn forward_busy_activity(&mut self, hub: &StreamHub) {
        for (client_id, surface_id, busy) in self.busy_activity_forwards() {
            let msg = StreamControl::Activity { surface_id, busy };
            let frame = StreamFrame::new(
                StreamTag::Control,
                serde_json::to_vec(&msg).unwrap_or_default(),
            );
            let _ = hub.push(client_id, frame); // 송신 실패에도 변화분 캐시는 갱신돼 같은 값은 다시 보내지 않는다.
        }
    }

    /// surface 소유 인스턴스의 attention 변화분을 mirror에 보낸다.
    /// mirror만으로는 서버 훅의 NeedsInput 등을 계산할 수 없다.
    pub fn forward_attention(&mut self, hub: &StreamHub) {
        for (client_id, surface_id, kind) in self.attention_forwards() {
            let msg = StreamControl::Attention {
                surface_id,
                kind: kind.map(|k| k.to_wire()),
            };
            let frame = StreamFrame::new(
                StreamTag::Control,
                serde_json::to_vec(&msg).unwrap_or_default(),
            );
            let _ = hub.push(client_id, frame); // 송신 실패에도 변화분 캐시는 갱신돼 같은 값은 다시 보내지 않는다.
        }
    }

    /// 서버가 알아낸 cwd의 변화분을 mirror에 보낸다. mirror에는 조회할 로컬 PTY가 없다.
    pub fn forward_surface_cwd(&mut self, hub: &StreamHub) {
        for (client_id, surface_id, cwd) in self.surface_cwd_forwards() {
            let msg = StreamControl::Cwd { surface_id, cwd };
            let frame = StreamFrame::new(
                StreamTag::Control,
                serde_json::to_vec(&msg).unwrap_or_default(),
            );
            let _ = hub.push(client_id, frame); // 송신 실패에도 변화분 캐시는 갱신돼 같은 값은 다시 보내지 않는다.
        }
    }

    fn build_workspace_descriptor(
        &self,
        idx: usize,
        workspace_id: u32,
        class: &AttachSurfaceClass,
    ) -> serde_json::Value {
        let (tree, surfaces) = self.build_workspace_tree_surfaces(idx, class);
        serde_json::json!({
            "event": "attached_workspace",
            "workspace_id": workspace_id,
            "name": self.workspaces[idx].name,
            "tree": tree,
            "surfaces": surfaces,
        })
    }

    /// 초기 attach와 StructuralDelta가 공유하는 트리·surface 정보다.
    pub(crate) fn build_workspace_tree_surfaces(
        &self,
        idx: usize,
        class: &AttachSurfaceClass,
    ) -> (serde_json::Value, Vec<serde_json::Value>) {
        let ws = &self.workspaces[idx];
        let mut kinds: HashMap<u32, &'static str> = HashMap::new();
        let mut display_names: HashMap<u32, String> = HashMap::new();
        for pane_id in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                for tab in &pane.tabs {
                    tab.for_each_surface(&mut |s| {
                        if let Some(id) = s.surface_id() {
                            kinds.insert(id, s.kind());
                            display_names.insert(id, s.display_name());
                        }
                    });
                }
            }
        }
        let (mesh_whitelisted, mesh_rejected) = mesh_mirror_candidates(class);
        let (content_whitelisted, content_rejected) = content_mirror_candidates(class);

        let mut surfaces = Vec::new();
        for &sid in &class.terminals {
            let (cols, rows) = self
                .terminals
                .get(sid)
                .map(|t| (t.cols(), t.rows()))
                .unwrap_or((80, 24));
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "terminal",
                "cols": cols,
                "rows": rows,
            }));
        }
        for (sid, kind, plugin_id) in &mesh_whitelisted {
            let display_name = display_names
                .get(sid)
                .cloned()
                .unwrap_or_else(|| kind.to_string());
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "mesh",
                "kind": kind,
                "plugin_id": plugin_id,
                "display_name": display_name,
            }));
        }
        // explorer는 root만 보낸다. client는 이를 초기 cwd로도 사용한다.
        for (sid, root) in &class.explorers {
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "explorer",
                "root": root.to_string_lossy(),
            }));
        }
        // 파일 내용은 별도 요청으로 받는다. file은 표시용 서버 경로이며 client의 로컬 파일 경로가 아니다.
        for (sid, _kind, file) in &content_whitelisted {
            let display_name = display_names
                .get(sid)
                .cloned()
                .unwrap_or_else(|| kinds.get(sid).copied().unwrap_or("markdown").to_string());
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "markdown",
                "file": file.as_ref().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                "display_name": display_name,
            }));
        }
        for &sid in class
            .non_terminals
            .iter()
            .chain(mesh_rejected.iter())
            .chain(content_rejected.iter())
        {
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "placeholder",
                "kind": kinds.get(&sid).copied().unwrap_or("unknown"),
            }));
        }
        (ws.to_attach_tree_json(), surfaces)
    }
}

/// 서버에서 실행한 구조 변경을 client에 반영하기 위한 결과.
#[derive(Debug)]
pub(crate) struct ForwardedDelta {
    pub delta: tasty_ipc::stream::StreamControl,
    /// 호출자는 delta 직후 새 터미널을 tap해야 client의 ID 매핑보다 snapshot이 먼저 도착하지 않는다.
    pub added_terminals: Vec<SurfaceId>,
    /// 실제로 종류가 바뀐 surface ID. PluginManager를 가진 호출자가 오래된 mesh frame을 지운다.
    pub converted_surface: Option<SurfaceId>,
}

/// mirror의 구조 변경을 서버에서 실행한다. 호출자가 holder를 검증해야 한다.
/// IPC의 권한·자기 대상·hard 점유 검사는 여기서 실행하지 않는다. split·tab·close는
/// structural_exec를 공유하고 convert·restore·move-surface는 Core::apply를 직접 호출한다.
///
/// anchor workspace의 실행 전후 차이로 새 터미널을 찾고 현재 트리를 반환한다.
/// workspace가 사라지면 점유를 해제하고 Ok(None), 실행 실패는 Err(reason)이다.
/// 호출자는 result → delta → 새 터미널 tap 순서로 처리한다. workspace가 사라진 경우에는
/// 이 함수의 강제 분리 통지가 result보다 먼저 나간다.
/// origin이 User인 close만 복원 스택에 남기고 Agent의 close는 남기지 않는다.
pub(crate) fn execute_forwarded_structural_op(
    core: &mut crate::core::Core,
    state: &mut dyn crate::core::cascade_window::CascadeWindow,
    engine: &mut CoreState,
    op: &StructuralOp,
    origin: tasty_ipc::stream::ForwardOrigin,
) -> Result<Option<ForwardedDelta>, String> {
    use crate::core::structural_exec::{self as exec, SplitLevel, SplitRequest};
    use serde_json::json;
    use std::collections::HashSet;

    // close가 anchor를 지워도 변경 후 workspace를 찾을 수 있도록 ID를 먼저 보관한다.
    let ws_id = engine
        .find_workspace_index_for_surface(op.anchor_surface_id())
        .map(|(idx, _)| engine.workspaces[idx].id);
    let before: HashSet<SurfaceId> = ws_id
        .and_then(|id| engine.find_workspace_index_for_id(id))
        .map(|idx| {
            engine.workspaces[idx]
                .all_surface_ids()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

    let mut converted_surface: Option<SurfaceId> = None;
    let restorable = origin == tasty_ipc::stream::ForwardOrigin::User;

    let outcome: Result<(), String> = match op {
        StructuralOp::SplitSurface {
            surface_id,
            direction,
            surface_kind,
            params,
        } => {
            let p = structural_params(
                params,
                json!({
                    "level": "surface",
                    "direction": direction.as_ipc_str(),
                    "target_surface": surface_id,
                    "type": surface_kind,
                }),
            );
            let target_pane = crate::core::param_bag::read_u32(&p, "target_pane")?;
            let req = SplitRequest {
                level: SplitLevel::Surface,
                direction: split_direction(*direction),
                target_surface: Some(*surface_id),
                target_pane,
                params: &p,
            };
            // 결과를 판정하기 전에 자동 tap 억제를 해제해야 오류가 나도 다음 생성에 영향을 주지 않는다.
            engine.attach.set_auto_tap_suppressed(true);
            let result = exec::split(core, state, engine, req);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::SplitPane {
            anchor_surface_id,
            direction,
            surface_kind,
            params,
        } => {
            let p = structural_params(
                params,
                json!({
                    "level": "pane",
                    "direction": direction.as_ipc_str(),
                    "target_surface": anchor_surface_id,
                    "type": surface_kind,
                }),
            );
            let target_pane = crate::core::param_bag::read_u32(&p, "target_pane")?;
            let req = SplitRequest {
                level: SplitLevel::Pane,
                direction: split_direction(*direction),
                target_surface: Some(*anchor_surface_id),
                target_pane,
                params: &p,
            };
            engine.attach.set_auto_tap_suppressed(true);
            let result = exec::split(core, state, engine, req);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::NewTab {
            anchor_surface_id,
            surface_kind,
            params,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} not found"))?;
            let p = structural_params(params, json!({ "pane_id": pane_id, "type": surface_kind }));
            engine.attach.set_auto_tap_suppressed(true);
            let activate = origin == tasty_ipc::stream::ForwardOrigin::User;
            let result = exec::create_tab(core, state, engine, pane_id, &p, activate);
            engine.attach.set_auto_tap_suppressed(false);
            forward_result(result)
        }
        StructuralOp::CloseSurface { surface_id } => {
            // holder의 요청은 도메인 실행을 직접 부른다. params로 점유 검사 면제를 허용하지 않는다.
            forward_result(exec::close_surface(
                core,
                state,
                engine,
                *surface_id,
                restorable,
            ))
        }
        StructuralOp::CloseTab { anchor_surface_id } => {
            let tab_id = engine
                .find_tab_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} tab not found"))?;
            // 닫기 전에 캡처해야 복원에 필요한 트리 정보가 남는다.
            if let Some(item) = restorable
                .then(|| engine.find_pane_for_tab(tab_id))
                .flatten()
                .and_then(|pane_id| {
                    let idx = engine
                        .find_pane_by_id(pane_id)?
                        .tabs
                        .iter()
                        .position(|t| t.id == tab_id)?;
                    Some((pane_id, idx))
                })
                .and_then(|(pane_id, idx)| engine.capture_closed_tab(pane_id, idx))
            {
                engine.push_closed_item(item);
            }
            forward_result(exec::close_tab(core, state, engine, tab_id))
        }
        StructuralOp::ClosePane { anchor_surface_id } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            // 닫기 전에 캡처해야 pane의 split context가 남는다.
            if let Some(item) = restorable
                .then(|| engine.capture_closed_pane(pane_id))
                .flatten()
            {
                engine.push_closed_item(item);
            }
            forward_result(exec::close_pane(core, state, engine, pane_id))
        }
        StructuralOp::MoveTab {
            anchor_surface_id,
            from_index,
            to_index,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            forward_result(exec::move_tab(
                core,
                engine,
                pane_id,
                *from_index,
                *to_index,
            ))
        }
        StructuralOp::ConvertSurface {
            surface_id,
            surface_kind,
            params,
            cwd,
        } => {
            // 요청 cwd가 우선이다. 없으면 서버의 inherit_cwd 설정에 따라 실제 터미널 cwd를 조회한다.
            use crate::core::intent::ConvertSurfaceTarget;
            let carried_cwd = cwd
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .map(std::path::PathBuf::from)
                .or_else(|| state.resolve_inherit_cwd_from_surface(engine, *surface_id));
            let target = if surface_kind == "terminal" {
                ConvertSurfaceTarget::Terminal { cwd: carried_cwd }
            } else {
                ConvertSurfaceTarget::Kind {
                    cwd: carried_cwd,
                    kind: surface_kind.clone(),
                    params: params.clone(),
                }
            };
            let intent = crate::core::intent::DomainIntent::ConvertSurface {
                surface_id: *surface_id,
                target,
            };
            match core.apply(engine, intent) {
                Ok(events) => match events.into_iter().next() {
                    Some(crate::core::intent::CoreEvent::SurfaceConverted {
                        replaced: true,
                        ..
                    }) => {
                        converted_surface = Some(*surface_id);
                        Ok(())
                    }
                    // 도메인이 낸 실패 이유를 그대로 보내 원인을 다른 오류로 바꾸지 않는다.
                    Some(crate::core::intent::CoreEvent::SurfaceConverted {
                        failure: Some(reason),
                        ..
                    }) => Err(reason),
                    _ => Err(convert_failure_fallback(*surface_id)),
                },
                Err(e) => Err(e.to_string()),
            }
        }
        StructuralOp::RestoreClosedItem { anchor_surface_id } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            let ws_id = engine
                .find_workspace_index_for_pane(pane_id)
                .map(|idx| engine.workspaces[idx].id)
                .ok_or_else(|| format!("pane {pane_id} workspace not found"))?;
            let intent = crate::core::intent::DomainIntent::RestoreClosedItem {
                target_pane_id: Some(pane_id),
                // 다른 workspace에서 닫힌 항목은 복원하지 않는다.
                scope: crate::core::intent::RestoreScope::Workspace(ws_id),
            };
            match core.apply(engine, intent) {
                Ok(events) => {
                    let restored = matches!(
                        events.into_iter().next(),
                        Some(crate::core::intent::CoreEvent::ClosedItemRestored {
                            restored: true,
                            ..
                        })
                    );
                    if restored {
                        // 로컬 복원 후속 처리는 서버 사용자의 workspace·pane 선택을 바꾸므로 호출하지 않는다.
                        // mirror의 선택은 client가 delta를 적용할 때 처리한다.
                        Ok(())
                    } else {
                        // client가 빈 복원 목록 안내를 구별하도록 전용 sentinel을 반환한다.
                        Err(tasty_ipc::stream::STRUCTURAL_REASON_RESTORE_EMPTY.to_string())
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
        StructuralOp::MoveSurface {
            source_surface_id,
            target_surface_id,
        } => {
            // 이 실행 함수 자체는 source와 target이 같은 workspace인지 검사하지 않는다.
            let intent = crate::core::intent::DomainIntent::MoveSurface {
                source_surface_id: *source_surface_id,
                target_surface_id: *target_surface_id,
            };
            match core.apply(engine, intent) {
                Ok(events) => {
                    let ev = events.into_iter().next();
                    if !matches!(
                        ev,
                        Some(crate::core::intent::CoreEvent::MoveSurfaceApplied { .. })
                    ) {
                        return Err("Core::apply returned no MoveSurfaceApplied event".to_string());
                    }
                    match ev.and_then(|ev| {
                        crate::core::structural_cascade::SurfaceCloseCascade::from_move_surface_applied(
                            ev, false,
                        )
                    }) {
                        Some(c) => {
                            crate::core::structural_cascade::cascade_surface_closed(
                                core, state, engine, c,
                            );
                            Ok(())
                        }
                        None => Err(format!(
                            "move failed: source={source_surface_id} target={target_surface_id}"
                        )),
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }
    };

    outcome?;

    // 이 경로가 delta를 반환하므로 일반 구조 변경 통지가 같은 트리를 다시 보내지 않게 한다.
    if let Some(ws_id) = ws_id {
        engine.attach.clear_structure_changed(ws_id);
    }

    let Some(ws_id) = ws_id else {
        return Ok(None);
    };
    let Some(idx_after) = engine.find_workspace_index_for_id(ws_id) else {
        // 보낼 트리가 없으므로 점유를 지우고 강제 분리한다. 호출자의 StructuralResult보다
        // 이 통지가 먼저 나가며, client는 분리 뒤의 결과 프레임을 읽지 않을 수 있다.
        engine.attach.force_detach_workspace(ws_id);
        return Ok(None);
    };
    let class = engine.workspaces[idx_after].classify_attach_surfaces();
    let added_terminals: Vec<SurfaceId> = class
        .terminals
        .iter()
        .copied()
        .filter(|sid| !before.contains(sid))
        .collect();
    // 새 터미널도 workspace 점유에 넣어 입력·resize의 holder 검사와 서버 읽기 전용 표시를 유지한다.
    for sid in &added_terminals {
        engine.attach.add_workspace_member(ws_id, *sid, true);
    }
    let (tree, surfaces) = engine.build_workspace_tree_surfaces(idx_after, &class);
    let delta = tasty_ipc::stream::StreamControl::StructuralDelta {
        workspace_id: ws_id,
        tree,
        surfaces,
    };
    Ok(Some(ForwardedDelta {
        delta,
        added_terminals,
        converted_surface,
    }))
}

/// 성공 값은 버리고 실패 문구는 그대로 전달한다. 다른 mirror로 다시 forward한 결과도 성공으로 취급한다.
fn forward_result<T>(
    result: Result<T, crate::core::structural_exec::StructuralFailure>,
) -> Result<(), String> {
    use crate::core::structural_exec::StructuralFailure;
    match result {
        Ok(_) => Ok(()),
        Err(StructuralFailure::Rejected(msg)) => Err(msg),
        Err(StructuralFailure::MissingEvent(msg)) => Err(msg.to_string()),
        Err(StructuralFailure::Apply(e)) => {
            if e.downcast_ref::<crate::core::MirrorStructuralBlocked>()
                .is_some_and(|blocked| blocked.forwarded)
            {
                Ok(())
            } else {
                Err(e.to_string())
            }
        }
    }
}

/// 도메인이 실패 이유를 빠뜨렸을 때 쓴다. 원인을 추측해 not found 등으로 바꾸지 않는다.
fn convert_failure_fallback(surface_id: SurfaceId) -> String {
    format!("surface {surface_id} was not converted")
}

fn split_direction(axis: tasty_ipc::stream::SplitAxis) -> crate::model::SplitDirection {
    match axis {
        tasty_ipc::stream::SplitAxis::Horizontal => crate::model::SplitDirection::Horizontal,
        tasty_ipc::stream::SplitAxis::Vertical => crate::model::SplitDirection::Vertical,
    }
}

/// base 객체에 제어 키를 덮어쓴다. base가 객체가 아니면 빈 객체에서 시작한다.
/// 이 묶음은 IPC params와 같은 경로로 읽히고 새 surface의 params에도 남는다.
fn structural_params(base: &serde_json::Value, control: serde_json::Value) -> serde_json::Value {
    let mut obj = base.as_object().cloned().unwrap_or_default();
    if let Some(ctrl) = control.as_object() {
        for (k, v) in ctrl {
            obj.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(obj)
}

/// 업로드 버퍼를 회수하고 이 engine의 workspace를 하나라도 점유한 client인지 확인한다.
/// 허용되면 캡처를 저장하고 서버 클립보드에 경로를 쓴 뒤 capture_result로 회신한다.
pub(crate) fn finalize_capture_upload(
    engine: &mut CoreState,
    core: &Core,
    hub: &StreamHub,
    client_id: u32,
    upload_id: u64,
    file_name: &str,
) {
    let bytes = engine.capture_uploads.take(client_id, upload_id);
    let is_holder = engine.attach.client_holds_workspace(client_id);
    let result = match (is_holder, bytes) {
        (false, _) => Err("client does not hold a workspace attach".to_string()),
        (true, None) => Err("no uploaded bytes for this upload_id".to_string()),
        (true, Some(bytes)) => save_capture_and_set_clipboard(core, file_name, &bytes),
    };
    let payload = match &result {
        Ok(path) => serde_json::json!({
            "event": "capture_result",
            "upload_id": upload_id,
            "ok": true,
            "path": path,
        }),
        Err(reason) => serde_json::json!({
            "event": "capture_result",
            "upload_id": upload_id,
            "ok": false,
            "reason": reason,
        }),
    };
    let frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&payload).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

/// 캡처를 저장한 뒤 경로를 클립보드에 쓴다. 클립보드 기록에 실패해도 파일은 남는다.
fn save_capture_and_set_clipboard(
    core: &Core,
    file_name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let dir = crate::paths::tasty_home()
        .map(|h| h.join("screenshots"))
        .ok_or_else(|| "no tasty home directory".to_string())?;
    let path_str = save_bulk_file(&dir, file_name, "screenshot.png", bytes)?;
    core.clipboard_arc()
        .write_text(&path_str)
        .map_err(|e| e.to_string())?;
    Ok(path_str)
}

/// file_name의 마지막 경로 요소를 dir 아래에 저장한다. 이름이 없으면 fallback_name을 쓴다.
/// 기존 파일을 덮어쓸 수 있으며 원자적 저장이나 symlink 검증은 하지 않는다.
/// 반환 경로가 절대경로인지는 전달한 dir에 달려 있다.
fn save_bulk_file(
    dir: &std::path::Path,
    file_name: &str,
    fallback_name: &str,
    bytes: &[u8],
) -> Result<String, String> {
    let safe_name = std::path::Path::new(file_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(safe_name);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

pub(crate) fn default_bulk_transfer_dir() -> Option<std::path::PathBuf> {
    crate::paths::tasty_home().map(|h| h.join("transfers"))
}

/// 설정 경로가 비어 있으면 기본 transfers 폴더를 쓴다. begin의 사용량 계산과 commit 저장이 공유한다.
pub(crate) fn resolve_bulk_transfer_dir(
    settings: &tasty_settings::Settings,
) -> Option<std::path::PathBuf> {
    let configured = settings.remote_transfer.dir.trim();
    if configured.is_empty() {
        default_bulk_transfer_dir()
    } else {
        Some(std::path::PathBuf::from(configured))
    }
}

/// 바로 아래 일반 파일의 크기만 더한다. 디렉터리 읽기 실패는 0, 개별 항목·metadata 오류는 건너뛴다.
pub(crate) fn dir_used_bytes(dir: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// 포화 덧셈 결과가 상한보다 큰지 확인한다. 같으면 허용하므로 상한이 u64::MAX면 포화값도 허용된다.
fn exceeds_capacity(used: u64, incoming: u64, max_bytes: u64) -> bool {
    used.saturating_add(incoming) > max_bytes
}

/// 저장 폴더의 사용량과 total_size로 begin을 허용할지 결정한다. 초과하면 등록 없이 거절한다.
/// 동시 전송의 미저장 바이트를 예약하거나 commit 때 다시 용량을 검사하지 않는다.
pub(crate) fn begin_bulk_transfer(
    engine: &mut CoreState,
    hub: &StreamHub,
    client_id: u32,
    transfer_id: u64,
    filename: String,
    total_size: u64,
) {
    let dir = resolve_bulk_transfer_dir(&engine.settings);
    let max_bytes = engine
        .settings
        .remote_transfer
        .max_mb
        .saturating_mul(1024 * 1024);
    let used = dir.as_deref().map(dir_used_bytes).unwrap_or(0);
    if exceeds_capacity(used, total_size, max_bytes) {
        tracing::warn!(
            "bulk transfer: capacity exceeded (used={used}, incoming={total_size}, max={max_bytes}) — rejecting begin"
        );
        let reply = StreamControl::BulkResult {
            transfer_id,
            ok: false,
            path: None,
            reason: Some("capacity exceeded".to_string()),
        };
        let frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&reply).unwrap_or_default(),
        );
        let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
        return;
    }
    engine
        .bulk_transfers
        .begin(client_id, transfer_id, filename, total_size);
}

/// 연결에 지정된 bulk_workspace를 누군가 점유하고 있는지 확인한 뒤 저장하고 경로를 회신한다.
/// bulk client 자체의 점유나 SSH 연결의 동일성을 여기서 확인하지는 않는다.
/// dir가 없으면 저장할 수 없으며 클립보드는 변경하지 않는다.
pub(crate) fn finalize_bulk_transfer(
    engine: &mut CoreState,
    hub: &StreamHub,
    client_id: u32,
    transfer_id: u64,
    bulk_workspace: WorkspaceId,
    dir: Option<std::path::PathBuf>,
) {
    let authorized = engine.attach.workspace_holder(bulk_workspace).is_some();
    // 권한 확인에 실패해도 버퍼를 회수해 큰 업로드가 메모리에 남지 않게 한다.
    let taken = engine.bulk_transfers.take(client_id, transfer_id);
    let result = if !authorized {
        Err("bound workspace has no active holder".to_string())
    } else {
        match (taken, dir) {
            (None, _) => Err("no uploaded bytes for this transfer_id".to_string()),
            (Some(_), None) => Err("no tasty home directory".to_string()),
            (Some((filename, bytes)), Some(d)) => {
                save_bulk_file(&d, &filename, "bulk-file", &bytes)
            }
        }
    };
    let reply = match result {
        Ok(path) => StreamControl::BulkResult {
            transfer_id,
            ok: true,
            path: Some(path),
            reason: None,
        },
        Err(reason) => StreamControl::BulkResult {
            transfer_id,
            ok: false,
            path: None,
            reason: Some(reason),
        },
    };
    let frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&reply).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

/// 이 engine의 workspace를 하나라도 점유한 client의 디렉터리 조회를 처리한다.
/// plugin IPC의 FsRead 검사는 적용하지 않으며 경로를 점유 workspace 내부로 제한하지 않는다.
pub(crate) fn handle_list_dir_request(
    engine: &mut CoreState,
    hub: &StreamHub,
    client_id: u32,
    request_id: u64,
    dir: &str,
) {
    let is_holder = engine.attach.client_holds_workspace(client_id);
    let result = if !is_holder {
        Err("client does not hold a workspace attach".to_string())
    } else {
        list_dir_for_request(dir)
    };
    let payload = match result {
        Ok((resolved_dir, entries)) => {
            let (wire_entries, truncated) = list_dir_entries_wire_capped(&entries);
            serde_json::json!({
                "event": "list_dir_result",
                "request_id": request_id,
                "ok": true,
                "dir": resolved_dir,
                "entries": wire_entries,
                "truncated": truncated,
            })
        }
        Err(reason) => serde_json::json!({
            "event": "list_dir_result",
            "request_id": request_id,
            "ok": false,
            "reason": reason,
        }),
    };
    let frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&payload).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

/// 빈 경로면 서버 홈을, 아니면 요청한 경로를 읽는다. 디렉터리 우선·이름순으로 반환한다.
fn list_dir_for_request(
    dir: &str,
) -> Result<(String, Vec<crate::core::fs_list::DirEntryInfo>), String> {
    let path = if dir.trim().is_empty() {
        directories::BaseDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .ok_or_else(|| "no home directory".to_string())?
    } else {
        std::path::PathBuf::from(dir)
    };
    let mut entries = crate::core::fs_list::read_dir_entries(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "permission denied".to_string()
        } else {
            e.to_string()
        }
    })?;
    crate::core::fs_list::sort_entries(
        &mut entries,
        tasty_model::SortColumn::Name,
        tasty_model::SortDir::Asc,
    );
    Ok((path.to_string_lossy().to_string(), entries))
}

/// entries의 직렬화 크기 제한. 프레임 상한을 넘기면 연결이 종료되므로 나머지 필드의 여유를 둔다.
/// 전체 프레임을 다시 재는 것은 아니며 각 entry의 직렬화 크기만 합산한다.
const LIST_DIR_ENTRIES_BYTE_BUDGET: usize = 700 * 1024;

/// 예산 안에 들어가는 entry만 반환한다. 두 번째 값은 생략한 항목이 있는지다.
fn list_dir_entries_wire_capped(
    entries: &[crate::core::fs_list::DirEntryInfo],
) -> (Vec<serde_json::Value>, bool) {
    list_dir_entries_wire_capped_with_budget(entries, LIST_DIR_ENTRIES_BYTE_BUDGET)
}

fn list_dir_entries_wire_capped_with_budget(
    entries: &[crate::core::fs_list::DirEntryInfo],
    mut budget: usize,
) -> (Vec<serde_json::Value>, bool) {
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let wire = list_dir_entry_wire(e);
        let approx_len = serde_json::to_vec(&wire).map(|b| b.len()).unwrap_or(0);
        if approx_len > budget {
            return (out, true);
        }
        budget -= approx_len;
        out.push(wire);
    }
    (out, false)
}

/// modified는 Unix epoch 초로 보낸다. 사람이 읽는 날짜 표기는 client가 만든다.
fn list_dir_entry_wire(e: &crate::core::fs_list::DirEntryInfo) -> serde_json::Value {
    let modified_unix = e
        .modified
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    serde_json::json!({
        "name": e.name,
        "is_dir": e.is_dir,
        "size": e.size,
        "modified_unix": modified_unix,
        "ext": e.ext,
    })
}

/// workspace를 하나라도 점유한 client의 Git 조회를 처리한다.
/// worktree_path가 있으면 그 경로를 쓰고, 없으면 서버 터미널에서 cwd를 조회한다.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_git_query_request(
    engine: &mut CoreState,
    hub: &StreamHub,
    client_id: u32,
    request_id: u64,
    surface_id: u32,
    kind: tasty_ipc::stream_hub::GitQueryKind,
    worktree_path: Option<String>,
    diff_path: Option<String>,
) {
    use tasty_ipc::stream_hub::GitQueryKind;

    let is_holder = engine.attach.client_holds_workspace(client_id);
    let result: Result<serde_json::Value, String> = if !is_holder {
        Err("client does not hold a workspace attach".to_string())
    } else {
        match kind {
            GitQueryKind::Snapshot => {
                git_query_snapshot(engine, surface_id, worktree_path.as_deref())
            }
            GitQueryKind::Diff => match diff_path.as_deref() {
                Some(p) if !p.trim().is_empty() => {
                    git_query_diff(engine, surface_id, worktree_path.as_deref(), p)
                }
                _ => Err("missing diff_path for kind=diff".to_string()),
            },
        }
    };
    let kind_wire = kind.as_wire_str();
    let payload = match result {
        Ok(mut data) => {
            let obj = data
                .as_object_mut()
                .expect("git query helpers return objects");
            obj.insert("event".to_string(), serde_json::json!("git_query_result"));
            obj.insert("request_id".to_string(), serde_json::json!(request_id));
            obj.insert("ok".to_string(), serde_json::json!(true));
            obj.insert("kind".to_string(), serde_json::json!(kind_wire));
            data
        }
        Err(reason) => serde_json::json!({
            "event": "git_query_result",
            "request_id": request_id,
            "ok": false,
            "kind": kind_wire,
            "reason": reason,
        }),
    };
    let frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&payload).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

fn resolve_git_query_target(
    engine: &CoreState,
    surface_id: u32,
    worktree_path: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    if let Some(p) = worktree_path {
        if p.trim().is_empty() {
            return Err("empty worktree_path".to_string());
        }
        return Ok(std::path::PathBuf::from(p));
    }
    engine
        .terminals
        .get(surface_id)
        .and_then(|t| t.get_cwd())
        .ok_or_else(|| "remote surface has no known cwd".to_string())
}

/// Git 상태·로그·worktree를 수집한다. worktree는 모두 싣고 남은 예산으로 상태·로그를 자른다.
/// worktree 목록 자체가 예산을 넘을 수 있으며 전체 프레임 크기를 다시 검사하지 않는다.
fn git_query_snapshot(
    engine: &CoreState,
    surface_id: u32,
    worktree_path: Option<&str>,
) -> Result<serde_json::Value, String> {
    let target = resolve_git_query_target(engine, surface_id, worktree_path)?;
    let repo = tasty_git_core::discover_repo(&target)
        .ok_or_else(|| "no git repository found".to_string())?;
    let current_wd = repo
        .workdir()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| repo.path().to_path_buf());
    let worktrees =
        tasty_git_core::collect_worktrees(&repo, &current_wd).map_err(|e| e.to_string())?;
    let active = worktrees.iter().find(|w| w.is_current);
    let branch = active.and_then(|w| w.branch.clone());
    let oid = active.and_then(|w| w.oid.clone());

    let status = tasty_git_core::collect_status(&repo).map_err(|e| e.to_string())?;
    let log = tasty_git_core::collect_log(&repo, GIT_QUERY_LOG_LIMIT).map_err(|e| e.to_string())?;

    let worktrees_wire: Vec<serde_json::Value> =
        worktrees.iter().map(worktree_entry_wire).collect();
    let mut budget = GIT_QUERY_BYTE_BUDGET.saturating_sub(wire_values_len(&worktrees_wire));
    let (status_wire, truncated_status) =
        cap_wire_values(budget, status.iter().map(status_entry_wire).collect());
    budget = budget.saturating_sub(wire_values_len(&status_wire));
    let (log_wire, truncated_log) =
        cap_wire_values(budget, log.iter().map(log_entry_wire).collect());

    Ok(serde_json::json!({
        "active_worktree_path": display_path_lossy(&current_wd),
        "branch": branch,
        "oid": oid,
        "worktrees": worktrees_wire,
        "status_entries": status_wire,
        "truncated_status": truncated_status,
        "log_entries": log_wire,
        "truncated_log": truncated_log,
    }))
}

/// 파일 diff를 hunk 단위로 예산에 맞춰 반환한다. 응답의 나머지 필드는 이 예산에 포함하지 않는다.
fn git_query_diff(
    engine: &CoreState,
    surface_id: u32,
    worktree_path: Option<&str>,
    diff_path: &str,
) -> Result<serde_json::Value, String> {
    let target = resolve_git_query_target(engine, surface_id, worktree_path)?;
    let repo = tasty_git_core::discover_repo(&target)
        .ok_or_else(|| "no git repository found".to_string())?;
    let diff = tasty_git_core::collect_diff(&repo, diff_path).map_err(|e| e.to_string())?;
    let (hunks_wire, truncated) = cap_diff_hunks(GIT_QUERY_BYTE_BUDGET, &diff.hunks);
    Ok(serde_json::json!({
        "file_path": diff.file_path,
        "hunks": hunks_wire,
        "truncated_diff": truncated,
    }))
}

fn display_path_lossy(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

fn worktree_entry_wire(w: &tasty_git_core::WorktreeEntry) -> serde_json::Value {
    serde_json::json!({
        "name": w.name,
        "path": display_path_lossy(&w.path),
        "branch": w.branch,
        "oid": w.oid,
        "is_main": w.is_main,
        "is_current": w.is_current,
        "locked": w.locked,
        "lock_reason": w.lock_reason,
        "is_valid": w.is_valid,
    })
}

fn file_status_wire(s: tasty_git_core::FileStatus) -> &'static str {
    use tasty_git_core::FileStatus;
    match s {
        FileStatus::Modified => "modified",
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Renamed => "renamed",
        FileStatus::Untracked => "untracked",
        FileStatus::Conflicted => "conflicted",
    }
}

fn status_entry_wire(e: &tasty_git_core::StatusEntry) -> serde_json::Value {
    serde_json::json!({ "status": file_status_wire(e.status), "path": e.path })
}

fn log_entry_wire(e: &tasty_git_core::LogEntry) -> serde_json::Value {
    serde_json::json!({
        "oid_short": e.oid_short,
        "summary": e.summary,
        "author": e.author,
        "time": e.time,
        "refs": e.refs,
    })
}

fn diff_line_wire(l: &tasty_git_core::DiffLine) -> serde_json::Value {
    use tasty_git_core::DiffLineKind;
    let kind = match l.kind {
        DiffLineKind::Context => "context",
        DiffLineKind::Addition => "addition",
        DiffLineKind::Deletion => "deletion",
    };
    serde_json::json!({
        "kind": kind,
        "content": l.content,
        "old_lineno": l.old_lineno,
        "new_lineno": l.new_lineno,
    })
}

fn diff_hunk_wire(h: &tasty_git_core::DiffHunk) -> serde_json::Value {
    serde_json::json!({
        "header": h.header,
        "lines": h.lines.iter().map(diff_line_wire).collect::<Vec<_>>(),
    })
}

/// 상태·로그·diff의 직렬화 데이터 제한. 나머지 응답 필드를 위해 프레임 상한보다 작게 둔다.
const GIT_QUERY_BYTE_BUDGET: usize = 700 * 1024;

const GIT_QUERY_LOG_LIMIT: usize = 200;

fn wire_values_len(items: &[serde_json::Value]) -> usize {
    items
        .iter()
        .map(|v| serde_json::to_vec(v).map(|b| b.len()).unwrap_or(0))
        .sum()
}

/// 각 value의 직렬화 길이를 더해 예산 안에서 자른다. 배열 괄호·쉼표는 합계에 포함하지 않는다.
fn cap_wire_values(
    mut budget: usize,
    items: Vec<serde_json::Value>,
) -> (Vec<serde_json::Value>, bool) {
    let mut out = Vec::with_capacity(items.len());
    for v in items {
        let len = serde_json::to_vec(&v).map(|b| b.len()).unwrap_or(0);
        if len > budget {
            return (out, true);
        }
        budget -= len;
        out.push(v);
    }
    (out, false)
}

/// hunk를 통째로 포함하거나 제외하며 hunk 내부의 줄을 일부만 보내지 않는다.
fn cap_diff_hunks(
    mut budget: usize,
    hunks: &[tasty_git_core::DiffHunk],
) -> (Vec<serde_json::Value>, bool) {
    let mut out = Vec::with_capacity(hunks.len());
    for h in hunks {
        let wire = diff_hunk_wire(h);
        let len = serde_json::to_vec(&wire).map(|b| b.len()).unwrap_or(0);
        if len > budget {
            return (out, true);
        }
        budget -= len;
        out.push(wire);
    }
    (out, false)
}

/// workspace를 하나라도 점유한 client에게 markdown 원문을 보낸다.
/// 동기 요청이라 plugin에 되묻지 않고 host가 파일을 직접 읽는다. plugin의 대용량 표시 확인과는 별개다.
/// 파일을 모두 읽은 뒤 응답을 자르므로 이 예산이 읽기 메모리 사용량을 제한하지는 않는다.
pub(crate) fn handle_markdown_content_request(
    engine: &mut CoreState,
    hub: &StreamHub,
    client_id: u32,
    request_id: u64,
    surface_id: u32,
) {
    let is_holder = engine.attach.client_holds_workspace(client_id);
    let result = if !is_holder {
        Err("client does not hold a workspace attach".to_string())
    } else {
        markdown_content_for_request(engine, surface_id)
    };
    let payload = match result {
        Ok((file, source, truncated)) => serde_json::json!({
            "event": "markdown_content_result",
            "request_id": request_id,
            "surface_id": surface_id,
            "ok": true,
            "file": file,
            "source": source,
            "truncated": truncated,
        }),
        Err(reason) => serde_json::json!({
            "event": "markdown_content_result",
            "request_id": request_id,
            "surface_id": surface_id,
            "ok": false,
            "reason": reason,
        }),
    };
    let frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&payload).unwrap_or_default(),
    );
    let _ = hub.push(client_id, frame); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

/// client에 다시 읽기 버튼을 표시하게 하는 변경 통지.
#[cfg(feature = "gui")]
const MARKDOWN_CHANGED_EVENT: &str = "markdown_changed";

/// webview.set_url로 문서가 다시 그려졌을 때 모든 workspace holder에 통지를 시도한다.
/// 파일 내용이 바뀌었다는 뜻은 아니며 client는 사용자 요청 후에 원문을 다시 읽는다.
/// notifier·점유가 없으면 생략한다. 반환값은 큐에 실린 client 수다.
/// webview.set_url이 GUI 전용이므로 헤드리스에서는 이 통지를 보내지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn notify_markdown_changed(
    attach: &crate::core::attach::OccupancyRegistry,
    kind: &str,
    plugin_id: &str,
    surface_id: SurfaceId,
) -> usize {
    if !is_attach_content_allowed(kind, plugin_id) {
        return 0;
    }
    let Some(hub) = attach.notifier() else {
        return 0;
    };
    let holders = attach.workspace_holders();
    if holders.is_empty() {
        return 0;
    }
    let payload = serde_json::json!({
        "event": MARKDOWN_CHANGED_EVENT,
        "surface_id": surface_id,
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_default();
    let mut sent = 0;
    for client_id in holders {
        match hub.push(
            client_id,
            StreamFrame::new(StreamTag::Control, bytes.clone()),
        ) {
            PushResult::Sent => sent += 1,
            other => tracing::debug!(
                "attach: markdown_changed surface={surface_id} client={client_id} not queued: {other:?}"
            ),
        }
    }
    sent
}

/// 허용된 content surface의 파일을 읽어 (file, source, truncated)를 반환한다.
/// 파일 없이 열린 markdown은 빈 문서로 답한다. 파일 전체를 읽은 뒤 응답 크기를 제한한다.
fn markdown_content_for_request(
    engine: &CoreState,
    surface_id: u32,
) -> Result<(String, String, bool), String> {
    let surface = engine
        .find_surface_by_id(surface_id)
        .ok_or_else(|| "unknown surface".to_string())?;
    let (kind, plugin_id, file) = surface
        .attach_content_info()
        .ok_or_else(|| "surface does not carry content".to_string())?;
    if !is_attach_content_allowed(kind, plugin_id) {
        return Err(format!("surface kind '{kind}' is not content-mirrored"));
    }
    let Some(path) = file else {
        return Ok((String::new(), String::new(), false));
    };
    let bytes = std::fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "permission denied".to_string()
        } else {
            e.to_string()
        }
    })?;
    let (source, truncated) = markdown_source_wire_capped(&bytes);
    Ok((path.to_string_lossy().to_string(), source, truncated))
}

/// source를 JSON 문자열로 직렬화한 크기 제한. 이스케이프와 따옴표를 포함한다.
/// 전체 프레임 상한보다 작게 두어 나머지 응답 필드의 여유를 남긴다.
/// plugin의 대용량 표시 확인과는 별개이며 파일 읽기·메모리 사용량의 상한이 아니다.
const MARKDOWN_CONTENT_BYTE_BUDGET: usize = 700 * 1024;

fn markdown_source_wire_capped(bytes: &[u8]) -> (String, bool) {
    markdown_source_wire_capped_with_budget(bytes, MARKDOWN_CONTENT_BYTE_BUDGET)
}

/// JSON 문자열 안에서 char가 차지할 바이트 수. BMP 문자는 아래 검사에서 serde_json과 비교한다.
fn json_escaped_char_len(ch: char) -> usize {
    match ch {
        '"' | '\\' | '\n' | '\r' | '\t' | '\u{08}' | '\u{0c}' => 2,
        c if (c as u32) < 0x20 => 6,
        c => c.len_utf8(),
    }
}

/// 따옴표 두 바이트와 이스케이프 비용을 세어 UTF-8 문자 경계에서 자른다.
/// bytes 전체를 먼저 lossy 변환한다. 빈 문자열도 따옴표 두 바이트가 필요하다.
fn markdown_source_wire_capped_with_budget(bytes: &[u8], budget: usize) -> (String, bool) {
    let text = String::from_utf8_lossy(bytes);
    let mut used: usize = 2;
    for (i, ch) in text.char_indices() {
        let cost = json_escaped_char_len(ch);
        if used + cost > budget {
            return (text[..i].to_string(), true);
        }
        used += cost;
    }
    (text.into_owned(), false)
}

#[cfg(test)]
mod markdown_content_tests {
    use super::{json_escaped_char_len, markdown_source_wire_capped_with_budget};

    fn serialized_len(s: &str) -> usize {
        serde_json::to_vec(&serde_json::Value::String(s.to_string()))
            .expect("string always serializes")
            .len()
    }

    #[test]
    fn a_document_within_budget_is_not_truncated() {
        let (source, truncated) = markdown_source_wire_capped_with_budget(b"# hi\n", 64);
        assert_eq!(source, "# hi\n");
        assert!(!truncated);
    }

    #[test]
    fn truncation_backs_up_to_a_char_boundary() {
        let bytes = "가나".as_bytes();
        assert_eq!(bytes.len(), 6);
        let (source, truncated) = markdown_source_wire_capped_with_budget(bytes, 7);
        assert_eq!(source, "가", "잘린 자리에 U+FFFD 가 생기면 안 된다");
        assert!(truncated);
    }

    #[test]
    fn truncation_at_an_exact_boundary_keeps_everything_before_it() {
        let bytes = "가나".as_bytes();
        let (source, truncated) = markdown_source_wire_capped_with_budget(bytes, 5);
        assert_eq!(source, "가");
        assert!(truncated);
        assert_eq!(serialized_len(&source), 5);
    }

    #[test]
    fn budget_counts_escape_expansion_not_raw_bytes() {
        let raw = "\"".repeat(400);
        assert_eq!(serialized_len(&raw), 802);

        let (source, truncated) = markdown_source_wire_capped_with_budget(raw.as_bytes(), 500);
        assert!(
            truncated,
            "원문(400) 은 예산(500) 안이지만 직렬화(802) 는 넘는다 — 잘려야 한다"
        );
        assert_eq!(source.len(), 249, "따옴표 249 개 = 2 + 249*2 = 500");
        assert_eq!(serialized_len(&source), 500);
    }

    #[test]
    fn control_characters_are_counted_at_their_six_byte_cost() {
        let raw = "\u{01}".repeat(100);
        let (source, truncated) = markdown_source_wire_capped_with_budget(raw.as_bytes(), 200);
        assert!(truncated);
        assert_eq!(source.chars().count(), 33, "2 + 33*6 = 200");
        assert_eq!(serialized_len(&source), 200);
    }

    /// 유효한 BMP 문자 전부를 serde_json의 직렬화 길이와 대조한다.
    #[test]
    fn escaped_char_len_matches_serde_json() {
        for cp in 0u32..=0xFFFF {
            let Some(ch) = char::from_u32(cp) else {
                continue; // surrogate
            };
            let s = ch.to_string();
            let expected = serialized_len(&s) - 2;
            assert_eq!(
                json_escaped_char_len(ch),
                expected,
                "U+{cp:04X} 의 이스케이프 길이가 serde_json 과 다르다"
            );
        }
    }

    #[test]
    fn invalid_utf8_is_carried_lossily_without_failing() {
        let (source, truncated) = markdown_source_wire_capped_with_budget(&[0xff, 0xfe], 64);
        assert!(!source.is_empty());
        assert!(!truncated);
    }
}

#[cfg(all(test, feature = "gui"))]
mod markdown_changed_tests {
    use super::notify_markdown_changed;
    use crate::core::attach::OccupancyRegistry;
    use tasty_ipc::stream::StreamTag;
    use tasty_ipc::stream_hub::StreamHub;

    fn changed_surface_id(frame: &tasty_ipc::stream::StreamFrame) -> Option<u64> {
        assert_eq!(frame.tag, StreamTag::Control);
        let v: serde_json::Value = serde_json::from_slice(&frame.payload).ok()?;
        (v.get("event")?.as_str()? == "markdown_changed").then_some(())?;
        v.get("surface_id")?.as_u64()
    }

    #[test]
    fn every_workspace_holder_receives_the_signal_once() {
        let hub = StreamHub::new();
        let a = hub.alloc_id();
        let b = hub.alloc_id();
        let bystander = hub.alloc_id();
        let rx_a = hub.register(a);
        let rx_b = hub.register(b);
        let rx_bystander = hub.register(bystander);
        let mut reg = OccupancyRegistry::new();
        reg.set_notifier(hub);
        reg.acquire_workspace(100, &[10], &[10, 11], a).unwrap();
        reg.acquire_workspace(200, &[20], &[20], b).unwrap();
        reg.acquire_workspace(300, &[30], &[30], a).unwrap();

        assert_eq!(
            notify_markdown_changed(&reg, "markdown", "com.tasty.markdown", 11),
            2
        );
        assert_eq!(changed_surface_id(&rx_a.try_recv().unwrap()), Some(11));
        assert!(
            rx_a.try_recv().is_err(),
            "두 워크스페이스를 점유해도 신호는 한 번"
        );
        assert_eq!(changed_surface_id(&rx_b.try_recv().unwrap()), Some(11));
        assert!(
            rx_bystander.try_recv().is_err(),
            "점유하지 않은 client 에는 가지 않는다"
        );
    }

    #[test]
    fn no_holder_or_non_content_kind_is_a_no_op() {
        let hub = StreamHub::new();
        let a = hub.alloc_id();
        let rx = hub.register(a);
        let mut reg = OccupancyRegistry::new();
        reg.set_notifier(hub);
        assert_eq!(
            notify_markdown_changed(&reg, "markdown", "com.tasty.markdown", 11),
            0
        );
        assert!(rx.try_recv().is_err());

        reg.acquire_workspace(100, &[10], &[10, 11], a).unwrap();
        assert_eq!(
            notify_markdown_changed(&reg, "html", "com.tasty.html", 11),
            0
        );
        assert_eq!(
            notify_markdown_changed(&reg, "markdown", "com.thirdparty.markdown", 11),
            0
        );
        assert!(rx.try_recv().is_err());
    }
}

#[cfg(test)]
mod git_query_tests {
    use super::*;

    #[test]
    fn cap_wire_values_stops_before_exceeding_budget() {
        let items: Vec<serde_json::Value> = (0..20)
            .map(|i| serde_json::json!({ "path": format!("file_{i:03}") }))
            .collect();
        let one_len = serde_json::to_vec(&items[0]).unwrap().len();
        let budget = one_len * 3;
        let (out, truncated) = cap_wire_values(budget, items);
        assert_eq!(out.len(), 3, "expected exactly 3 entries to fit: {out:?}");
        assert!(truncated);
    }

    #[test]
    fn cap_wire_values_empty_input_never_truncated() {
        let (out, truncated) = cap_wire_values(0, Vec::new());
        assert!(out.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn worktree_entry_wire_roundtrips_fields() {
        let w = tasty_git_core::WorktreeEntry {
            name: "main".to_string(),
            path: std::path::PathBuf::from("/repo"),
            branch: Some("main".to_string()),
            oid: Some("abc1234".to_string()),
            is_main: true,
            is_current: true,
            locked: false,
            lock_reason: None,
            is_valid: true,
        };
        let wire = worktree_entry_wire(&w);
        assert_eq!(wire["name"], "main");
        assert_eq!(wire["path"], "/repo");
        assert_eq!(wire["is_current"], true);
    }

    fn test_engine() -> crate::core::CoreState {
        let term_waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::core::CoreState::new(80, 24, term_waker).expect("engine")
    }

    #[test]
    fn resolve_git_query_target_prefers_worktree_path_over_surface_cwd() {
        let engine = test_engine();
        let resolved = resolve_git_query_target(&engine, 999, Some("/explicit/path")).unwrap();
        assert_eq!(resolved, std::path::PathBuf::from("/explicit/path"));
    }

    #[test]
    fn resolve_git_query_target_errors_without_cwd_or_worktree_path() {
        let engine = test_engine();
        let err = resolve_git_query_target(&engine, 999, None).unwrap_err();
        assert!(err.contains("no known cwd"));
    }
}

#[cfg(test)]
mod list_dir_entries_wire_capped_tests {
    use super::list_dir_entries_wire_capped_with_budget;
    use crate::core::fs_list::DirEntryInfo;

    fn entry(name: &str) -> DirEntryInfo {
        DirEntryInfo {
            #[cfg(feature = "gui")]
            path: name.into(),
            name: name.to_string(),
            is_dir: false,
            size: 0,
            modified: None,
            ext: String::new(),
        }
    }

    #[test]
    fn fits_within_budget_untruncated() {
        let entries: Vec<_> = (0..10).map(|i| entry(&format!("f{i}"))).collect();
        let (wire, truncated) = list_dir_entries_wire_capped_with_budget(&entries, 10_000);
        assert_eq!(wire.len(), 10);
        assert!(!truncated);
    }

    #[test]
    fn stops_before_exceeding_budget() {
        // 포맷이 바뀌어도 항목 개수 경계를 검사하도록 현재 직렬화 길이로 예산을 정한다.
        let entries: Vec<_> = (0..20).map(|i| entry(&format!("file_{i:03}"))).collect();
        let one_entry_len = serde_json::to_vec(&super::list_dir_entry_wire(&entries[0]))
            .unwrap()
            .len();
        let budget = one_entry_len * 3;
        let (wire, truncated) = list_dir_entries_wire_capped_with_budget(&entries, budget);
        assert_eq!(wire.len(), 3, "expected exactly 3 entries to fit: {wire:?}");
        assert!(truncated);
    }

    #[test]
    fn empty_input_is_never_truncated() {
        let (wire, truncated) = list_dir_entries_wire_capped_with_budget(&[], 0);
        assert!(wire.is_empty());
        assert!(!truncated);
    }
}

/// attach_error와 Detach를 보낸다. 대상 engine을 찾지 못한 GUI 메인 루프에서도 호출한다.
pub(crate) fn reject_attach(
    hub: &StreamHub,
    client_id: AttachClientId,
    reason: &str,
    holder: Option<AttachClientId>,
) {
    let msg = serde_json::json!({
        "event": "attach_error",
        "reason": reason,
        "holder": holder,
    });
    let error_frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&msg).unwrap_or_default(),
    );
    let _ = hub.push(client_id, error_frame); // 실패한 거절 통지는 재시도하지 않는다.
    let _ = hub.push(client_id, StreamFrame::new(StreamTag::Detach, Vec::new())); // 손실·끊김 처리는 허브에 맡기고 여기서는 재전송하지 않는다.
}

/// model 크레이트는 앱의 허용 목록을 참조할 수 없어 여기서 mesh 후보를 다시 확인한다.
/// 반환값의 rejected는 비터미널 목록과 합쳐 placeholder로 처리해야 한다.
pub(crate) fn mesh_mirror_candidates(
    class: &AttachSurfaceClass,
) -> (Vec<(SurfaceId, &str, &str)>, Vec<SurfaceId>) {
    let mut whitelisted = Vec::new();
    let mut rejected = Vec::new();
    for (sid, kind, plugin_id) in &class.mesh_candidates {
        if crate::core::surface_registry::egui_mesh::is_egui_mesh_allowed(kind, plugin_id) {
            whitelisted.push((*sid, kind.as_str(), plugin_id.as_str()));
        } else {
            rejected.push(*sid);
        }
    }
    (whitelisted, rejected)
}

/// markdown의 파일 원문만 지원한다. 같은 webview 계열인 html의 URL을 파일 경로로 취급하면 안 된다.
pub(crate) fn is_attach_content_allowed(kind: &str, plugin_id: &str) -> bool {
    matches!((kind, plugin_id), ("markdown", "com.tasty.markdown"))
}

/// (허용한 (surface_id, kind, file) 목록, placeholder로 보낼 ID 목록).
type ContentMirrorSplit<'a> = (
    Vec<(SurfaceId, &'a str, Option<&'a std::path::Path>)>,
    Vec<SurfaceId>,
);

/// model이 수집한 content 후보를 앱의 허용 목록으로 확인한다. 거절된 ID는 placeholder로 처리한다.
pub(crate) fn content_mirror_candidates(class: &AttachSurfaceClass) -> ContentMirrorSplit<'_> {
    let mut whitelisted = Vec::new();
    let mut rejected = Vec::new();
    for (sid, kind, plugin_id, file) in &class.content_candidates {
        if is_attach_content_allowed(kind, plugin_id) {
            whitelisted.push((*sid, kind.as_str(), file.as_deref()));
        } else {
            rejected.push(*sid);
        }
    }
    (whitelisted, rejected)
}

#[cfg(test)]
mod content_mirror_candidate_tests {
    use super::content_mirror_candidates;
    use crate::model::AttachSurfaceClass;
    use std::path::PathBuf;

    #[test]
    fn content_mirror_candidates_filters_by_whitelist() {
        let class = AttachSurfaceClass {
            content_candidates: vec![
                (
                    10,
                    "markdown".into(),
                    "com.tasty.markdown".into(),
                    Some(PathBuf::from("/proj/README.md")),
                ),
                (11, "html".into(), "com.tasty.html".into(), None),
                (
                    12,
                    "markdown".into(),
                    "com.thirdparty.markdown".into(),
                    Some(PathBuf::from("/x.md")),
                ),
            ],
            ..Default::default()
        };
        let (whitelisted, rejected) = content_mirror_candidates(&class);
        assert_eq!(
            whitelisted,
            vec![(
                10,
                "markdown",
                Some(std::path::Path::new("/proj/README.md"))
            )]
        );
        assert_eq!(rejected, vec![11, 12]);
    }

    #[test]
    fn a_markdown_surface_without_a_file_is_still_a_candidate() {
        let class = AttachSurfaceClass {
            content_candidates: vec![(7, "markdown".into(), "com.tasty.markdown".into(), None)],
            ..Default::default()
        };
        let (whitelisted, rejected) = content_mirror_candidates(&class);
        assert_eq!(whitelisted, vec![(7, "markdown", None)]);
        assert!(rejected.is_empty());
    }
}

#[cfg(test)]
mod mesh_mirror_candidate_tests {
    use super::mesh_mirror_candidates;
    use crate::model::AttachSurfaceClass;

    #[test]
    fn mesh_mirror_candidates_filters_by_whitelist() {
        let class = AttachSurfaceClass {
            terminals: vec![1],
            non_terminals: vec![2],
            explorers: vec![],
            mesh_candidates: vec![
                (10, "image".to_string(), "com.tasty.image".to_string()),
                (
                    11,
                    "mesh_demo".to_string(),
                    "com.tasty.mesh-demo".to_string(),
                ),
                (12, "widget".to_string(), "com.example.widget".to_string()),
                (13, "markdown".to_string(), "com.tasty.markdown".to_string()),
            ],
            content_candidates: vec![],
        };
        let (whitelisted, rejected) = mesh_mirror_candidates(&class);
        let mut whitelisted_ids: Vec<u32> = whitelisted.iter().map(|(sid, _, _)| *sid).collect();
        whitelisted_ids.sort_unstable();
        let mut rejected_ids = rejected;
        rejected_ids.sort_unstable();
        assert_eq!(whitelisted_ids, vec![10, 11]);
        assert_eq!(rejected_ids, vec![12, 13]);
    }
}

#[cfg(test)]
mod mesh_descriptor_display_name_tests {
    use crate::core::egui_mesh_surface::EguiMeshSurface;

    fn engine_with_mesh_surface(
        kind: &'static str,
        plugin_id: &str,
        display_name: &str,
    ) -> (crate::core::CoreState, usize) {
        let term_waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, term_waker).unwrap();
        let surface: Box<dyn crate::model::Surface> = Box::new(EguiMeshSurface::new(
            100,
            kind,
            plugin_id.to_string(),
            display_name.to_string(),
            Some("/docs/README.md".to_string()),
        ));
        let pane = crate::model::Pane::new_with_surface(1, 1, display_name.to_string(), surface);
        let ws = crate::model::Workspace::new_with_pane(1, "ws".to_string(), pane);
        engine.workspaces.push(ws);
        let idx = engine.workspaces.len() - 1;
        (engine, idx)
    }

    #[test]
    fn image_mesh_descriptor_carries_real_display_name() {
        let (engine, idx) = engine_with_mesh_surface("image", "com.tasty.image", "screenshot.png");
        let class = engine.workspaces[idx].classify_attach_surfaces();
        let (_tree, surfaces) = engine.build_workspace_tree_surfaces(idx, &class);
        let mesh = surfaces
            .iter()
            .find(|s| s.get("role").and_then(|v| v.as_str()) == Some("mesh"))
            .expect("mesh surface descriptor 가 있어야 한다");
        assert_eq!(
            mesh.get("display_name").and_then(|v| v.as_str()),
            Some("screenshot.png"),
            "mesh 디스크립터의 display_name 은 실제 파일명이어야 한다 — kind 문자열(\"image\")로 대체되면 안 됨"
        );
    }

    #[test]
    fn image_and_mesh_demo_mesh_descriptors_also_carry_display_name() {
        for (kind, plugin_id, name) in [
            ("image", "com.tasty.image", "screenshot.png"),
            ("mesh_demo", "com.tasty.mesh-demo", "Demo"),
        ] {
            let (engine, idx) = engine_with_mesh_surface(kind, plugin_id, name);
            let class = engine.workspaces[idx].classify_attach_surfaces();
            let (_tree, surfaces) = engine.build_workspace_tree_surfaces(idx, &class);
            let mesh = surfaces
                .iter()
                .find(|s| s.get("role").and_then(|v| v.as_str()) == Some("mesh"))
                .unwrap_or_else(|| panic!("{kind} mesh surface descriptor 가 있어야 한다"));
            assert_eq!(
                mesh.get("display_name").and_then(|v| v.as_str()),
                Some(name),
                "{kind} mesh 디스크립터도 display_name 을 실어야 한다"
            );
        }
    }
}

#[cfg(test)]
mod forward_exec_tests {
    use super::execute_forwarded_structural_op;
    use crate::state::AppState;
    use tasty_ipc::stream::{ForwardOrigin, SplitAxis, StructuralOp};
    use tasty_terminal::Terminal;

    fn make_core_state() -> (
        crate::core::Core,
        AppState,
        crate::core::CoreState,
        tempfile::TempDir,
    ) {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStorage;
        use tasty_themes::{ThemeStorage, ThemeStore};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::core::builder::CoreBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let term_waker: tasty_terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, term_waker).unwrap();
        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let state = AppState::new(&mut engine, preset_store.clone(), memory.clone());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner))
            .with_home(Arc::new(TmpHome::new(home_tmp.path().to_path_buf())))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core build");
        (core, state, engine, home_tmp)
    }

    fn seed(engine: &mut crate::core::CoreState) -> u32 {
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.terminals.insert(a, Terminal::new_detached(80, 24));
        a
    }

    fn delta_surface_ids(fd: &super::ForwardedDelta) -> std::collections::HashSet<u32> {
        let tasty_ipc::stream::StreamControl::StructuralDelta { surfaces, .. } = &fd.delta else {
            panic!("expected StructuralDelta");
        };
        surfaces
            .iter()
            .filter_map(|s| {
                s.get("remote_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            })
            .collect()
    }

    #[test]
    fn forward_split_surface_executes_and_spawns() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        let fd = r
            .expect("expected Ok")
            .expect("split 성공은 delta 를 동반해야 한다");
        assert_eq!(
            engine.terminals.iter().count(),
            before + 1,
            "forward split 은 서버에서 새 터미널을 spawn 해야 한다"
        );
        assert_eq!(
            fd.added_terminals.len(),
            1,
            "added에는 새 터미널 1개가 있어야 한다"
        );
        let ids = delta_surface_ids(&fd);
        assert!(ids.contains(&a), "delta 에 anchor surface 가 있어야 한다");
        assert!(
            ids.contains(&fd.added_terminals[0]),
            "delta.surfaces 에 신규 surface 가 있어야 한다"
        );
    }

    #[test]
    fn a_forwarded_close_surface_leaves_a_restorable_snapshot() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        // workspace 전체를 지우는 경우와 구분하려고 같은 탭에 형제 surface를 둔다.
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::SplitSurface {
                surface_id: a,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];

        let before = engine.closed_items.len();
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseSurface { surface_id: b },
            ForwardOrigin::User,
        )
        .expect("forwarded close must succeed");
        assert_eq!(
            engine.closed_items.len(),
            before + 1,
            "forward 된 surface close 는 서버 복원 스택에 정확히 하나 남겨야 한다"
        );
    }

    #[test]
    fn a_forwarded_close_tab_leaves_a_restorable_snapshot() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::NewTab {
                anchor_surface_id: a,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("new tab ok")
        .expect("new tab delta");
        let b = fd.added_terminals[0];

        let before = engine.closed_items.len();
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseTab {
                anchor_surface_id: b,
            },
            ForwardOrigin::User,
        )
        .expect("forwarded close must succeed");
        assert_eq!(
            engine.closed_items.len(),
            before + 1,
            "forward 된 tab close 는 서버 복원 스택에 정확히 하나 남겨야 한다"
        );
        assert!(
            matches!(
                engine.closed_items.list().next(),
                Some(crate::model::ClosedItem::Tab(_))
            ),
            "탭 단위로 캡처돼야 한다"
        );
    }

    fn tabs_and_selection(engine: &crate::core::CoreState, surface_id: u32) -> (usize, usize) {
        let pane_id = engine.find_pane_for_surface(surface_id).expect("pane");
        let pane = engine.find_pane_by_id(pane_id).expect("pane");
        (pane.tabs.len(), pane.active_tab)
    }

    fn forward_empty_new_tab(
        core: &mut crate::core::Core,
        state: &mut AppState,
        engine: &mut crate::core::CoreState,
        anchor: u32,
        origin: ForwardOrigin,
    ) {
        execute_forwarded_structural_op(
            core,
            state,
            engine,
            &StructuralOp::NewTab {
                anchor_surface_id: anchor,
                surface_kind: "empty".to_string(),
                params: serde_json::json!({}),
            },
            origin,
        )
        .expect("forwarded new tab must succeed");
    }

    #[test]
    fn a_forwarded_agent_new_tab_keeps_the_selected_tab() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        forward_empty_new_tab(&mut core, &mut state, &mut engine, a, ForwardOrigin::Agent);
        assert_eq!(tabs_and_selection(&engine, a), (2, 0));
    }

    #[test]
    fn a_forwarded_user_new_tab_selects_it() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        forward_empty_new_tab(&mut core, &mut state, &mut engine, a, ForwardOrigin::User);
        assert_eq!(tabs_and_selection(&engine, a), (2, 1));
    }

    #[test]
    fn ipc_tab_create_of_a_non_terminal_keeps_the_selected_tab() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let pane_id = engine.find_pane_for_surface(a).expect("pane");
        let req = ipc_request(
            "tab.create",
            serde_json::json!({ "pane_id": pane_id, "type": "empty" }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        let result = resp.result.expect("tab.create must succeed");
        assert_eq!(result["tab_count"], 2);
        assert_eq!(result["active_tab"], 0);
        assert_eq!(tabs_and_selection(&engine, a), (2, 0));
    }

    #[test]
    fn a_forwarded_close_pane_captures_the_split_context() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::SplitPane {
                anchor_surface_id: a,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("split pane ok")
        .expect("split pane delta");
        let b = fd.added_terminals[0];

        let before = engine.closed_items.len();
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::ClosePane {
                anchor_surface_id: b,
            },
            ForwardOrigin::User,
        )
        .expect("forwarded close must succeed");
        assert_eq!(
            engine.closed_items.len(),
            before + 1,
            "forward 된 pane close 는 서버 복원 스택에 정확히 하나 남겨야 한다"
        );
        assert!(
            matches!(
                engine.closed_items.list().next(),
                Some(crate::model::ClosedItem::Pane { .. })
            ),
            "pane의 split context가 복원 기록에 있어야 한다"
        );
    }

    #[test]
    fn a_forwarded_agent_close_leaves_no_snapshot() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let creates = [
            StructuralOp::SplitSurface {
                surface_id: a,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            StructuralOp::NewTab {
                anchor_surface_id: a,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            StructuralOp::SplitPane {
                anchor_surface_id: a,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
        ];
        let mut added = Vec::new();
        for op in &creates {
            let fd = execute_forwarded_structural_op(
                &mut core,
                &mut state,
                &mut engine,
                op,
                ForwardOrigin::User,
            )
            .expect("create ok")
            .expect("create delta");
            added.push(fd.added_terminals[0]);
        }
        let closes = [
            StructuralOp::CloseSurface {
                surface_id: added[0],
            },
            StructuralOp::CloseTab {
                anchor_surface_id: added[1],
            },
            StructuralOp::ClosePane {
                anchor_surface_id: added[2],
            },
        ];
        let before = engine.closed_items.len();
        for op in &closes {
            execute_forwarded_structural_op(
                &mut core,
                &mut state,
                &mut engine,
                op,
                ForwardOrigin::Agent,
            )
            .unwrap_or_else(|e| panic!("agent close {op:?} must succeed: {e}"));
            assert!(
                engine.find_workspace_index_for_surface(a).is_some(),
                "시험 전제: anchor 가 살아 있어 워크스페이스 통째 close 갈래가 아니어야 한다"
            );
        }
        for sid in &added {
            assert!(
                engine.find_workspace_index_for_surface(*sid).is_none(),
                "시험 전제: 에이전트 close 가 실제로 닫았어야 한다 — surface {sid}"
            );
        }
        assert_eq!(
            engine.closed_items.len(),
            before,
            "원격 에이전트의 close 는 서버 복원 스택에 아무것도 남기지 않아야 한다"
        );
    }

    #[test]
    fn a_plain_ipc_close_still_leaves_no_snapshot() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::SplitSurface {
                surface_id: a,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];

        let before = engine.closed_items.len();
        let resp = crate::adapters::ipc::handler::surface::handle_surface_close(
            &mut core,
            &mut state,
            &mut engine,
            serde_json::Value::Null,
            &serde_json::json!({ "surface_id": b }),
        );
        assert!(resp.error.is_none(), "일반 IPC close 자체는 성공해야 한다");
        assert_eq!(
            engine.closed_items.len(),
            before,
            "에이전트 경로의 close 는 사용자 되돌리기 스택을 건드리지 않는다"
        );
    }

    #[test]
    fn a_forwarded_restore_recreates_the_tab_and_lands_in_the_delta() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::NewTab {
                anchor_surface_id: a,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("new tab ok")
        .expect("new tab delta");
        let b = fd.added_terminals[0];
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseTab {
                anchor_surface_id: b,
            },
            ForwardOrigin::User,
        )
        .expect("close ok");

        let before = engine.workspaces[0].all_surface_ids().len();
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::RestoreClosedItem {
                anchor_surface_id: a,
            },
            ForwardOrigin::User,
        )
        .expect("restore must succeed")
        .expect("delta must be produced");
        assert_eq!(
            engine.workspaces[0].all_surface_ids().len(),
            before + 1,
            "복원은 anchor 워크스페이스 안에 surface 를 되살려야 한다"
        );
        assert_eq!(
            fd.added_terminals.len(),
            1,
            "복원된 터미널이 tap 대상에 포함돼야 한다"
        );
    }

    #[test]
    fn a_forwarded_restore_with_an_empty_stack_reports_the_sentinel_reason() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let err = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::RestoreClosedItem {
                anchor_surface_id: a,
            },
            ForwardOrigin::User,
        )
        .expect_err("빈 스택은 Err(reason) 이어야 한다");
        assert_eq!(err, tasty_ipc::stream::STRUCTURAL_REASON_RESTORE_EMPTY);
    }

    #[test]
    fn a_forwarded_restore_does_not_move_the_local_users_focus() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::SplitPane {
                anchor_surface_id: a,
                direction: SplitAxis::Vertical,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("split pane ok")
        .expect("split pane delta");
        let b = fd.added_terminals[0];
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::ClosePane {
                anchor_surface_id: b,
            },
            ForwardOrigin::User,
        )
        .expect("close pane ok");

        let ws_before = state.active_workspace;
        let focus_before: Vec<(u32, u32)> = engine
            .workspaces
            .iter()
            .map(|w| (w.id, w.focused_pane))
            .collect();
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::RestoreClosedItem {
                anchor_surface_id: a,
            },
            ForwardOrigin::User,
        )
        .expect("restore must succeed");
        assert_eq!(
            state.active_workspace, ws_before,
            "원격의 복원이 로컬 사용자의 활성 워크스페이스를 바꾸면 안 된다"
        );
        let focus_after: Vec<(u32, u32)> = engine
            .workspaces
            .iter()
            .map(|w| (w.id, w.focused_pane))
            .collect();
        assert_eq!(
            focus_before, focus_after,
            "원격의 복원이 로컬 사용자의 focused pane 을 바꾸면 안 된다"
        );
    }

    #[test]
    fn a_forwarded_restore_never_takes_another_workspaces_item() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::NewTab {
                anchor_surface_id: a,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("new tab ok")
        .expect("new tab delta");
        let b = fd.added_terminals[0];
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseTab {
                anchor_surface_id: b,
            },
            ForwardOrigin::User,
        )
        .expect("close ok");

        let other_ws = core
            .create_default_workspace(&mut engine)
            .expect("second workspace");
        let other_sid = engine.workspaces[other_ws].all_surface_ids()[0];
        engine
            .terminals
            .insert(other_sid, Terminal::new_detached(80, 24));
        let other_pane = engine.workspaces[other_ws]
            .pane_layout()
            .first_pane()
            .unwrap()
            .id;
        let other_tab_idx = 0;
        let snapshot = engine
            .capture_closed_tab(other_pane, other_tab_idx)
            .expect("other workspace tab snapshot");
        engine.push_closed_item(snapshot);

        let restored_ws0 = engine.workspaces[0].all_surface_ids().len();
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::RestoreClosedItem {
                anchor_surface_id: a,
            },
            ForwardOrigin::User,
        )
        .expect("restore must succeed");
        assert_eq!(
            engine.workspaces[0].all_surface_ids().len(),
            restored_ws0 + 1,
            "anchor 워크스페이스의 항목이 복원돼야 한다"
        );
        assert_eq!(
            engine.closed_items.len(),
            1,
            "다른 워크스페이스 항목은 스택에 그대로 남아야 한다"
        );
    }

    #[test]
    fn forward_split_inherits_workspace_occupancy() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 42;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");
        let op = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("expected Ok")
        .expect("split delta");
        let new_sid = fd.added_terminals[0];
        assert!(
            engine.attach.is_hard_occupied(new_sid),
            "새 surface는 hard 점유 목록에 등록돼야 한다"
        );
        assert_eq!(
            engine.attach.workspace_of_surface(new_sid),
            Some(ws_id),
            "새 surface는 점유 workspace의 멤버로 등록돼야 한다"
        );
        assert_eq!(
            engine.attach.workspace_holder_of(new_sid),
            Some(client_id),
            "새 surface 의 holder 는 workspace holder 와 동일해야 한다"
        );
    }

    /// notifier를 주입해야 tap 경로가 실행된다. 실행 중 자동 tap과 호출자의 후속 tap이 겹치지 않는지 본다.
    #[test]
    fn forward_split_surface_taps_exactly_once_with_real_stream_hub() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 7;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");
        let hub = tasty_ipc::stream_hub::StreamHub::new();
        let _rx = hub.register(client_id);
        engine.attach.set_notifier(hub.clone());

        let op = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("expected Ok")
        .expect("split delta");
        let new_sid = fd.added_terminals[0];

        assert_eq!(
            engine.terminals.get(new_sid).unwrap().output_tap_count(),
            0,
            "구조 변경 실행 중에는 tap하지 않고 호출자가 delta를 보낸 뒤 tap해야 한다"
        );

        engine.tap_surface_for_stream(new_sid, client_id, &hub);
        assert_eq!(
            engine.terminals.get(new_sid).unwrap().output_tap_count(),
            1,
            "호출자가 tap한 뒤에는 하나만 등록돼야 한다"
        );
    }

    fn attached_pair(
        core: &mut crate::core::Core,
        state: &mut AppState,
        engine: &mut crate::core::CoreState,
    ) -> (u32, u32, u32, tasty_ipc::stream_hub::SinkReceiver) {
        let a = seed(engine);
        let b = execute_forwarded_structural_op(
            core,
            state,
            engine,
            &StructuralOp::SplitSurface {
                surface_id: a,
                direction: SplitAxis::Horizontal,
                surface_kind: "terminal".to_string(),
                params: serde_json::json!({}),
            },
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta")
        .added_terminals[0];
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a, b], &[a, b], 7)
            .expect("workspace 점유 획득");
        let hub = tasty_ipc::stream_hub::StreamHub::new();
        let rx = hub.register(7);
        engine.attach.set_notifier(hub);
        (a, b, ws_id, rx)
    }

    fn drain_control(rx: &tasty_ipc::stream_hub::SinkReceiver) -> Vec<serde_json::Value> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .filter(|f| f.tag == tasty_ipc::stream::StreamTag::Control)
            .map(|f| serde_json::from_slice(&f.payload).expect("control json"))
            .collect()
    }

    fn surface_ids_of(delta: &serde_json::Value) -> Vec<u64> {
        delta["surfaces"]
            .as_array()
            .expect("surfaces")
            .iter()
            .filter_map(|s| s["remote_id"].as_u64())
            .collect()
    }

    #[test]
    fn a_pty_exit_in_an_attached_workspace_reaches_the_holder_as_a_delta() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (a, b, ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, b);
        let msgs = drain_control(&rx);
        assert_eq!(msgs.len(), 1, "delta 는 정확히 한 번: {msgs:?}");
        assert_eq!(msgs[0]["event"], "structural_delta");
        assert_eq!(msgs[0]["workspace_id"], ws_id);
        assert_eq!(
            surface_ids_of(&msgs[0]),
            vec![u64::from(a)],
            "닫힌 b 가 빠진 트리"
        );
        assert_eq!(
            engine.attach.workspace_holder(ws_id),
            Some(7),
            "점유는 그대로"
        );
    }

    #[test]
    fn a_forwarded_close_leaves_no_structure_change_behind() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (_a, b, _ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseSurface { surface_id: b },
            ForwardOrigin::User,
        )
        .expect("close ok")
        .expect("close delta");
        engine.push_structure_changes();
        assert!(
            drain_control(&rx).is_empty(),
            "forward close 의 트리는 호출측이 보내는 delta 하나뿐이어야 한다"
        );
    }

    #[test]
    fn the_last_member_exiting_force_detaches_the_holder() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (a, b, ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, b);
        drain_control(&rx);
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, a);
        assert!(
            engine.find_workspace_index_for_id(ws_id).is_none(),
            "시험 전제: 워크스페이스가 purge 됐다"
        );
        let msgs = drain_control(&rx);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert_eq!(msgs[0]["event"], "force_detached");
        assert_eq!(
            engine.attach.workspace_holder(ws_id),
            None,
            "stale lock 없음"
        );
    }

    #[test]
    fn a_local_member_reaches_the_holder_as_a_delta_before_its_snapshot() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (a, _b, _ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        let pane_id = engine.find_pane_for_surface(a).expect("pane");
        crate::core::structural_exec::create_tab(
            &mut core,
            &mut state,
            &mut engine,
            pane_id,
            &serde_json::json!({ "pane_id": pane_id }),
            false,
        )
        .expect("local create_tab");
        let first = rx.try_recv().expect("holder 에게 무언가 나가야 한다");
        assert_eq!(
            first.tag,
            tasty_ipc::stream::StreamTag::Control,
            "첫 프레임은 delta"
        );
        let delta: serde_json::Value = serde_json::from_slice(&first.payload).unwrap();
        assert_eq!(delta["event"], "structural_delta");
        assert_eq!(surface_ids_of(&delta).len(), 3, "a · b · 새 탭");
    }

    fn push_unrelated_workspace(engine: &mut crate::core::CoreState, ws_id: u32, surface_id: u32) {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::core::egui_mesh_surface::EguiMeshSurface::new(
                surface_id,
                "image",
                "com.tasty.image".to_string(),
                "elsewhere".to_string(),
                None,
            ));
        let pane =
            crate::model::Pane::new_with_surface(ws_id, ws_id, "elsewhere".to_string(), surface);
        engine
            .workspaces
            .push(crate::model::Workspace::new_with_pane(
                ws_id,
                "elsewhere".to_string(),
                pane,
            ));
    }

    #[test]
    fn an_anchor_alive_in_another_workspace_is_not_called_gone() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (_a, _b, _ws_id, _rx) = attached_pair(&mut core, &mut state, &mut engine);
        push_unrelated_workspace(&mut engine, 900, 901);
        let alive = StructuralOp::CloseSurface { surface_id: 901 };
        let gone = StructuralOp::CloseSurface { surface_id: 555 };
        let reason =
            |op| crate::core::attach_structure_sync::unresolved_forward_reason([&engine], 7, op);
        assert_eq!(reason(&alive), "workspace not found");
        assert!(
            reason(&gone).starts_with("no live surface 555 "),
            "{}",
            reason(&gone)
        );
    }

    #[test]
    fn an_anchor_alive_in_another_engine_is_not_called_gone() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let (_a, _b, _ws_id, _rx) = attached_pair(&mut core, &mut state, &mut engine);
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut other = crate::core::CoreState::new(80, 24, waker).expect("engine");
        push_unrelated_workspace(&mut other, 900, 901);
        let op = StructuralOp::CloseSurface { surface_id: 901 };
        let reason = |engines: Vec<&crate::core::CoreState>| {
            crate::core::attach_structure_sync::unresolved_forward_reason(engines, 7, &op)
        };
        assert_eq!(reason(vec![&engine, &other]), "workspace not found");
        assert_eq!(reason(vec![&other, &engine]), "workspace not found");
        assert!(reason(vec![&engine]).starts_with("no live surface 901 "));
    }

    #[test]
    fn forward_new_tab_executes() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        let fd = r.expect("expected Ok").expect("new-tab 성공은 delta 동반");
        assert_eq!(engine.terminals.iter().count(), before + 1);
        assert_eq!(fd.added_terminals.len(), 1);
    }

    #[test]
    fn forward_close_tab_removes_from_delta() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let mk = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let added = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &mk,
            ForwardOrigin::User,
        )
        .expect("new-tab Ok")
        .expect("new-tab delta");
        let new_sid = added.added_terminals[0];
        let close = StructuralOp::CloseTab {
            anchor_surface_id: new_sid,
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &close,
            ForwardOrigin::User,
        )
        .expect("close Ok")
        .expect("close delta");
        assert!(fd.added_terminals.is_empty(), "close 는 added 없음");
        let ids = delta_surface_ids(&fd);
        assert!(
            !ids.contains(&new_sid),
            "닫힌 surface 는 delta 에서 빠져야 한다"
        );
        assert!(ids.contains(&a), "남은 surface 는 delta 에 유지");
    }

    #[test]
    fn forward_unknown_kind_fails() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "definitely-not-registered".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        assert!(r.is_err(), "unknown kind must fail");
        assert!(
            r.unwrap_err().contains("unknown surface kind"),
            "reason 이 미등록 kind 를 가리켜야 한다"
        );
        assert_eq!(
            engine.terminals.iter().count(),
            before,
            "실패한 forward 는 새 터미널을 만들지 않는다"
        );
    }

    #[test]
    fn forward_convert_unknown_kind_reports_the_remote_reason() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "definitely-not-registered".to_string(),
            params: serde_json::json!({}),
            cwd: None,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        let Err(reason) = r else {
            panic!("unknown kind 로의 convert 는 실패해야 한다");
        };
        assert_eq!(reason, "unknown surface kind: definitely-not-registered");
        assert!(
            engine.terminals.get(a).is_some(),
            "실패한 convert 는 원래 터미널을 그대로 둔다"
        );
    }

    /// 같은 실패 입력을 forward와 IPC에 넣는다. 기대 문구는 실행 결과와 별도로 고정해 두었다.
    struct FailureCase(
        &'static str,
        &'static str,
        Box<dyn Fn(u32) -> StructuralOp>,
        fn(
            &mut crate::core::Core,
            &mut dyn crate::adapters::ipc::window_port::IpcWindow,
            &mut crate::core::CoreState,
            serde_json::Value,
            &serde_json::Value,
        ) -> tasty_ipc::protocol::JsonRpcResponse,
        Box<dyn Fn(u32, u32) -> serde_json::Value>,
    );

    fn failure_cases() -> Vec<FailureCase> {
        use crate::adapters::ipc::handler::{pane, tab};
        use serde_json::json;

        let missing_dir = "/definitely/not/a/tasty/test/dir";
        vec![
            FailureCase(
                "new tab of an unknown kind",
                "unknown surface kind: definitely-not-registered",
                Box::new(|a| StructuralOp::NewTab {
                    anchor_surface_id: a,
                    surface_kind: "definitely-not-registered".to_string(),
                    params: json!({}),
                }),
                tab::handle_tab_create,
                Box::new(|_, pane| json!({ "pane_id": pane, "type": "definitely-not-registered" })),
            ),
            FailureCase(
                "new tab with a cwd the server does not have",
                "cwd does not exist: /definitely/not/a/tasty/test/dir",
                Box::new(move |a| StructuralOp::NewTab {
                    anchor_surface_id: a,
                    surface_kind: "terminal".to_string(),
                    params: json!({ "cwd": missing_dir }),
                }),
                tab::handle_tab_create,
                Box::new(
                    move |_, pane| json!({ "pane_id": pane, "type": "terminal", "cwd": missing_dir }),
                ),
            ),
            FailureCase(
                "surface split whose bag also names a pane",
                "Cannot specify both 'target_surface' and 'target_pane'. Use one.",
                Box::new(|a| StructuralOp::SplitSurface {
                    surface_id: a,
                    direction: SplitAxis::Vertical,
                    surface_kind: "terminal".to_string(),
                    params: json!({ "target_pane": 1 }),
                }),
                pane::handle_split,
                Box::new(
                    |a, _| json!({ "level": "surface", "target_surface": a, "target_pane": 1, "type": "terminal" }),
                ),
            ),
            FailureCase(
                "pane split whose bag carries a malformed pane id",
                "'target_pane' was given as \"not-a-number\" — it must be a whole number that fits in 32 \
                 bits and is not negative. Refusing rather than coercing it: a truncated id names a \
                 different, possibly real, target, and a dropped value is indistinguishable from the \
                 parameter being absent",
                Box::new(|a| StructuralOp::SplitPane {
                    anchor_surface_id: a,
                    direction: SplitAxis::Horizontal,
                    surface_kind: "terminal".to_string(),
                    params: json!({ "target_pane": "not-a-number" }),
                }),
                pane::handle_split,
                Box::new(
                    |a, _| json!({ "level": "pane", "target_surface": a, "target_pane": "not-a-number", "type": "terminal" }),
                ),
            ),
            FailureCase(
                "surface split with a cwd the server does not have",
                "cwd does not exist: /definitely/not/a/tasty/test/dir",
                Box::new(move |a| StructuralOp::SplitSurface {
                    surface_id: a,
                    direction: SplitAxis::Horizontal,
                    surface_kind: "terminal".to_string(),
                    params: json!({ "cwd": missing_dir }),
                }),
                pane::handle_split,
                Box::new(
                    move |a, _| json!({ "level": "surface", "target_surface": a, "type": "terminal", "cwd": missing_dir }),
                ),
            ),
        ]
    }

    fn fail_both_ways(case: &FailureCase) -> (String, String) {
        let FailureCase(name, _, op, ipc, ipc_params) = case;
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let pane_id = engine.find_pane_for_surface(a).expect("seed pane");

        let resp = ipc(
            &mut core,
            &mut state,
            &mut engine,
            serde_json::json!(1),
            &ipc_params(a, pane_id),
        );
        let ipc_msg = resp
            .error
            .unwrap_or_else(|| panic!("{name}: IPC 가 성공했다 — 실패 입력이 아니다"))
            .message;

        let forwarded = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op(a),
            ForwardOrigin::User,
        );
        let Err(forward_msg) = forwarded else {
            panic!("{name}: forward 가 성공했다 — IPC 는 `{ipc_msg}` 로 실패했다");
        };
        (ipc_msg, forward_msg)
    }

    /// 두 진입점의 문구만 비교한다. 둘이 함께 바뀌는 경우는 별도의 고정 기대값 검사에서 잡는다.
    #[test]
    fn forward_and_ipc_fail_with_the_same_reason_for_the_same_input() {
        for case in failure_cases() {
            let (ipc_msg, forward_msg) = fail_both_ways(&case);
            assert_eq!(
                forward_msg, ipc_msg,
                "{}: 두 진입점의 실패 문구가 다르다",
                case.0
            );
        }
    }

    /// 고정한 실패 문구와 비교한다. 의도적으로 오류 설명을 바꾸면 기대값도 함께 검토해야 한다.
    #[test]
    fn failure_reasons_keep_the_base_literals() {
        for case in failure_cases() {
            let (ipc_msg, forward_msg) = fail_both_ways(&case);
            assert_eq!(ipc_msg, case.1, "{}: IPC 실패 문구가 기준과 다르다", case.0);
            assert_eq!(
                forward_msg, case.1,
                "{}: forward 회신 사유가 기준과 다르다",
                case.0
            );
        }
    }

    #[test]
    fn forward_missing_anchor_fails() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        seed(&mut engine);
        let op = StructuralOp::ClosePane {
            anchor_surface_id: 999_999,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        assert!(r.is_err(), "missing anchor must fail");
    }

    use crate::adapters::ipc::handler::handle_with_caller;
    use tasty_ipc::caller::CallerContext;
    use tasty_ipc::protocol::JsonRpcRequest;

    fn ipc_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: Some(serde_json::json!(1)),
            session_token: None,
        }
    }

    #[test]
    fn forward_new_tab_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");
        let before = engine.terminals.iter().count();
        let op = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward NewTab 은 hard-occupied 상태에서도 성공해야 한다"
        );
        assert_eq!(engine.terminals.iter().count(), before + 1);
    }

    #[test]
    fn forward_split_pane_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");
        let panes_before = engine.workspaces[0].pane_layout().all_pane_ids().len();
        let op = StructuralOp::SplitPane {
            anchor_surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward SplitPane 은 hard-occupied 상태에서도 성공해야 한다"
        );
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids().len(),
            panes_before + 1
        );
    }

    #[test]
    fn forward_close_pane_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let split = StructuralOp::SplitPane {
            anchor_surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split,
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];
        let all: Vec<u32> = engine.workspaces[0].all_surface_ids();
        engine
            .attach
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let panes_before = engine.workspaces[0].pane_layout().all_pane_ids().len();
        let close = StructuralOp::ClosePane {
            anchor_surface_id: b,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &close,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward ClosePane 은 hard-occupied 상태에서도 성공해야 한다"
        );
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids().len(),
            panes_before - 1
        );
    }

    #[test]
    fn forward_move_tab_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let new_tab = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &new_tab,
            ForwardOrigin::User,
        )
        .expect("new-tab ok")
        .expect("new-tab delta");
        let all: Vec<u32> = engine.workspaces[0].all_surface_ids();
        engine
            .attach
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let mv = StructuralOp::MoveTab {
            anchor_surface_id: a,
            from_index: 0,
            to_index: 1,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &mv,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward MoveTab 은 hard-occupied 상태에서도 성공해야 한다"
        );
    }

    #[test]
    fn forward_close_surface_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let split_surface = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split_surface,
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];
        let all: Vec<u32> = engine.workspaces[0].all_surface_ids();
        engine
            .attach
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let before = engine.terminals.iter().count();
        let close = StructuralOp::CloseSurface { surface_id: b };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &close,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward CloseSurface 는 hard-occupied 상태에서도 성공해야 한다"
        );
        assert_eq!(engine.terminals.iter().count(), before - 1);
    }

    #[test]
    fn forward_close_last_surface_force_detaches_holder() {
        use std::time::Duration;
        use tasty_ipc::stream::StreamTag;
        use tasty_ipc::stream_hub::StreamHub;

        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;

        let hub = StreamHub::new();
        let holder = hub.alloc_id();
        let rx = hub.register(holder);
        engine.attach.set_notifier(hub);
        let all: Vec<u32> = engine.workspaces[0].all_surface_ids();
        engine
            .attach
            .acquire_workspace(ws_id, &all, &all, holder)
            .expect("workspace 점유 획득");

        let close = StructuralOp::CloseSurface { surface_id: a };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &close,
            ForwardOrigin::User,
        );

        assert!(
            r.is_ok(),
            "마지막 surface close forward 는 에러가 아니어야 한다"
        );
        assert!(
            engine.find_workspace_index_for_id(ws_id).is_none(),
            "workspace 는 purge 되어야 한다"
        );

        // 통지가 누락돼도 검사가 무한히 기다리지 않도록 timeout을 둔다.
        let f1 = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("force_detached Control 프레임이 와야 한다");
        assert_eq!(f1.tag, StreamTag::Control);
        assert!(String::from_utf8_lossy(&f1.payload).contains("force_detached"));
        let f2 = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("Detach 프레임이 와야 한다");
        assert_eq!(f2.tag, StreamTag::Detach);

        assert_eq!(engine.attach.workspace_holder(ws_id), None);
    }

    #[test]
    fn forward_convert_surface_executes_and_converts() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            cwd: None,
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("convert ok")
        .expect("convert delta");
        assert_eq!(
            fd.converted_surface,
            Some(a),
            "converted_surface 는 실제로 교체된 surface_id 를 실어야 한다"
        );
        assert!(
            engine.terminals.get(a).is_some(),
            "terminal 로의 convert 는 새 Terminal 을 insert 해야 한다"
        );
    }

    #[test]
    fn forward_convert_surface_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
            cwd: None,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward ConvertSurface 는 hard-occupied 상태에서도 성공해야 한다"
        );
    }

    fn explorer_root(engine: &crate::core::CoreState, surface_id: u32) -> std::path::PathBuf {
        engine
            .find_surface_by_id(surface_id)
            .expect("converted surface")
            .as_any()
            .downcast_ref::<tasty_model::ExplorerPanel>()
            .expect("explorer 로 변환됐어야 한다")
            .current_root()
            .to_path_buf()
    }

    #[test]
    fn forward_convert_surface_applies_wire_cwd() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let dir = tempfile::tempdir().expect("test tempdir");
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "explorer".to_string(),
            params: serde_json::json!({}),
            cwd: Some(dir.path().to_string_lossy().into_owned()),
        };
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("convert ok")
        .expect("convert delta");
        assert_eq!(explorer_root(&engine, a), dir.path());
    }

    #[test]
    fn forward_convert_surface_resolves_cwd_from_target_surface() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let dir = tempfile::tempdir().expect("test tempdir");
        engine
            .terminals
            .get_mut(a)
            .expect("seeded terminal")
            .set_cached_cwd(dir.path().to_path_buf());
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "explorer".to_string(),
            params: serde_json::json!({}),
            cwd: None,
        };
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("convert ok")
        .expect("convert delta");
        assert_eq!(explorer_root(&engine, a), dir.path());
    }

    #[test]
    fn forward_convert_surface_respects_inherit_cwd_gate() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let dir = tempfile::tempdir().expect("test tempdir");
        engine
            .terminals
            .get_mut(a)
            .expect("seeded terminal")
            .set_cached_cwd(dir.path().to_path_buf());
        engine.settings.general.inherit_cwd = false;
        let op = StructuralOp::ConvertSurface {
            surface_id: a,
            surface_kind: "explorer".to_string(),
            params: serde_json::json!({}),
            cwd: None,
        };
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &op,
            ForwardOrigin::User,
        )
        .expect("convert ok")
        .expect("convert delta");
        assert_ne!(explorer_root(&engine, a), dir.path());
    }

    #[test]
    fn forward_move_surface_executes_and_cleans_up_target() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let split_surface = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split_surface,
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];
        let before = engine.terminals.iter().count();

        let mv = StructuralOp::MoveSurface {
            source_surface_id: a,
            target_surface_id: b,
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &mv,
            ForwardOrigin::User,
        )
        .expect("move ok")
        .expect("move delta");
        assert!(
            fd.converted_surface.is_none(),
            "이동은 converted_surface를 반환하면 안 된다"
        );
        assert_eq!(
            engine.terminals.iter().count(),
            before - 1,
            "이동 대상으로 덮어쓴 B의 터미널은 제거돼야 한다"
        );
        assert!(
            engine.terminals.get(a).is_some(),
            "이동한 A의 터미널은 유지돼야 한다"
        );
    }

    #[test]
    fn forward_move_surface_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let split_surface = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split_surface,
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];
        let all: Vec<u32> = engine.workspaces[0].all_surface_ids();
        engine
            .attach
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let mv = StructuralOp::MoveSurface {
            source_surface_id: a,
            target_surface_id: b,
        };
        let r = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &mv,
            ForwardOrigin::User,
        );
        assert!(
            r.is_ok(),
            "holder 의 forward MoveSurface 는 hard-occupied 상태에서도 성공해야 한다"
        );
    }

    #[test]
    fn dispatch_denies_structural_create_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.terminals.iter().count();
        let panes_before = engine.workspaces[0].pane_layout().all_pane_ids().len();

        for (method, params) in [
            ("tab.create", serde_json::json!({ "pane_id": pane_id })),
            (
                "split",
                serde_json::json!({
                    "level": "surface",
                    "target_surface": a,
                    "direction": "horizontal",
                }),
            ),
        ] {
            let req = ipc_request(method, params);
            let resp = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &req,
                &CallerContext::Local,
            );
            let err = resp.error.unwrap_or_else(|| {
                panic!("{method}: hard-occupied 워크스페이스에 대한 비-holder 요청은 거부돼야 한다")
            });
            assert!(
                err.message.to_lowercase().contains("occupied"),
                "{method}: 에러 메시지에 점유 안내가 있어야 한다 (got: {})",
                err.message
            );
        }

        assert_eq!(
            engine.terminals.iter().count(),
            terminals_before,
            "거부된 요청은 새 터미널을 만들면 안 된다"
        );
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids().len(),
            panes_before,
            "거부된 요청은 새 pane 을 만들면 안 된다"
        );
    }

    #[test]
    fn dispatch_denies_structural_close_move_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        let tab_id = engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .expect("pane exists")
            .tabs[0]
            .id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.terminals.iter().count();
        let panes_before = engine.workspaces[0].pane_layout().all_pane_ids().len();
        let tabs_before = engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .expect("pane exists")
            .tabs
            .len();

        for (method, params) in [
            ("pane.close", serde_json::json!({ "pane_id": pane_id })),
            ("tab.close", serde_json::json!({ "tab_id": tab_id })),
            (
                "tab.move",
                serde_json::json!({ "pane_id": pane_id, "from_index": 0, "to_index": 0 }),
            ),
            ("surface.close", serde_json::json!({ "surface_id": a })),
        ] {
            let req = ipc_request(method, params);
            let resp = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &req,
                &CallerContext::Local,
            );
            let err = resp.error.unwrap_or_else(|| {
                panic!("{method}: hard-occupied 워크스페이스에 대한 비-holder 요청은 거부돼야 한다")
            });
            assert!(
                err.message.to_lowercase().contains("occupied"),
                "{method}: 에러 메시지에 점유 안내가 있어야 한다 (got: {})",
                err.message
            );
        }

        assert_eq!(engine.terminals.iter().count(), terminals_before);
        assert_eq!(
            engine.workspaces[0].pane_layout().all_pane_ids().len(),
            panes_before,
            "거부된 요청은 pane 을 닫으면 안 된다"
        );
        assert_eq!(
            engine.workspaces[0]
                .pane_layout()
                .find_pane(pane_id)
                .expect("pane exists")
                .tabs
                .len(),
            tabs_before,
            "거부된 요청은 tab 을 닫거나 이동하면 안 된다"
        );
    }

    #[test]
    fn dispatch_denies_terminal_spawn_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.terminals.iter().count();
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        let tabs_before = engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .expect("pane exists")
            .tabs
            .len();

        let req = ipc_request(
            "terminal.spawn",
            serde_json::json!({ "parent": a, "workspace": ws_id.to_string() }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        let err = resp.error.unwrap_or_else(|| {
            panic!("hard-occupied 워크스페이스로의 terminal.spawn 은 거부돼야 한다")
        });
        assert!(
            err.message.to_lowercase().contains("occupied"),
            "에러 메시지에 점유 안내가 있어야 한다 (got: {})",
            err.message
        );

        assert_eq!(
            engine.terminals.iter().count(),
            terminals_before,
            "거부된 spawn 은 새 터미널을 만들면 안 된다"
        );
        assert_eq!(
            engine.workspaces[0]
                .pane_layout()
                .find_pane(pane_id)
                .expect("pane exists")
                .tabs
                .len(),
            tabs_before,
            "거부된 spawn 은 새 tab 을 만들면 안 된다"
        );
    }

    #[test]
    fn dispatch_allows_terminal_spawn_when_not_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let terminals_before = engine.terminals.iter().count();

        let req = ipc_request(
            "terminal.spawn",
            serde_json::json!({ "parent": a, "workspace": ws_id.to_string() }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        assert!(
            resp.error.is_none(),
            "점유되지 않은 workspace 의 terminal.spawn 은 허용돼야 한다: {:?}",
            resp.error
        );
        assert_eq!(
            engine.terminals.iter().count(),
            terminals_before + 1,
            "정상 spawn 은 새 터미널을 만들어야 한다"
        );
    }

    // terminal.spawn은 생성된 surface를 동기 응답으로 요구한다. forward하면 원격에 탭만 남을 수 있다.

    #[test]
    fn dispatch_denies_terminal_spawn_into_mirror_workspace() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine.workspaces[0].mirror = true;

        let terminals_before = engine.terminals.iter().count();

        let req = ipc_request(
            "terminal.spawn",
            serde_json::json!({ "parent": a, "workspace": ws_id.to_string() }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );

        let err = resp
            .error
            .expect("mirror 워크스페이스 spawn 은 거부돼야 한다");
        assert!(
            err.message.to_lowercase().contains("mirror"),
            "에러 메시지에 mirror 사유가 있어야 한다 (got: {})",
            err.message
        );
        assert!(
            !err.message.contains("surface_id"),
            "내부 필드 이름이 노출되면 안 된다 (got: {})",
            err.message
        );
        assert!(
            engine.pending_structural_forward.is_empty(),
            "거부된 spawn 은 원격 NewTab 을 forward 하면 안 된다"
        );
        assert_eq!(engine.terminals.iter().count(), terminals_before);
    }

    /// GUI는 mirror의 tab.create를 forward한다. 큐를 비우는 메인 루프가 GUI에만 있다.
    #[cfg(feature = "gui")]
    #[test]
    fn dispatch_still_forwards_tab_create_in_mirror_workspace() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        seed(&mut engine);
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        engine.workspaces[0].mirror = true;
        let terminals_before = engine.terminals.iter().count();

        let req = ipc_request("tab.create", serde_json::json!({ "pane_id": pane_id }));
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );

        assert!(
            resp.error.is_none(),
            "mirror 구조 변경은 forward success 로 회신돼야 한다: {:?}",
            resp.error
        );
        assert!(
            matches!(
                engine.pending_structural_forward.first().map(|p| &p.op),
                Some(StructuralOp::NewTab { .. })
            ),
            "mirror 의 tab.create 는 원격 NewTab 으로 큐잉돼야 한다 (got: {:?})",
            engine.pending_structural_forward.first().map(|p| &p.op)
        );
        assert_eq!(
            engine.terminals.iter().count(),
            terminals_before,
            "forward 된 구조 변경은 로컬 트리를 바꾸지 않는다"
        );
    }

    /// 헤드리스에는 forward 큐를 보내는 경로가 없어 mirror 요청을 성공으로 답하면 안 된다.
    #[cfg(not(feature = "gui"))]
    #[test]
    fn dispatch_refuses_tab_create_in_mirror_workspace_in_headless() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        seed(&mut engine);
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        engine.workspaces[0].mirror = true;
        let terminals_before = engine.terminals.iter().count();

        let req = ipc_request("tab.create", serde_json::json!({ "pane_id": pane_id }));
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );

        assert!(
            resp.result.is_none(),
            "headless 는 보낼 수 없는 forward 를 성공으로 답하면 안 된다: {:?}",
            resp.result
        );
        let err = resp
            .error
            .expect("headless 의 mirror 구조 변경은 거절돼야 한다");
        assert!(
            err.message.contains("mirror") && err.message.contains("headless"),
            "사유가 mirror 이면서 빌드 조합임을 말해야 한다 (got: {})",
            err.message
        );
        assert!(
            engine.pending_structural_forward.is_empty(),
            "헤드리스에서 mirror forward 큐에 요청을 넣으면 안 된다"
        );
        assert_eq!(engine.terminals.iter().count(), terminals_before);
    }

    /// workspace와 다른 pane을 지정해도 최종 pane의 점유·mirror 여부로 차단해야 한다.
    #[test]
    fn dispatch_denies_terminal_spawn_when_pane_override_targets_blocked_workspace() {
        for blocked in ["mirror", "hard-occupied"] {
            let (mut core, mut state, mut engine, _home) = make_core_state();
            let a = seed(&mut engine);
            let blocked_ws_id = engine.workspaces[0].id;
            let blocked_pane = engine.workspaces[0].pane_layout().all_pane_ids()[0];

            let create = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &ipc_request("workspace.create", serde_json::json!({ "name": "clean" })),
                &CallerContext::Local,
            );
            assert!(create.error.is_none(), "테스트 준비: {:?}", create.error);
            let clean_ws_id = engine
                .workspaces
                .iter()
                .find(|w| w.id != blocked_ws_id)
                .expect("두 번째 워크스페이스")
                .id;

            match blocked {
                "mirror" => engine.workspaces[0].mirror = true,
                _ => {
                    engine
                        .attach
                        .acquire_workspace(blocked_ws_id, &[a], &[a], 7)
                        .expect("workspace 점유 획득");
                }
            }

            let terminals_before = engine.terminals.iter().count();
            let req = ipc_request(
                "terminal.spawn",
                serde_json::json!({
                    "parent": a,
                    "workspace": clean_ws_id.to_string(),
                    "pane": blocked_pane,
                }),
            );
            let resp = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &req,
                &CallerContext::Local,
            );

            let err = resp.error.unwrap_or_else(|| {
                panic!("{blocked}: pane 오버라이드로 차단 대상 워크스페이스에 spawn 되면 안 된다")
            });
            let msg = err.message.to_lowercase();
            assert!(
                msg.contains("mirror") || msg.contains("occupied"),
                "{blocked}: 에러 메시지에 사유가 있어야 한다 (got: {})",
                err.message
            );
            assert_eq!(
                engine.terminals.iter().count(),
                terminals_before,
                "{blocked}: 거부된 spawn 은 새 터미널을 만들면 안 된다"
            );
            assert!(
                engine.pending_structural_forward.is_empty(),
                "{blocked}: 거부된 spawn 은 원격으로 아무것도 보내지 않는다"
            );
        }
    }

    #[test]
    fn dispatch_allows_tab_create_when_not_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        seed(&mut engine);
        let pane_id = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        let req = ipc_request("tab.create", serde_json::json!({ "pane_id": pane_id }));
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        assert!(
            resp.error.is_none(),
            "점유되지 않은 workspace 의 tab.create 는 허용돼야 한다: {:?}",
            resp.error
        );
    }

    #[test]
    fn dispatch_denies_convert_entrypoints_when_hard_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        for (method, params) in [
            (
                "markdown.navigate",
                serde_json::json!({ "surface_id": a, "path": "/tmp/does-not-matter.md" }),
            ),
            (
                "image.open",
                serde_json::json!({ "surface_id": a, "path": "/tmp/does-not-matter.png" }),
            ),
        ] {
            let req = ipc_request(method, params);
            let resp = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &req,
                &CallerContext::Local,
            );
            let err = resp.error.unwrap_or_else(|| {
                panic!("{method}: hard-occupied 워크스페이스에 대한 비-holder 요청은 거부돼야 한다")
            });
            assert!(
                err.message.to_lowercase().contains("occupied"),
                "{method}: 에러 메시지에 점유 안내가 있어야 한다 (got: {})",
                err.message
            );
        }
    }

    /// 없는 파일 오류까지 진행하면 점유 검사는 통과한 것이다. occupied 오류와 구분한다.
    #[test]
    fn dispatch_allows_markdown_navigate_when_not_occupied() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let req = ipc_request(
            "markdown.navigate",
            serde_json::json!({ "surface_id": a, "path": "/tmp/does-not-matter.md" }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &CallerContext::Local,
        );
        if let Some(err) = resp.error {
            assert!(
                !err.message.to_lowercase().contains("occupied"),
                "점유되지 않은 workspace 는 가드에 걸리면 안 된다: {}",
                err.message
            );
        }
    }
}

#[cfg(test)]
mod bulk_capacity_tests {
    use super::{dir_used_bytes, exceeds_capacity};

    #[test]
    fn dir_used_bytes_sums_top_level_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.bin"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.path().join("b.bin"), vec![0u8; 250]).unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("c.bin"), vec![0u8; 999]).unwrap();
        assert_eq!(dir_used_bytes(dir.path()), 350);
    }

    #[test]
    fn dir_used_bytes_missing_dir_is_zero() {
        let missing = std::path::Path::new("/nonexistent/tasty/transfers/xyz");
        assert_eq!(dir_used_bytes(missing), 0);
    }

    #[test]
    fn capacity_boundary_equal_is_allowed() {
        assert!(!exceeds_capacity(400, 100, 500));
        assert!(!exceeds_capacity(0, 500, 500));
        assert!(!exceeds_capacity(500, 0, 500));
    }

    #[test]
    fn capacity_boundary_over_is_rejected() {
        assert!(exceeds_capacity(400, 101, 500));
        assert!(exceeds_capacity(0, 501, 500));
    }

    #[test]
    fn capacity_saturates_on_overflow() {
        assert!(exceeds_capacity(u64::MAX, 1, 500));
        assert!(exceeds_capacity(u64::MAX - 1, 100, 1_000_000));
    }
}
