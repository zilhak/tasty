use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;
use super::terminal_surface::DeferredSpawn;

/// 빈 surface placeholder. 일반적으로는 convert 버튼만 보여주는 비활성 패널이지만,
/// `deferred_spawn`이 Some일 때는 layout 복원 직후 PTY가 아직 spawn되지 않은
/// 터미널 자리를 차지하는 placeholder다.
pub struct EmptySurface {
    pub id: SurfaceId,
    /// PTY lazy spawn 파라미터. Some이면 이 surface는 활성화될 때 TerminalSurface로
    /// 교체될 deferred terminal 자리표시자다.
    pub deferred_spawn: Option<DeferredSpawn>,
    /// 호스트가 carry 한 시작 cwd. fresh empty 면 None — Surface cwd invariant
    /// (`docs/architecture/invariants/surface-cwd.md`) 에 따라 다음 변환 시 후보로 사용.
    pub cwd: Option<PathBuf>,
}

impl EmptySurface {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            deferred_spawn: None,
            cwd: None,
        }
    }

    /// Deferred PTY spawn 파라미터를 가진 placeholder를 생성.
    pub fn new_deferred(id: SurfaceId, spawn: DeferredSpawn) -> Self {
        Self {
            id,
            deferred_spawn: Some(spawn),
            cwd: None,
        }
    }

    /// 호스트가 carry 한 cwd 를 부여 (builder).
    pub fn with_cwd(mut self, cwd: Option<PathBuf>) -> Self {
        self.cwd = cwd;
        self
    }

    /// `deferred_spawn`이 있는지 여부.
    pub fn is_deferred(&self) -> bool {
        self.deferred_spawn.is_some()
    }

    /// Deferred spawn을 꺼내 소비. PTY spawn 직후 호출.
    pub fn take_deferred_spawn(&mut self) -> Option<DeferredSpawn> {
        self.deferred_spawn.take()
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
        if let Some(spawn) = &self.deferred_spawn {
            // Deferred terminal placeholder — 외부에 Terminal로 보이고 pty_ready: false.
            serde_json::json!({
                "type": "Terminal",
                "kind": "terminal",
                "id": self.id,
                "cols": spawn.cols,
                "rows": spawn.rows,
                "pty_ready": false,
            })
        } else {
            serde_json::json!({
                "type": "Empty",
                "kind": "empty",
                "id": self.id,
            })
        }
    }
}
