pub use super::surface_layout::{SurfaceLayout, SurfaceRegion};
use super::surface_trait::Surface;
use super::{PhysicalPx, Rect, SurfaceId};
use tasty_terminal::Terminal;

/// Single terminal instance (Surface type: Terminal).
pub struct TerminalSurface {
    pub id: SurfaceId,
    pub terminal: Terminal,
    /// If lazy init is enabled and terminal hasn't been spawned yet,
    /// this holds the deferred spawn parameters.
    pub deferred_spawn: Option<DeferredSpawn>,
    /// `~/.tasty/scrollback/<id>.bin` 파일 식별자. layout 저장/복원 시 디스크
    /// 영속 scrollback 의 키로 사용된다. `None` 이면 아직 디스크에 dump 한
    /// 적이 없거나 옵션이 꺼져 있는 상태. surface 인스턴스 자체에 살아 있어
    /// 세션 간 stale meta 가 다른 surface 에 상속되는 일이 없다.
    pub scrollback_persist_id: Option<String>,
}

/// Parameters needed to spawn a PTY later (lazy init).
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
    /// 실제로 spawn 되는 순간 새 `TerminalSurface` 의 `scrollback_persist_id`
    /// 필드로 이관된다.
    pub scrollback_persist_id: Option<String>,
}

impl TerminalSurface {
    /// Replace the inner terminal with a new one (same surface ID, same layout position).
    /// The old terminal's PTY process is dropped, which sends SIGHUP to the child.
    pub fn replace_terminal(&mut self, new_terminal: Terminal) {
        self.terminal = new_terminal;
    }
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

    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        self.terminal.get_cwd()
    }

    fn focused_terminal(&self) -> Option<&Terminal> {
        Some(&self.terminal)
    }

    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        Some(&mut self.terminal)
    }

    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        if self.id == surface_id {
            Some(&self.terminal)
        } else {
            None
        }
    }

    fn find_terminal_surface(&self, surface_id: SurfaceId) -> Option<&TerminalSurface> {
        if self.id == surface_id {
            Some(self)
        } else {
            None
        }
    }

    fn find_terminal_mut(&mut self, surface_id: SurfaceId) -> Option<&mut Terminal> {
        if self.id == surface_id {
            Some(&mut self.terminal)
        } else {
            None
        }
    }

    fn resize_all(&mut self, rect: Rect, cell_width: f32, cell_height: f32) {
        let cols = (rect.width / cell_width)
            .floor()
            .max(PhysicalPx(1.0))
            .value() as usize;
        let rows = (rect.height / cell_height)
            .floor()
            .max(PhysicalPx(1.0))
            .value() as usize;
        self.terminal.resize(cols, rows);
    }

    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        out.push(&mut self.terminal);
    }

    fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        f(self.id, &mut self.terminal);
    }

    fn as_terminal_surface(&self) -> Option<&TerminalSurface> {
        Some(self)
    }
    fn as_terminal_surface_mut(&mut self) -> Option<&mut TerminalSurface> {
        Some(self)
    }
    fn take_terminal_surface(self: Box<Self>) -> Option<TerminalSurface> {
        Some(*self)
    }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": self.terminal.cols(),
            "rows": self.terminal.rows(),
        })
    }
}
