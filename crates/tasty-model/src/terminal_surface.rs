use super::SurfaceId;
pub use super::surface_layout::{SurfaceLayout, SurfaceRegion};
use super::surface_trait::Surface;

/// Surface 트리의 *terminal kind* placeholder.
///
/// PTY/Terminal/scrollback_persist 데이터는 모두 `CoreState::terminals`
/// (`TerminalStore`) 가 owner. 본 struct 는 *Surface 트리에서 terminal kind 인
/// leaf 라는 사실만 표시* 하는 id-only marker.
pub struct TerminalSurface {
    pub id: SurfaceId,
}

/// Parameters needed to spawn a PTY later (lazy init).
///
/// `EmptySurface { deferred_spawn: Some(..) }` placeholder 의 본체. PTY 가 spawn
/// 되는 시점에 TerminalSurface marker 로 교체된다.
#[derive(Clone)]
pub struct DeferredSpawn {
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    pub cols: usize,
    pub rows: usize,
    pub waker: tasty_terminal::Waker,
    pub working_dir: Option<std::path::PathBuf>,
    /// PTY spawn 직후 즉시 send_key 로 주입할 명령. 줄바꿈은 호출자가 붙이지 않고
    /// `ensure_initialized` 가 `\r` 를 자동으로 덧붙여 submit 한다. TUI 세션 재개용
    /// (예: `claude -r <uuid>`).
    pub restore_command: Option<String>,
    /// 복원 시 layout.json 의 scrollback_ref 를 그대로 들고 있다가, PTY 가
    /// 실제로 spawn 되는 순간 `TerminalStore::set_scrollback_persist_id` 로 이관된다.
    pub scrollback_persist_id: Option<String>,
}

impl Surface for TerminalSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "terminal"
    }

    fn type_name(&self) -> &'static str {
        "Terminal"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// Terminal 의 cwd 는 `engine.terminals.get(id).get_cwd()` 로 store 경유 —
    /// trait 는 None 반환. caller (cwd_from_surface) 가 분기 처리. Surface cwd
    /// invariant — `docs/architecture/invariants/surface-cwd.md`.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn to_tree_json(&self) -> serde_json::Value {
        // cols/rows 는 caller 가 engine.terminals.get(id) 로 enrichment.
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": 0,
            "rows": 0,
        })
    }
}
