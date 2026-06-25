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
    /// 연속 PTY spawn 실패 횟수. reify 가 매 프레임(~60fps) 재시도하므로, 복원된
    /// layout 의 shell 바이너리가 영구히 없는 등 영구 실패 시 spawn 폭주를 막기 위해
    /// `Tab::ensure_initialized` 가 실패할 때마다 +1 하고, 상한에 도달하면 재시도를
    /// 멈춘다 (transient 실패는 상한 전에 성공해 0 으로 의미를 잃는다).
    pub spawn_attempts: u32,
    /// 호스트가 carry 한 시작 cwd. fresh empty 면 None — Surface cwd invariant
    /// (`docs/architecture/invariants/surface-cwd.md`) 에 따라 다음 변환 시 후보로 사용.
    pub cwd: Option<PathBuf>,
}

impl EmptySurface {
    pub fn new(id: SurfaceId) -> Self {
        Self {
            id,
            deferred_spawn: None,
            spawn_attempts: 0,
            cwd: None,
        }
    }

    /// Deferred PTY spawn 파라미터를 가진 placeholder를 생성.
    pub fn new_deferred(id: SurfaceId, spawn: DeferredSpawn) -> Self {
        Self {
            id,
            deferred_spawn: Some(spawn),
            spawn_attempts: 0,
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
