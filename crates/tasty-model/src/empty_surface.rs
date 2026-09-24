use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;
use super::terminal_surface::{Deferred, DeferredPlugin, DeferredSpawn};

/// 비활성 빈 surface 또는 복원 대기용 placeholder. Deferred::Terminal은 PTY 생성을,
/// Deferred::Plugin은 kind 등록 뒤 복원을 기다린다.
pub struct EmptySurface {
    pub id: SurfaceId,
    /// 교체할 surface의 복원 정보. Terminal과 Plugin 중 하나만 지정할 수 있다.
    pub deferred: Option<Deferred>,
    /// PTY 생성의 연속 실패 횟수. 상한 뒤에는 반복 재시도를 멈춘다.
    pub spawn_attempts: u32,
    /// 호스트가 carry 한 시작 cwd. fresh empty 면 None — Surface cwd invariant
    /// (`docs/design/policies/cwd.md#surface-cwd-invariant`) 에 따라 다음 변환 시 후보로 사용.
    pub cwd: Option<PathBuf>,
}

impl EmptySurface {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            deferred: None,
            spawn_attempts: 0,
            cwd: None,
        }
    }

    /// Deferred PTY spawn 파라미터를 가진 terminal placeholder를 생성.
    pub fn new_deferred(id: SurfaceId, spawn: DeferredSpawn) -> Self {
        Self {
            id,
            deferred: Some(Deferred::Terminal(spawn)),
            spawn_attempts: 0,
            cwd: None,
        }
    }

    /// plugin kind registry 등록을 기다리는 non-terminal placeholder를 생성.
    pub fn new_deferred_plugin(id: SurfaceId, plugin: DeferredPlugin) -> Self {
        Self {
            id,
            deferred: Some(Deferred::Plugin(plugin)),
            spawn_attempts: 0,
            cwd: None,
        }
    }

    /// 호스트가 carry 한 cwd 를 부여 (builder).
    pub fn with_cwd(mut self, cwd: Option<PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }

    /// 실제화 대기(terminal 이든 plugin 이든) 상태인지 여부.
    pub fn is_deferred(&self) -> bool {
        self.deferred.is_some()
    }

    /// terminal 자리표시자면 그 spawn 파라미터. plugin/비-deferred 면 None.
    pub fn deferred_spawn(&self) -> Option<&DeferredSpawn> {
        match &self.deferred {
            Some(Deferred::Terminal(spawn)) => Some(spawn),
            _ => None,
        }
    }

    /// terminal 자리표시자의 spawn 파라미터를 가변 참조로. plugin/비-deferred 면 None.
    pub fn deferred_spawn_mut(&mut self) -> Option<&mut DeferredSpawn> {
        match &mut self.deferred {
            Some(Deferred::Terminal(spawn)) => Some(spawn),
            _ => None,
        }
    }

    /// plugin 자리표시자면 그 kind/snapshot. terminal/비-deferred 면 None.
    pub fn deferred_plugin(&self) -> Option<&DeferredPlugin> {
        match &self.deferred {
            Some(Deferred::Plugin(p)) => Some(p),
            _ => None,
        }
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
            Some(Deferred::Terminal(spawn)) => {
                // Deferred terminal placeholder — 외부에 Terminal로 보이고 pty_ready: false.
                serde_json::json!({
                    "type": "Terminal",
                    "kind": "terminal",
                    "id": self.id,
                    "cols": spawn.cols,
                    "rows": spawn.rows,
                    "pty_ready": false,
                })
            }
            Some(Deferred::Plugin(p)) => {
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
