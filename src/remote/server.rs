//! attach 점유를 터미널 출력·입력, mesh·문서 조회, 구조 변경과 파일 전송에 연결한다.
//! GUI와 헤드리스 메인 루프가 StreamHub의 수신 결과를 이 모듈에 전달한다.
mod content_queries;

use super::transfer_spool::{Spool, TransferOwner};
use crate::runtime::engine_access::EngineMut;

use crate::runtime::engine_access::EngineRef;
use std::collections::HashMap;

use crate::app::services::AppServices;
use crate::core::attach::{AttachClientId, AttachError};
use crate::model::{AttachSurfaceClass, SurfaceId, WorkspaceId};
use tasty_ipc::stream::{StreamControl, StreamFrame, StreamTag};
use tasty_ipc::stream_hub::{PushResult, StreamHub};

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
            self.workspaces(),
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
        let descriptor = crate::model::SurfaceDescriptor::new(100, kind);
        engine.runtime.surfaces.insert(100, surface);
        let pane = crate::model::Pane::new_with_surface(1, 1, display_name.to_string(), descriptor);
        let mut ws = crate::model::Workspace::new_with_pane(1, "ws".to_string(), pane);
        ws.mirror = true;
        engine.push_mirror_workspace(ws);
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
        let snapshot =
            serde_json::to_vec(&(&descriptor["tree"], &descriptor["surfaces"])).unwrap_or_default();
        self.remote
            .record_structure_sent(workspace_id, client_id, snapshot);

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
