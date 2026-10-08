use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;
use super::terminal_surface::DeferredPlugin;

/// 비활성 빈 surface 또는 plugin kind 등록 뒤 복원을 기다리는 placeholder.
pub struct EmptySurface {
    pub id: SurfaceId,
    /// kind 등록을 기다리는 plugin surface의 복원 정보. 없으면 비활성 빈 surface다.
    pub deferred: Option<DeferredPlugin>,
    /// 호스트가 carry 한 시작 cwd. fresh empty 면 None — Surface cwd invariant
    /// (`docs/design/policies/cwd.md#surface-cwd-invariant`) 에 따라 다음 변환 시 후보로 사용.
    pub cwd: Option<PathBuf>,
}

impl EmptySurface {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            deferred: None,
            cwd: None,
        }
    }

    /// plugin kind registry 등록을 기다리는 non-terminal placeholder를 생성.
    pub fn new_deferred_plugin(id: SurfaceId, plugin: DeferredPlugin) -> Self {
        Self {
            id,
            deferred: Some(plugin),
            cwd: None,
        }
    }

    /// 호스트가 carry 한 cwd 를 부여 (builder).
    pub fn with_cwd(mut self, cwd: Option<PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }

    /// plugin 복원 대기 상태인지 여부.
    pub fn is_deferred(&self) -> bool {
        self.deferred.is_some()
    }

    /// plugin 자리표시자면 그 kind/snapshot. 비활성 빈 surface면 None.
    pub fn deferred_plugin(&self) -> Option<&DeferredPlugin> {
        self.deferred.as_ref()
    }
}

impl Surface for EmptySurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "empty"
    }
    fn type_name(&self) -> &'static str {
        "Empty"
    }
    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        self.cwd.clone()
    }

    fn to_tree_json(&self) -> serde_json::Value {
        match &self.deferred {
            Some(p) => {
                // 원래 kind는 유지하되 아직 사용할 수 없음을 ready:false로 알린다.
                serde_json::json!({
                    "type": "Pending",
                    "kind": p.kind,
                    "id": self.id,
                    "ready": false,
                    "pending_reason": "plugin_not_loaded",
                })
            }
            None => serde_json::json!({
                "type": "Empty",
                "kind": "empty",
                "id": self.id,
            }),
        }
    }
}
