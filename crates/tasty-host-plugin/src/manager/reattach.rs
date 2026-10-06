//! 원 프로세스가 회수된 뒤 남은 remote surface 를 새 프로세스에 다시 게시한다.
//! disable 뒤 enable, 무응답 재시작, 교체는 모두 새 프로세스를 띄우는 같은 창구를 지난다.
//! mesh surface 는 렌더할 때 `needs_egui_mesh_bootstrap` 이 새 세대에 다시 보낸다.

use serde_json::{Value, json};

use super::{PendingRequestKind, PluginManager, RemotePublication, RemoteSurfaceEntry};
use crate::host_cmd::SurfaceHandles;
use crate::protocol;

/// remote surface 를 처음 게시한 요청.
pub(crate) struct SurfaceOrigin {
    method: &'static str,
    params: Value,
}

impl SurfaceOrigin {
    pub(crate) fn create(params: Value) -> Self {
        Self {
            method: protocol::METHOD_SURFACE_CREATE,
            params,
        }
    }

    pub(crate) fn restore(params: Value) -> Self {
        Self {
            method: protocol::METHOD_SURFACE_RESTORE,
            params,
        }
    }

    fn pending(&self, surface_id: u32, handles: &SurfaceHandles) -> PendingRequestKind {
        let binding = handles.binding();
        if self.method == protocol::METHOD_SURFACE_RESTORE {
            PendingRequestKind::SurfaceRestore {
                surface_id,
                binding,
            }
        } else {
            PendingRequestKind::SurfaceCreate {
                surface_id,
                binding,
            }
        }
    }
}

static SNAPSHOT_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl PluginManager {
    /// 생성·복원 요청을 지금 프로세스에 보내고 등록한다. 프로세스가 없으면 보내지 않은
    /// 채로 등록하고, 다음에 그 플러그인이 뜰 때 [`Self::reattach_orphan_surfaces`] 가 보낸다.
    pub(super) fn publish_remote_surface(
        &mut self,
        surface_id: u32,
        plugin_id: String,
        handles: SurfaceHandles,
        origin: SurfaceOrigin,
    ) {
        let publication = self.send_owned_surface_request(
            &plugin_id,
            origin.method,
            origin.params.clone(),
            origin.pending(surface_id, &handles),
        );
        self.surfaces.insert(
            surface_id,
            RemoteSurfaceEntry {
                plugin_id,
                handles,
                publication,
                origin,
            },
        );
    }

    /// 새로 뜬 `plugin_id` 프로세스에 게시되지 않은 그 플러그인의 surface 를 다시 보낸다.
    /// 마지막 snapshot 이 있으면 그것으로 복원하고, 없으면 원 요청을 그대로 보낸다.
    pub(super) fn reattach_orphan_surfaces(&mut self, plugin_id: &str) {
        let Some(current) = self
            .processes
            .get(plugin_id)
            .map(|process| process.reply_binding())
        else {
            return;
        };
        let orphans: Vec<u32> = self
            .surfaces
            .iter()
            .filter(|(_, entry)| {
                entry.plugin_id == plugin_id
                    && match &entry.publication {
                        RemotePublication::Sent(binding) => !binding.ptr_eq(&current),
                        RemotePublication::NeverSent => true,
                    }
            })
            .map(|(id, _)| *id)
            .collect();
        for surface_id in orphans {
            let Some(entry) = self.surfaces.get(&surface_id) else {
                continue;
            };
            let snapshot = tasty_utils::poison::recover_mutex(
                entry.handles.snapshot_cache.lock(),
                "remote surface snapshot cache",
                &SNAPSHOT_POISONED,
            )
            .clone();
            let (method, params, pending) = match snapshot {
                Some(data) => (
                    protocol::METHOD_SURFACE_RESTORE,
                    json!({
                        "surface_id": surface_id,
                        "kind": entry.origin.params.get("kind").cloned().unwrap_or(Value::Null),
                        "data": data,
                    }),
                    PendingRequestKind::SurfaceRestore {
                        surface_id,
                        binding: entry.handles.binding(),
                    },
                ),
                None => (
                    entry.origin.method,
                    entry.origin.params.clone(),
                    entry.origin.pending(surface_id, &entry.handles),
                ),
            };
            let publication = self.send_owned_surface_request(plugin_id, method, params, pending);
            let sent = matches!(publication, RemotePublication::Sent(_));
            if let Some(entry) = self.surfaces.get_mut(&surface_id) {
                entry.publication = publication;
            }
            if sent {
                tracing::info!(plugin_id, surface_id, method, "orphan surface reattached");
            } else {
                tracing::warn!(
                    plugin_id,
                    surface_id,
                    "orphan surface could not be sent to the new process; retrying at next start"
                );
            }
        }
    }
}
