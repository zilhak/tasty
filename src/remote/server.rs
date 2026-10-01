//! attach 점유를 터미널 출력·입력, mesh·문서 조회, 구조 변경과 파일 전송에 연결한다.
//! GUI와 헤드리스 메인 루프가 StreamHub의 수신 결과를 이 모듈에 전달한다.
use super::transfer_spool::{Spool, TransferOwner};
use crate::runtime::engine_access::EngineMut;

use crate::runtime::engine_access::EngineRef;
use std::collections::HashMap;

use crate::app::services::AppServices;
use crate::core::CoreState;
use crate::core::attach::{AttachClientId, AttachError};
use crate::model::{AttachSurfaceClass, SurfaceId, WorkspaceId};
use tasty_ipc::stream::{StreamControl, StreamFrame, StreamTag, encode_mux};
use tasty_ipc::stream_hub::StreamHub;

mod content_queries;

#[cfg(feature = "gui")]
pub(crate) use content_queries::notify_markdown_changed;
pub(crate) use content_queries::{
    handle_git_query_request, handle_list_dir_request, handle_markdown_content_request,
};

impl EngineMut<'_> {
    pub(crate) fn refresh_attach_presentation(
        &mut self,
        presentation: &dyn crate::model::StructurePresentation,
    ) {
        self.remote.presentation = crate::model::StructurePresentationSnapshot::capture(
            &self.workspaces(),
            self.categories(),
            presentation,
        );
    }

    /// mesh surface는 PTY snapshot·tap 없이 점유와 attached 통지만 처리한다.
    /// 실제 구독은 후속 MeshContext 요청을 받은 App 계층에서 시작한다.
    fn attach_mesh_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        match self.live.occupancy.acquire(surface_id, client_id) {
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
        let Some(grant) = self.as_ref().attached_mesh_grant(surface_id, client_id) else {
            return false;
        };
        let Some(binding) = self
            .remote
            .notifier()
            .and_then(|hub| hub.client_binding(client_id))
        else {
            return false;
        };
        let Some(descriptor) = self.core.find_surface_by_id(surface_id) else {
            return false;
        };
        let activation = descriptor.activation_generation;
        self.remote.mesh_mirror.upsert(
            surface_id,
            client_id,
            grant,
            binding,
            activation,
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
        let Some(hub) = self.remote.notifier() else {
            return false;
        };
        if !self
            .as_ref()
            .attached_mesh_context_is_current(surface_id, &hub)
            || self
                .remote
                .mesh_mirror
                .get(surface_id)
                .is_none_or(|ctx| ctx.client_id != client_id)
        {
            return false;
        }
        self.remote.mesh_mirror.request_full_resend(surface_id)
    }

    /// 점유를 확인한 뒤 mesh mirror에서 온 입력을 쌓는다.
    pub fn apply_attached_mesh_input(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        input: tasty_plugin_protocol::protocol::RawInputWire,
    ) -> bool {
        let Some(hub) = self.remote.notifier() else {
            return false;
        };
        if !self
            .as_ref()
            .attached_mesh_context_is_current(surface_id, &hub)
            || self
                .remote
                .mesh_mirror
                .get(surface_id)
                .is_none_or(|ctx| ctx.client_id != client_id)
        {
            return false;
        }
        if self.remote.mesh_mirror.push_input(surface_id, input) {
            true
        } else {
            if let Some(ctx) = self.remote.mesh_mirror.get(surface_id) {
                hub.unregister_bound(client_id, &ctx.binding);
            }
            self.remote.mesh_mirror.remove(surface_id);
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
        let Some(ws) = self.live.occupancy.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.live.occupancy.workspace_holder(ws) != Some(client_id) {
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
}

/// 업로드 버퍼를 회수하고 이 engine의 workspace를 하나라도 점유한 client인지 확인한다.
/// 허용되면 캡처를 저장하고 서버 클립보드에 경로를 쓴 뒤 capture_result로 회신한다.
pub(crate) fn finalize_capture_upload(
    engine: &mut EngineMut<'_>,
    core: &AppServices,
    hub: &StreamHub,
    client_id: u32,
    upload_id: u64,
    file_name: &str,
) {
    let bytes = engine.remote.capture_uploads.take(client_id, upload_id);
    let is_holder = engine.live.occupancy.client_holds_workspace(client_id);
    let result = match (is_holder, bytes) {
        (false, _) => Err("client does not hold a workspace attach".to_string()),
        (true, None) => Err("no uploaded bytes for this upload_id".to_string()),
        (true, Some((owner, bytes))) if owner.current(&engine.as_ref(), hub, client_id) => {
            save_capture_and_set_clipboard(core, file_name, bytes)
        }
        (true, Some(_)) => Err("capture origin grant or registration retired".into()),
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
    core: &AppServices,
    file_name: &str,
    bytes: Spool,
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
    bytes: Spool,
) -> Result<String, String> {
    let safe_name = std::path::Path::new(file_name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(safe_name);
    bytes.save_to(&path)?;
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
    engine: &mut EngineMut<'_>,
    hub: &StreamHub,
    client_id: u32,
    transfer_id: u64,
    filename: String,
    total_size: u64,
) {
    let dir = resolve_bulk_transfer_dir(&engine.runtime.settings);
    let max_bytes = engine
        .runtime
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
    let Some(owner) = TransferOwner::capture(&engine.as_ref(), hub, client_id, true) else {
        reject_bulk_transfer(
            hub,
            client_id,
            transfer_id,
            "bulk origin is no longer attached",
        );
        return;
    };
    if let Err(error) =
        engine
            .remote
            .bulk_transfers
            .begin(client_id, transfer_id, filename, total_size, owner)
    {
        reject_bulk_transfer(hub, client_id, transfer_id, &error);
    }
}

/// 연결에 지정된 bulk_workspace를 누군가 점유하고 있는지 확인한 뒤 저장하고 경로를 회신한다.
/// bulk client 자체의 점유나 SSH 연결의 동일성을 여기서 확인하지는 않는다.
/// dir가 없으면 저장할 수 없으며 클립보드는 변경하지 않는다.
pub(crate) fn finalize_bulk_transfer(
    engine: &mut EngineMut<'_>,
    hub: &StreamHub,
    client_id: u32,
    transfer_id: u64,
    bulk_workspace: WorkspaceId,
    dir: Option<std::path::PathBuf>,
) {
    let authorized = engine
        .live
        .occupancy
        .workspace_holder(bulk_workspace)
        .is_some();
    // 권한 확인에 실패해도 버퍼를 회수해 큰 업로드가 메모리에 남지 않게 한다.
    let taken = engine.remote.bulk_transfers.take(client_id, transfer_id);
    let result = if !authorized {
        Err("bound workspace has no active holder".to_string())
    } else {
        match (taken, dir) {
            (None, _) => Err("no uploaded bytes for this transfer_id".to_string()),
            (Some(_), None) => Err("no tasty home directory".to_string()),
            (Some((filename, owner, bytes)), Some(d))
                if owner.current(&engine.as_ref(), hub, client_id) =>
            {
                save_bulk_file(&d, &filename, "bulk-file", bytes)
            }
            (Some(_), Some(_)) => Err("bulk origin grant or registration retired".into()),
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

impl EngineRef<'_> {
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
            "name": self.core.workspace_at(idx).expect("workspace index is valid").name,
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
        let ws = self
            .core
            .workspace_at(idx)
            .expect("workspace index is valid");
        let mut kinds: HashMap<u32, &'static str> = HashMap::new();
        let mut display_names: HashMap<u32, String> = HashMap::new();
        for id in ws.all_surface_ids() {
            if let Some(surface) = self.runtime.surfaces.get(&id) {
                kinds.insert(id, surface.kind());
                display_names.insert(id, surface.display_name());
            }
        }
        let (mesh_whitelisted, mesh_rejected) = mesh_mirror_candidates(class);
        let (content_whitelisted, content_rejected) = content_mirror_candidates(class);

        let mut surfaces = Vec::new();
        for &sid in &class.terminals {
            let (cols, rows) = self
                .runtime
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
        (
            ws.to_attach_tree_json(&self.observed_presentation(&self.remote.presentation)),
            surfaces,
        )
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
        if crate::runtime::surface_registry::egui_mesh::is_egui_mesh_allowed(kind, plugin_id) {
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
    use crate::runtime::egui_mesh_surface::EguiMeshSurface;

    fn engine_with_mesh_surface(
        kind: &'static str,
        plugin_id: &str,
        display_name: &str,
    ) -> (crate::runtime::engine_session::EngineSession, usize) {
        let term_waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, term_waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let surface: Box<dyn crate::model::Surface> = Box::new(EguiMeshSurface::new(
            100,
            kind,
            plugin_id.to_string(),
            display_name.to_string(),
            Some("/docs/README.md".to_string()),
        ));
        let pane = crate::model::Pane::new_with_surface(1, 1, display_name.to_string(), surface);
        let ws = crate::model::Workspace::new_with_pane(1, "ws".to_string(), pane);
        engine.push_local_workspace(ws);
        let idx = engine.workspaces().len() - 1;
        (engine_session, idx)
    }

    #[test]
    fn image_mesh_descriptor_carries_real_display_name() {
        let (mut engine_session, idx) =
            engine_with_mesh_surface("image", "com.tasty.image", "screenshot.png");
        let engine = engine_session.borrow_mut();
        let class = engine.classify_attach_surfaces(
            engine
                .workspace_at(idx)
                .expect("workspace index is valid")
                .id,
        );
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
            let (mut engine_session, idx) = engine_with_mesh_surface(kind, plugin_id, name);
            let engine = engine_session.borrow_mut();
            let class = engine.classify_attach_surfaces(
                engine
                    .workspace_at(idx)
                    .expect("workspace index is valid")
                    .id,
            );
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
    use crate::app::attach_structure::execute_forwarded_structural_op;
    use crate::state::RequestContext;
    use tasty_ipc::stream::{ForwardOrigin, SplitAxis, StructuralOp};
    use tasty_terminal::Terminal;

    fn make_core_state() -> (
        crate::app::services::AppServices,
        RequestContext,
        crate::runtime::engine_session::EngineSession,
        tempfile::TempDir,
    ) {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStorage;
        use tasty_themes::{ThemeStorage, ThemeStore};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::app::services::builder::AppServicesBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let term_waker: tasty_terminal::Waker = Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, term_waker).unwrap();
        let mut engine = engine_session.borrow_mut();
        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let state = RequestContext::new(&mut engine, preset_store.clone(), memory.clone());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let core = AppServicesBuilder::new()
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
            .expect("test AppServices build");
        (core, state, engine_session, home_tmp)
    }

    fn seed(engine: &mut crate::runtime::engine_access::EngineMut<'_>) -> u32 {
        let a = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        engine
            .runtime
            .terminals
            .insert(a, Terminal::new_detached(80, 24), None);
        a
    }

    fn delta_surface_ids(
        fd: &crate::app::attach_structure::ForwardedDelta,
    ) -> std::collections::HashSet<u32> {
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let before = engine.runtime.terminals.iter().count();
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
            engine.runtime.terminals.iter().count(),
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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

    /// pane의 마지막 탭과 workspace의 유일한 pane은 닫히지 않으므로 기록도 남기지 않는다.
    #[test]
    fn a_forwarded_close_that_cannot_close_leaves_no_snapshot() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        for op in [
            StructuralOp::CloseTab {
                anchor_surface_id: a,
            },
            StructuralOp::ClosePane {
                anchor_surface_id: a,
            },
        ] {
            let before = engine.closed_items.len();
            let result = execute_forwarded_structural_op(
                &mut core,
                &mut state,
                &mut engine,
                &op,
                ForwardOrigin::User,
            );
            assert!(
                engine.find_terminal_by_id(a).is_some(),
                "{op:?}: 마지막 탭·pane은 닫히지 않는다 ({:?})",
                result.map(|_| ())
            );
            assert_eq!(
                engine.closed_items.len(),
                before,
                "{op:?}: 닫히지 않은 대상은 서버 복원 스택에 남기지 않는다"
            );
        }
    }

    fn tabs_and_selection(engine: &crate::core::CoreState, surface_id: u32) -> (usize, usize) {
        let pane_id = engine.find_pane_for_surface(surface_id).expect("pane");
        let pane = engine.find_pane_by_id(pane_id).expect("pane");
        (
            pane.tabs.len(),
            crate::model::StructurePresentation::tab_index(&engine.remote.presentation, pane),
        )
    }

    fn forward_empty_new_tab(
        core: &mut crate::app::services::AppServices,
        state: &mut RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        forward_empty_new_tab(&mut core, &mut state, &mut engine, a, ForwardOrigin::Agent);
        assert_eq!(tabs_and_selection(&engine, a), (2, 0));
    }

    #[test]
    fn a_forwarded_user_new_tab_selects_it() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        forward_empty_new_tab(&mut core, &mut state, &mut engine, a, ForwardOrigin::User);
        assert_eq!(tabs_and_selection(&engine, a), (2, 1));
    }

    #[test]
    fn ipc_tab_create_of_a_non_terminal_keeps_the_selected_tab() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
    fn forward_origins_map_to_a_remote_agent() {
        use crate::core::origin::{AgentSource, IntentOrigin};
        for origin in [ForwardOrigin::User, ForwardOrigin::Agent] {
            assert!(
                matches!(
                    crate::app::attach_structure::forward_intent_origin(origin),
                    IntentOrigin::Agent {
                        source: AgentSource::Remote
                    }
                ),
                "{origin:?}"
            );
        }
    }

    fn split_op(anchor: u32, pane: bool) -> StructuralOp {
        let surface_kind = "terminal".to_string();
        let params = serde_json::json!({});
        if pane {
            StructuralOp::SplitPane {
                anchor_surface_id: anchor,
                direction: SplitAxis::Vertical,
                surface_kind,
                params,
            }
        } else {
            StructuralOp::SplitSurface {
                surface_id: anchor,
                direction: SplitAxis::Vertical,
                surface_kind,
                params,
            }
        }
    }

    /// 원격 사용자 조작이어도 서버 호스트 사용자의 선택 pane·surface는 그대로다.
    #[test]
    fn a_forwarded_user_split_keeps_the_server_users_focus() {
        for pane in [true, false] {
            let (mut core, mut state, mut engine_session, _home) = make_core_state();
            let mut engine = engine_session.borrow_mut();
            let a = seed(&mut engine);
            let before = (
                state.focused_pane_id(&engine),
                state.focused_surface_id(&engine),
            );
            let fd = execute_forwarded_structural_op(
                &mut core,
                &mut state,
                &mut engine,
                &split_op(a, pane),
                ForwardOrigin::User,
            )
            .expect("split ok")
            .expect("split delta");
            assert_eq!(fd.added_terminals.len(), 1, "pane={pane}");
            assert_eq!(
                (
                    state.focused_pane_id(&engine),
                    state.focused_surface_id(&engine),
                ),
                before,
                "pane={pane}"
            );
        }
    }

    /// 복원 기록은 ForwardOrigin에서 정하고, lifecycle의 사용자 닫기 표시는 서버 사용자 기준이다.
    #[test]
    fn a_forwarded_user_close_is_restorable_but_not_a_local_user_close() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let fd = execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split_op(a, false),
            ForwardOrigin::User,
        )
        .expect("split ok")
        .expect("split delta");
        let b = fd.added_terminals[0];
        let before = engine.closed_items.len();
        state.pending_lifecycle_events.clear();

        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &StructuralOp::CloseSurface { surface_id: b },
            ForwardOrigin::User,
        )
        .expect("close ok");

        assert_eq!(engine.closed_items.len(), before + 1);
        // lifecycle 통지는 gui 빌드에만 있다.
        if cfg!(feature = "gui") {
            assert!(!state.pending_lifecycle_events.is_empty());
            assert!(
                state
                    .pending_lifecycle_events
                    .iter()
                    .all(|e| !e.is_user_close)
            );
        }
    }

    /// 서버도 mirror면 요청은 다시 전달된다. 그 실패는 서버 사용자 toast로 가지 않는다.
    #[cfg(feature = "gui")] // headless에는 전달 큐를 보내는 루프가 없어 거절한다
    #[test]
    fn a_chained_forward_stays_a_silent_agent_request() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let active = state.active_workspace_index(&engine);
        engine.make_mirror_fixture(active);

        let surfaces_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()
            .len();

        // 다시 전달한 요청은 forward_result가 성공으로 돌려준다.
        execute_forwarded_structural_op(
            &mut core,
            &mut state,
            &mut engine,
            &split_op(a, true),
            ForwardOrigin::User,
        )
        .expect("forwarded again");

        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .all_surface_ids()
                .len(),
            surfaces_before,
            "mirror 구조 변경은 로컬에서 실행하지 않는다"
        );
        assert_eq!(engine.remote.pending_structural_forward.len(), 1);
        let forwarded = &engine.remote.pending_structural_forward[0];
        assert!(forwarded.silent_failure);
        assert!(!forwarded.user_triggered);
    }

    #[test]
    fn a_forwarded_agent_close_leaves_no_snapshot() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
            &crate::core::origin::IPC_AGENT,
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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

        let before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()
            .len();
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
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .all_surface_ids()
                .len(),
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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

        let ws_before = state.active_workspace_index(&engine);
        let focus_before: Vec<(u32, u32)> = engine
            .workspaces()
            .into_iter()
            .map(|w| (w.id, state.navigation.pane_id(w).unwrap_or(0)))
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
            state.active_workspace_index(&engine),
            ws_before,
            "원격의 복원이 로컬 사용자의 활성 워크스페이스를 바꾸면 안 된다"
        );
        let focus_after: Vec<(u32, u32)> = engine
            .workspaces()
            .into_iter()
            .map(|w| (w.id, state.navigation.pane_id(w).unwrap_or(0)))
            .collect();
        assert_eq!(
            focus_before, focus_after,
            "원격의 복원이 로컬 사용자의 focused pane 을 바꾸면 안 된다"
        );
    }

    #[test]
    fn a_forwarded_restore_never_takes_another_workspaces_item() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let other_sid = engine
            .workspace_at(other_ws)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        engine
            .runtime
            .terminals
            .insert(other_sid, Terminal::new_detached(80, 24), None);
        let other_pane = engine
            .workspace_at(other_ws)
            .expect("workspace index is valid")
            .pane_layout()
            .first_pane()
            .unwrap()
            .id;
        let other_tab_idx = 0;
        let snapshot = engine
            .capture_closed_tab(other_pane, other_tab_idx, &state.navigation)
            .expect("other workspace tab snapshot");
        engine.push_closed_item(snapshot);

        let restored_ws0 = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()
            .len();
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
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .all_surface_ids()
                .len(),
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let client_id = 42;
        engine
            .live
            .occupancy
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
            engine.live.occupancy.is_hard_occupied(new_sid),
            "새 surface는 hard 점유 목록에 등록돼야 한다"
        );
        assert_eq!(
            engine.live.occupancy.workspace_of_surface(new_sid),
            Some(ws_id),
            "새 surface는 점유 workspace의 멤버로 등록돼야 한다"
        );
        assert_eq!(
            engine.live.occupancy.workspace_holder_of(new_sid),
            Some(client_id),
            "새 surface 의 holder 는 workspace holder 와 동일해야 한다"
        );
    }

    /// notifier를 주입해야 tap 경로가 실행된다. 실행 중 자동 tap과 호출자의 후속 tap이 겹치지 않는지 본다.
    #[test]
    fn forward_split_surface_taps_exactly_once_with_real_stream_hub() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let client_id = 7;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");
        let hub = tasty_ipc::stream_hub::StreamHub::new();
        let _rx = hub.register(client_id);
        engine.remote.set_notifier(hub.clone());

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
            engine
                .runtime
                .terminals
                .get(new_sid)
                .unwrap()
                .output_tap_count(),
            0,
            "구조 변경 실행 중에는 tap하지 않고 호출자가 delta를 보낸 뒤 tap해야 한다"
        );

        engine.tap_surface_for_stream(new_sid, client_id, &hub);
        assert_eq!(
            engine
                .runtime
                .terminals
                .get(new_sid)
                .unwrap()
                .output_tap_count(),
            1,
            "호출자가 tap한 뒤에는 하나만 등록돼야 한다"
        );
    }

    /// 사용자의 로컬 분할은 forward 억제 구간 밖이라 생성 직후 delta를 보내고 한 번 tap한다.
    /// 비-holder IPC의 생성은 라우터 가드가 거절하므로 AppServices::apply로 이 후처리를 확인한다.
    #[test]
    fn local_split_in_held_workspace_sends_delta_then_taps_once() {
        use crate::app::command::{CoreEvent, DomainIntent};
        use tasty_ipc::stream::{StreamTag, decode_mux};

        let (mut core, _state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let client_id = 7;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");
        let hub = tasty_ipc::stream_hub::StreamHub::new();
        let rx = hub.register(client_id);
        engine.remote.set_notifier(hub.clone());

        let events = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: crate::model::SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect("local split must succeed");
        let Some(CoreEvent::SurfaceSplit { new_surface_id, .. }) = events.into_iter().next() else {
            panic!("expected SurfaceSplit event");
        };

        assert_eq!(
            engine.live.occupancy.workspace_holder_of(new_surface_id),
            Some(client_id),
            "새 surface는 holder의 점유에 편입돼야 한다"
        );
        assert_eq!(
            engine
                .runtime
                .terminals
                .get(new_surface_id)
                .unwrap()
                .output_tap_count(),
            1,
            "로컬 생성은 즉시 한 번 tap해야 한다"
        );

        let frames: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let delta_at = frames.iter().position(|f| {
            f.tag == StreamTag::Control
                && serde_json::from_slice::<serde_json::Value>(&f.payload).is_ok_and(|v| {
                    v["event"] == "structural_delta"
                        && surface_ids_of(&v).contains(&u64::from(new_surface_id))
                })
        });
        let snapshot_at = frames.iter().position(|f| {
            f.tag == StreamTag::Data
                && decode_mux(&f.payload).is_some_and(|(sid, _)| sid == new_surface_id)
        });
        let (Some(delta_at), Some(snapshot_at)) = (delta_at, snapshot_at) else {
            panic!("delta와 새 surface의 snapshot이 모두 와야 한다: {delta_at:?}, {snapshot_at:?}");
        };
        assert!(
            delta_at < snapshot_at,
            "client가 ID 매핑을 만든 뒤 snapshot을 받도록 delta가 먼저 와야 한다"
        );
    }

    fn attached_pair(
        core: &mut crate::app::services::AppServices,
        state: &mut RequestContext,
        engine: &mut crate::runtime::engine_access::EngineMut<'_>,
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
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a, b], &[a, b], 7)
            .expect("workspace 점유 획득");
        let hub = tasty_ipc::stream_hub::StreamHub::new();
        let rx = hub.register(7);
        engine.remote.set_notifier(hub);
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let (a, b, ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        let generation = engine
            .runtime
            .terminals
            .generation(b)
            .expect("terminal binding");
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, b, generation);
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
            engine.live.occupancy.workspace_holder(ws_id),
            Some(7),
            "점유는 그대로"
        );
    }

    #[test]
    fn a_forwarded_close_leaves_no_structure_change_behind() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let (a, b, ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        let generation = engine
            .runtime
            .terminals
            .generation(b)
            .expect("terminal binding");
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, b, generation);
        drain_control(&rx);
        let generation = engine
            .runtime
            .terminals
            .generation(a)
            .expect("terminal binding");
        crate::app::process_exit::handle(&mut core, &mut state, &mut engine, a, generation);
        assert!(
            engine.find_workspace_index_for_id(ws_id).is_none(),
            "시험 전제: 워크스페이스가 purge 됐다"
        );
        let msgs = drain_control(&rx);
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        assert_eq!(msgs[0]["event"], "force_detached");
        assert_eq!(
            engine.live.occupancy.workspace_holder(ws_id),
            None,
            "stale lock 없음"
        );
    }

    #[test]
    fn a_local_member_reaches_the_holder_as_a_delta_before_its_snapshot() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let (a, _b, _ws_id, rx) = attached_pair(&mut core, &mut state, &mut engine);
        let pane_id = engine.find_pane_for_surface(a).expect("pane");
        crate::app::structural_exec::create_tab(
            &mut core,
            &mut state,
            &mut engine,
            pane_id,
            &serde_json::json!({ "pane_id": pane_id }),
            false,
            &crate::core::origin::IPC_AGENT,
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
            Box::new(crate::runtime::egui_mesh_surface::EguiMeshSurface::new(
                surface_id,
                "image",
                "com.tasty.image".to_string(),
                "elsewhere".to_string(),
                None,
            ));
        let pane =
            crate::model::Pane::new_with_surface(ws_id, ws_id, "elsewhere".to_string(), surface);
        engine.push_local_workspace(crate::model::Workspace::new_with_pane(
            ws_id,
            "elsewhere".to_string(),
            pane,
        ));
    }

    #[test]
    fn an_anchor_alive_in_another_workspace_is_not_called_gone() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let (_a, _b, _ws_id, _rx) = attached_pair(&mut core, &mut state, &mut engine);
        push_unrelated_workspace(&mut engine, 900, 901);
        let alive = StructuralOp::CloseSurface { surface_id: 901 };
        let gone = StructuralOp::CloseSurface { surface_id: 555 };
        let reason =
            |op| crate::remote::structure_sync::unresolved_forward_reason([&*engine.core], 7, op);
        assert_eq!(reason(&alive), "workspace not found");
        assert!(
            reason(&gone).starts_with("no live surface 555 "),
            "{}",
            reason(&gone)
        );
    }

    #[test]
    fn an_anchor_alive_in_another_engine_is_not_called_gone() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let (_a, _b, _ws_id, _rx) = attached_pair(&mut core, &mut state, &mut engine);
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut other_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine");
        let mut other = other_session.borrow_mut();
        push_unrelated_workspace(&mut other, 900, 901);
        let op = StructuralOp::CloseSurface { surface_id: 901 };
        let reason = |engines: Vec<&crate::core::CoreState>| {
            crate::remote::structure_sync::unresolved_forward_reason(engines, 7, &op)
        };
        assert_eq!(reason(vec![&engine, &other]), "workspace not found");
        assert_eq!(reason(vec![&other, &engine]), "workspace not found");
        assert!(reason(vec![&engine]).starts_with("no live surface 901 "));
    }

    #[test]
    fn forward_new_tab_executes() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let before = engine.runtime.terminals.iter().count();
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
        assert_eq!(engine.runtime.terminals.iter().count(), before + 1);
        assert_eq!(fd.added_terminals.len(), 1);
    }

    #[test]
    fn forwarded_tab_origin_changes_wire_selection_without_changing_server_selection() {
        for origin in [ForwardOrigin::User, ForwardOrigin::Agent] {
            let (mut core, mut state, mut engine_session, _home) = make_core_state();
            let mut engine = engine_session.borrow_mut();
            let anchor = seed(&mut engine);
            state.reconcile_presentation(&engine);
            let pane_id = engine.find_pane_for_surface(anchor).unwrap();
            let selected = state
                .navigation
                .tab_id(engine.find_pane_by_id(pane_id).unwrap());
            let result = execute_forwarded_structural_op(
                &mut core,
                &mut state,
                &mut engine,
                &StructuralOp::NewTab {
                    anchor_surface_id: anchor,
                    surface_kind: "empty".into(),
                    params: serde_json::json!({}),
                },
                origin,
            )
            .unwrap()
            .unwrap();
            let tasty_ipc::stream::StreamControl::StructuralDelta { tree, .. } = result.delta
            else {
                panic!("expected structural delta");
            };
            let tabs = tree["panes"][0]["tabs"].as_array().unwrap();
            assert_eq!(tabs[1]["active"], origin == ForwardOrigin::User);
            assert_eq!(tabs[0]["active"], origin == ForwardOrigin::Agent);
            assert_eq!(
                state
                    .navigation
                    .tab_id(engine.find_pane_by_id(pane_id).unwrap()),
                selected
            );
            // A later subscription takes the server's current View/defaults,
            // not the previous remote caller's transient creation projection.
            engine.refresh_attach_presentation(&state.navigation);
            assert_eq!(
                engine.remote.presentation.selected_tabs.get(&pane_id),
                selected.as_ref()
            );
        }
    }

    #[test]
    fn forward_close_tab_removes_from_delta() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let before = engine.runtime.terminals.iter().count();
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
            engine.runtime.terminals.iter().count(),
            before,
            "실패한 forward 는 새 터미널을 만들지 않는다"
        );
    }

    #[test]
    fn forward_convert_unknown_kind_reports_the_remote_reason() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
            engine.runtime.terminals.get(a).is_some(),
            "실패한 convert 는 원래 터미널을 그대로 둔다"
        );
    }

    /// 같은 실패 입력을 forward와 IPC에 넣는다. 기대 문구는 실행 결과와 별도로 고정해 두었다.
    struct FailureCase(
        &'static str,
        &'static str,
        Box<dyn Fn(u32) -> StructuralOp>,
        fn(
            &mut crate::app::services::AppServices,
            &mut dyn crate::adapters::ipc::window_port::IpcWindow,
            &mut crate::runtime::engine_access::EngineMut<'_>,
            serde_json::Value,
            &serde_json::Value,
            &crate::core::origin::IntentOrigin,
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
                "'target_pane' was given as \"not-a-number\" — it must be a whole number that fits in 32 bits and is not negative",
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let pane_id = engine.find_pane_for_surface(a).expect("seed pane");

        let resp = ipc(
            &mut core,
            &mut state,
            &mut engine,
            serde_json::json!(1),
            &ipc_params(a, pane_id),
            &crate::core::origin::IPC_AGENT,
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");
        let before = engine.runtime.terminals.iter().count();
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
        assert_eq!(engine.runtime.terminals.iter().count(), before + 1);
    }

    #[test]
    fn forward_split_pane_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");
        let panes_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()
            .len();
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
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            panes_before + 1
        );
    }

    #[test]
    fn forward_close_pane_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
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
        let all: Vec<u32> = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids();
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let panes_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()
            .len();
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
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            panes_before - 1
        );
    }

    #[test]
    fn forward_move_tab_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
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
        let all: Vec<u32> = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids();
        engine
            .live
            .occupancy
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
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
        let all: Vec<u32> = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids();
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &all, &all, 7)
            .expect("workspace 점유 획득");
        let before = engine.runtime.terminals.iter().count();
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
        assert_eq!(engine.runtime.terminals.iter().count(), before - 1);
    }

    #[test]
    fn forward_close_last_surface_force_detaches_holder() {
        use std::time::Duration;
        use tasty_ipc::stream::StreamTag;
        use tasty_ipc::stream_hub::StreamHub;

        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;

        let hub = StreamHub::new();
        let holder = hub.alloc_id();
        let rx = hub.register(holder);
        engine.remote.set_notifier(hub);
        let all: Vec<u32> = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids();
        engine
            .live
            .occupancy
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

        assert_eq!(engine.live.occupancy.workspace_holder(ws_id), None);
    }

    #[test]
    fn forward_convert_surface_executes_and_converts() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
            engine.runtime.terminals.get(a).is_some(),
            "terminal 로의 convert 는 새 Terminal 을 insert 해야 한다"
        );
    }

    #[test]
    fn forward_convert_surface_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let dir = tempfile::tempdir().expect("test tempdir");
        engine
            .runtime
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let dir = tempfile::tempdir().expect("test tempdir");
        engine
            .runtime
            .terminals
            .get_mut(a)
            .expect("seeded terminal")
            .set_cached_cwd(dir.path().to_path_buf());
        engine.runtime.settings.general.inherit_cwd = false;
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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
        let before = engine.runtime.terminals.iter().count();

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
            engine.runtime.terminals.iter().count(),
            before - 1,
            "이동 대상으로 덮어쓴 B의 터미널은 제거돼야 한다"
        );
        assert!(
            engine.runtime.terminals.get(a).is_some(),
            "이동한 A의 터미널은 유지돼야 한다"
        );
    }

    #[test]
    fn forward_move_surface_succeeds_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
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
        let all: Vec<u32> = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids();
        engine
            .live
            .occupancy
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.runtime.terminals.iter().count();
        let panes_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()
            .len();

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
            engine.runtime.terminals.iter().count(),
            terminals_before,
            "거부된 요청은 새 터미널을 만들면 안 된다"
        );
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            panes_before,
            "거부된 요청은 새 pane 을 만들면 안 된다"
        );
    }

    #[test]
    fn dispatch_denies_structural_close_move_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        let tab_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .find_pane(pane_id)
            .expect("pane exists")
            .tabs[0]
            .id;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.runtime.terminals.iter().count();
        let panes_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()
            .len();
        let tabs_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
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

        assert_eq!(engine.runtime.terminals.iter().count(), terminals_before);
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            panes_before,
            "거부된 요청은 pane 을 닫으면 안 된다"
        );
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .find_pane(pane_id)
                .expect("pane exists")
                .tabs
                .len(),
            tabs_before,
            "거부된 요청은 tab 을 닫거나 이동하면 안 된다"
        );
    }

    /// preset.apply·pty.attach_surface도 hard 점유 workspace에 탭·pane을 끼워 넣으므로 거부한다.
    /// 대상을 생략한 preset.apply는 활성 workspace의 pane에 적용되므로 같은 기준으로 판정한다.
    /// 거부는 preset 조회보다 먼저여서 이 시험은 preset 파일 없이 점유 사유만 확인한다.
    #[test]
    fn dispatch_denies_preset_apply_and_pty_attach_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        assert_eq!(
            state.active_workspace_index(&engine),
            0,
            "기본 대상은 점유된 workspace 여야 한다"
        );
        let pty_id =
            crate::adapters::ipc::handler::pty::tests::spawn_test_pty(&mut core, &mut engine);
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let tab_count = |engine: &crate::core::CoreState| {
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .find_pane(pane_id)
                .expect("pane exists")
                .tabs
                .len()
        };
        let tabs_before = tab_count(&engine);

        for (method, params) in [
            (
                "preset.apply",
                serde_json::json!({ "kind": "tab", "name": "any", "target_pane_id": pane_id }),
            ),
            (
                "preset.apply",
                serde_json::json!({ "kind": "tab", "name": "any" }),
            ),
            (
                "preset.apply",
                serde_json::json!({ "kind": "pane", "name": "any", "target_workspace_id": ws_id }),
            ),
            (
                "preset.apply",
                serde_json::json!({ "kind": "pane", "name": "any" }),
            ),
            (
                "pty.attach_surface",
                serde_json::json!({ "id": pty_id, "pane_id": pane_id }),
            ),
        ] {
            let req = ipc_request(method, params.clone());
            let resp = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &req,
                &CallerContext::Local,
            );
            let err = resp.error.unwrap_or_else(|| {
                panic!("{method} {params}: hard-occupied 워크스페이스에 대한 비-holder 요청은 거부돼야 한다")
            });
            assert!(
                err.message.to_lowercase().contains("occupied"),
                "{method} {params}: 에러 메시지에 점유 안내가 있어야 한다 (got: {})",
                err.message
            );
        }

        assert_eq!(
            tab_count(&engine),
            tabs_before,
            "거부된 요청은 탭을 만들면 안 된다"
        );
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()
                .len(),
            1
        );
        assert!(
            engine.runtime.terminals.is_standalone(pty_id),
            "거부된 pty.attach_surface 는 PTY 를 registry 에 남겨야 한다"
        );

        // workspace preset은 새 workspace를 만들므로 점유와 무관하다.
        let req = ipc_request(
            "preset.apply",
            serde_json::json!({ "kind": "workspace", "name": "any" }),
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
                "workspace preset 은 점유로 거부하면 안 된다 (got: {})",
                err.message
            );
        }
        engine.runtime.terminals.remove(pty_id);
    }

    /// mirror surface를 자식으로 둔 terminal.kill은 로컬에서 닫히지 않는다.
    /// GUI는 원격으로 전달했다고 답하고 헤드리스는 거절한다. 어느 쪽이든 관계와 soft 점유는 남는다.
    #[test]
    fn terminal_kill_keeps_the_child_when_a_mirror_child_is_not_closed_locally() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let parent = seed(&mut engine);
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        let child = engine.runtime.counters.next_surface();
        let tab_id = engine.runtime.counters.next_tab();
        engine
            .runtime
            .terminals
            .insert(child, Terminal::new_detached(80, 24), None);
        engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("pane exists")
            .add_terminal_marker_tab_background(tab_id, child, None);
        let adopt = ipc_request(
            "terminal.adopt",
            serde_json::json!({ "surface": parent, "target": child }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &adopt,
            &CallerContext::Local,
        );
        let idx = resp.result.expect("adopt 성공")["child_index"]
            .as_u64()
            .expect("child_index") as u32;
        engine.make_mirror_fixture(0);

        let kill = ipc_request(
            "terminal.kill",
            serde_json::json!({ "surface": parent, "child": idx }),
        );
        let resp = handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &kill,
            &CallerContext::Local,
        );

        if cfg!(feature = "gui") {
            let result = resp
                .result
                .unwrap_or_else(|| panic!("forward 는 성공 응답이다: {:?}", resp.error));
            assert_eq!(result["forwarded"], true, "{result}");
            assert!(
                result.get("killed_surface_id").is_none(),
                "닫지 않은 surface 를 죽였다고 답하면 안 된다: {result}"
            );
            assert_eq!(engine.remote.pending_structural_forward.len(), 1);
        } else {
            assert!(resp.error.is_some(), "헤드리스는 mirror 닫기를 거절한다");
        }
        assert!(
            engine
                .runtime
                .child_terminals
                .find_child(parent, idx)
                .is_some(),
            "닫히지 않은 child 의 관계를 지우면 안 된다"
        );
        assert!(
            engine.live.occupancy.occupancy_of(child).is_some(),
            "닫히지 않은 child 의 soft 점유를 풀면 안 된다"
        );
        assert!(engine.find_surface_by_id(child).is_some());
    }

    /// terminal.kill은 child가 hard 점유 workspace에 있으면 surface.close처럼 거부한다.
    /// 점유를 먼저 강제 해제하면 holder가 workspace 전체에서 떨어져 나가므로 registry·점유를 그대로 둔다.
    /// 점유가 없으면 child의 soft 점유만 풀고 닫는다.
    #[test]
    fn dispatch_denies_terminal_kill_when_child_is_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let parent = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        let child = engine.runtime.counters.next_surface();
        let tab_id = engine.runtime.counters.next_tab();
        engine
            .runtime
            .terminals
            .insert(child, Terminal::new_detached(80, 24), None);
        engine
            .workspace_at_mut(0)
            .expect("workspace index is valid")
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .expect("pane exists")
            .add_terminal_marker_tab_background(tab_id, child, None);
        let idx = engine.runtime.child_terminals.next_index_for(parent);
        engine.runtime.child_terminals.register_child(
            parent,
            crate::runtime::child_terminal::ChildEntry {
                child_surface_id: child,
                index: idx,
                cwd: None,
                role: None,
                nickname: None,
            },
        );
        engine
            .occupy_soft(child, parent, None)
            .expect("child soft 점유");
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[parent, child], &[parent, child], 7)
            .expect("workspace 점유 획득");

        let kill = |core: &mut crate::app::services::AppServices,
                    state: &mut RequestContext,
                    engine: &mut crate::runtime::engine_access::EngineMut<'_>| {
            let req = ipc_request(
                "terminal.kill",
                serde_json::json!({ "surface": parent, "child": idx }),
            );
            handle_with_caller(core, state, engine, &req, &CallerContext::Local)
        };

        let resp = kill(&mut core, &mut state, &mut engine);
        let err = resp
            .error
            .expect("hard 점유 child 의 terminal.kill 은 거부돼야 한다");
        assert!(
            err.message.contains("hard-occupied"),
            "surface.close 와 같은 점유 사유여야 한다 (got: {})",
            err.message
        );
        assert_eq!(
            engine.live.occupancy.workspace_holder(ws_id),
            Some(7),
            "거부된 kill 은 holder 를 떼어내면 안 된다"
        );
        assert!(
            engine
                .runtime
                .child_terminals
                .find_child(parent, idx)
                .is_some(),
            "거부된 kill 은 child 관계를 지우면 안 된다"
        );
        assert!(engine.find_surface_by_id(child).is_some());

        // holder가 떠난 뒤에는 kill이 soft 점유를 풀고 surface를 닫는다.
        engine.force_detach_workspace(ws_id);
        engine
            .occupy_soft(child, parent, None)
            .expect("hard 해제 뒤 soft 점유 복원");
        let resp = kill(&mut core, &mut state, &mut engine);
        assert!(resp.error.is_none(), "{:?}", resp.error);
        assert!(
            engine
                .runtime
                .child_terminals
                .find_child(parent, idx)
                .is_none()
        );
        assert!(engine.live.occupancy.occupancy_of(child).is_none());
        assert!(engine.find_surface_by_id(child).is_none());
    }

    #[test]
    fn dispatch_denies_terminal_spawn_when_hard_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &[a], &[a], 7)
            .expect("workspace 점유 획득");

        let terminals_before = engine.runtime.terminals.iter().count();
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        let tabs_before = engine
            .workspace_at(0)
            .expect("workspace index is valid")
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
            engine.runtime.terminals.iter().count(),
            terminals_before,
            "거부된 spawn 은 새 터미널을 만들면 안 된다"
        );
        assert_eq!(
            engine
                .workspace_at(0)
                .expect("workspace index is valid")
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let terminals_before = engine.runtime.terminals.iter().count();

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
            engine.runtime.terminals.iter().count(),
            terminals_before + 1,
            "정상 spawn 은 새 터미널을 만들어야 한다"
        );
    }

    // terminal.spawn은 생성된 surface를 동기 응답으로 요구한다. forward하면 원격에 탭만 남을 수 있다.

    #[test]
    fn dispatch_denies_terminal_spawn_into_mirror_workspace() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine.make_mirror_fixture(0);

        let terminals_before = engine.runtime.terminals.iter().count();

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
            engine.remote.pending_structural_forward.is_empty(),
            "거부된 spawn 은 원격 NewTab 을 forward 하면 안 된다"
        );
        assert_eq!(engine.runtime.terminals.iter().count(), terminals_before);
    }

    /// GUI는 mirror의 tab.create를 forward한다. 큐를 비우는 메인 루프가 GUI에만 있다.
    #[cfg(feature = "gui")]
    #[test]
    fn dispatch_still_forwards_tab_create_in_mirror_workspace() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        seed(&mut engine);
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        engine.make_mirror_fixture(0);
        let terminals_before = engine.runtime.terminals.iter().count();

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
                engine
                    .remote
                    .pending_structural_forward
                    .first()
                    .map(|p| &p.op),
                Some(StructuralOp::NewTab { .. })
            ),
            "mirror 의 tab.create 는 원격 NewTab 으로 큐잉돼야 한다 (got: {:?})",
            engine
                .remote
                .pending_structural_forward
                .first()
                .map(|p| &p.op)
        );
        assert_eq!(
            engine.runtime.terminals.iter().count(),
            terminals_before,
            "forward 된 구조 변경은 로컬 트리를 바꾸지 않는다"
        );
    }

    /// 헤드리스에는 forward 큐를 보내는 경로가 없어 mirror 요청을 성공으로 답하면 안 된다.
    #[cfg(not(feature = "gui"))]
    #[test]
    fn dispatch_refuses_tab_create_in_mirror_workspace_in_headless() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        seed(&mut engine);
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
        engine.make_mirror_fixture(0);
        let terminals_before = engine.runtime.terminals.iter().count();

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
            engine.remote.pending_structural_forward.is_empty(),
            "헤드리스에서 mirror forward 큐에 요청을 넣으면 안 된다"
        );
        assert_eq!(engine.runtime.terminals.iter().count(), terminals_before);
    }

    /// workspace와 다른 pane을 지정해도 최종 pane의 점유·mirror 여부로 차단해야 한다.
    #[test]
    fn dispatch_denies_terminal_spawn_when_pane_override_targets_blocked_workspace() {
        for blocked in ["mirror", "hard-occupied"] {
            let (mut core, mut state, mut engine_session, _home) = make_core_state();
            let mut engine = engine_session.borrow_mut();
            let a = seed(&mut engine);
            let blocked_ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
            let blocked_pane = engine
                .workspace_at(0)
                .expect("workspace index is valid")
                .pane_layout()
                .all_pane_ids()[0];

            let create = handle_with_caller(
                &mut core,
                &mut state,
                &mut engine,
                &ipc_request("workspace.create", serde_json::json!({ "name": "clean" })),
                &CallerContext::Local,
            );
            assert!(create.error.is_none(), "테스트 준비: {:?}", create.error);
            let clean_ws_id = engine
                .workspaces()
                .into_iter()
                .find(|w| w.id != blocked_ws_id)
                .expect("두 번째 워크스페이스")
                .id;

            match blocked {
                "mirror" => engine.make_mirror_fixture(0),
                _ => {
                    engine
                        .live
                        .occupancy
                        .acquire_workspace(blocked_ws_id, &[a], &[a], 7)
                        .expect("workspace 점유 획득");
                }
            }

            let terminals_before = engine.runtime.terminals.iter().count();
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
                engine.runtime.terminals.iter().count(),
                terminals_before,
                "{blocked}: 거부된 spawn 은 새 터미널을 만들면 안 된다"
            );
            assert!(
                engine.remote.pending_structural_forward.is_empty(),
                "{blocked}: 거부된 spawn 은 원격으로 아무것도 보내지 않는다"
            );
        }
    }

    #[test]
    fn dispatch_allows_tab_create_when_not_occupied() {
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        seed(&mut engine);
        let pane_id = engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .pane_layout()
            .all_pane_ids()[0];
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
        let a = seed(&mut engine);
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        engine
            .live
            .occupancy
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
        let (mut core, mut state, mut engine_session, _home) = make_core_state();
        let mut engine = engine_session.borrow_mut();
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

impl crate::runtime::engine_access::EngineMut<'_> {
    /// surface를 점유하고 화면 snapshot과 이후 출력을 보낸다. 거절하면 attach_error와 Detach를 보낸다.
    /// hub는 연결을 등록한 허브여야 한다. forwarder는 채널 EOF 또는 다음 push의 끊김 결과로 종료한다.
    pub fn attach_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        if !self.runtime.terminals.contains(surface_id)
            && !self.as_ref().is_surface_deferred(surface_id)
        {
            if let Some((kind, plugin_id)) = self.as_ref().find_mesh_surface_info(surface_id)
                && crate::runtime::surface_registry::egui_mesh::is_egui_mesh_allowed(
                    &kind, &plugin_id,
                )
            {
                self.attach_mesh_surface_for_stream(surface_id, client_id, hub);
                return;
            }
            reject_attach(hub, client_id, "not_found", None);
            return;
        }

        match self.live.occupancy.acquire(surface_id, client_id) {
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

        if !self.runtime.terminals.contains(surface_id) {
            let _ = self.live.occupancy.release(surface_id, client_id); // already released has no additional work.
            reject_attach(hub, client_id, "spawn_failed", None);
            return;
        }
        self.subscribe_terminal(surface_id, client_id, hub, false, true);

        tracing::debug!("attach: surface {surface_id} -> client {client_id}");
    }

    /// 점유한 surface의 PTY로 입력을 보낸다. 서버 로컬 입력 차단은 우회한다.
    /// 해당 점유나 터미널이 없으면 false다.
    pub fn feed_attached_input(&mut self, client_id: AttachClientId, bytes: &[u8]) -> bool {
        let Some(surface_id) = self.live.occupancy.surface_held_by(client_id) else {
            return false;
        };
        if !self.live.occupancy.surface_attachment_ready(surface_id) {
            return false;
        }
        if let Some(terminal) = self.runtime.terminals.get_mut(surface_id) {
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
    ) -> bool {
        let Some(idx) = self.find_workspace_index_for_id(workspace_id) else {
            reject_attach(hub, client_id, "workspace_not_found", None);
            return false;
        };
        let class = self
            .classify_attach_surfaces(self.workspace_at(idx).expect("workspace index is valid").id);
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

        match self.live.occupancy.acquire_workspace(
            workspace_id,
            &class.terminals,
            &members,
            client_id,
        ) {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return false;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return false;
            }
        }

        let descriptor = self.build_workspace_descriptor(idx, workspace_id, &class);
        let descriptor_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&descriptor).unwrap_or_default(),
        );
        if hub.push(client_id, descriptor_frame) != PushResult::Sent {
            return false;
        }

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
        true
    }

    /// workspace 연결에 터미널 snapshot과 출력·resize tap을 등록한다.
    /// snapshot과 tap은 한 번의 잠금으로 걸어 그 사이 출력이 빠지지 않게 한다.
    /// Engine 소유 subscription은 원 grant·terminal generation·hub registration이 바뀌면 폐기한다.
    pub(crate) fn tap_surface_for_stream(
        &mut self,
        sid: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        self.subscribe_terminal(sid, client_id, hub, true, false);
    }

    /// 해당 workspace를 점유한 client의 입력만 지정 터미널로 보낸다.
    pub fn feed_attached_workspace_input(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        bytes: &[u8],
    ) -> bool {
        if !self
            .live
            .occupancy
            .surface_attachment_ready(remote_surface_id)
        {
            return false;
        }
        let Some(ws) = self.live.occupancy.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if !self.live.occupancy.workspace_attachment_ready(ws)
            || self.live.occupancy.workspace_holder(ws) != Some(client_id)
        {
            return false;
        }
        if let Some(terminal) = self.runtime.terminals.get_mut(remote_surface_id) {
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
        let Some(ws) = self.live.occupancy.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.live.occupancy.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        if self.runtime.terminals.contains(remote_surface_id) {
            self.runtime.terminals.resize(remote_surface_id, cols, rows);
            true
        } else {
            false
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
}

impl crate::runtime::engine_access::EngineMut<'_> {
    fn build_workspace_descriptor(
        &self,
        idx: usize,
        workspace_id: u32,
        class: &AttachSurfaceClass,
    ) -> serde_json::Value {
        self.as_ref()
            .build_workspace_descriptor(idx, workspace_id, class)
    }

    /// 초기 attach와 StructuralDelta가 공유하는 트리·surface 정보다.
    pub(crate) fn build_workspace_tree_surfaces(
        &self,
        idx: usize,
        class: &AttachSurfaceClass,
    ) -> (serde_json::Value, Vec<serde_json::Value>) {
        self.as_ref().build_workspace_tree_surfaces(idx, class)
    }
}

impl EngineRef<'_> {
    fn attached_mesh_grant(&self, surface: u32, client: u32) -> Option<u64> {
        let direct = self
            .live
            .occupancy
            .locks_snapshot()
            .into_iter()
            .find(|(id, _)| *id == surface)
            .map(|(_, grant)| grant);
        let grant = direct.or_else(|| {
            let workspace = self.live.occupancy.workspace_of_surface(surface)?;
            self.live
                .occupancy
                .workspaces_snapshot()
                .into_iter()
                .find(|(id, _)| *id == workspace)
                .map(|(_, grant)| grant)
        })?;
        (grant.holder == client && grant.ready).then_some(grant.granted_seq)
    }
    pub(crate) fn attached_mesh_context_is_current(&self, surface: u32, hub: &StreamHub) -> bool {
        self.remote.mesh_mirror.get(surface).is_some_and(|ctx| {
            self.attached_mesh_grant(surface, ctx.client_id) == Some(ctx.grant)
                && hub.matches_client_binding(ctx.client_id, &ctx.binding)
                && self
                    .core
                    .find_surface_by_id(surface)
                    .is_some_and(|descriptor| descriptor.activation_generation == ctx.activation)
        })
    }
}

fn reject_bulk_transfer(hub: &StreamHub, client: u32, transfer: u64, reason: &str) {
    let Some(binding) = hub.client_binding(client) else {
        return;
    };
    let reply = StreamControl::BulkResult {
        transfer_id: transfer,
        ok: false,
        path: None,
        reason: Some(reason.into()),
    };
    hub.push_bound(
        client,
        &binding,
        StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&reply).unwrap_or_default(),
        ),
    );
}
pub(crate) fn append_bulk_transfer(
    engine: &mut EngineMut<'_>,
    hub: &StreamHub,
    client: u32,
    transfer: u64,
    seq: u32,
    bytes: &[u8],
) -> bool {
    let accepted =
        TransferOwner::capture(&engine.as_ref(), hub, client, true).is_some_and(|owner| {
            engine
                .remote
                .bulk_transfers
                .append(client, transfer, seq, bytes, &owner)
        });
    if !accepted {
        reject_bulk_transfer(
            hub,
            client,
            transfer,
            "bulk transfer rejected: stale origin, invalid sequence/length, or spool failure",
        );
        if let Some(binding) = hub.client_binding(client) {
            hub.unregister_bound(client, &binding);
        }
        engine.remote.bulk_transfers.clear_client(client);
    }
    accepted
}
pub(crate) fn append_capture_upload(
    engine: &mut EngineMut<'_>,
    hub: &StreamHub,
    client: u32,
    upload: u64,
    bytes: &[u8],
    now: std::time::Instant,
) {
    let accepted =
        TransferOwner::capture(&engine.as_ref(), hub, client, false).is_some_and(|owner| {
            engine
                .remote
                .capture_uploads
                .append(client, upload, bytes, now, owner)
        });
    if !accepted {
        if let Some(binding) = hub.client_binding(client) {
            let response = serde_json::json!({"event":"capture_result","upload_id":upload,"ok":false,"reason":"capture transfer origin or spool unavailable"});
            hub.push_bound(
                client,
                &binding,
                StreamFrame::new(
                    StreamTag::Control,
                    serde_json::to_vec(&response).unwrap_or_default(),
                ),
            );
            hub.unregister_bound(client, &binding);
        }
        engine.remote.capture_uploads.clear_client(client);
    }
}
