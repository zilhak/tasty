pub use super::surface_layout::{SurfaceLayout, SurfaceRegion};
use super::surface_trait::Surface;
use super::{PhysicalRect, SurfaceId};
use tasty_terminal::Terminal;

/// Surface 트리의 *terminal kind* placeholder (D.3.E.4.d 이후).
///
/// **D.3.E.4** — Terminal/PTY 데이터는 `Core::CoreState::terminals`
/// (`TerminalStore`) 가 owner. 본 struct 는 *Surface 트리에서 terminal kind 인
/// leaf 라는 사실만 표시* 한다. PTY/grid 접근은 `engine.terminals.get(id)` 로
/// 직접 조회.
///
/// `terminal: Option<Terminal>` 은 cutover 진행 중의 *과도기* 형태 —
/// 새로 생성된 TerminalSurface 는 항상 `terminal = None` (Terminal 은 store 가
/// owner). E.4.f 에서 본 필드 자체를 제거하면 Surface 가 *pure id-marker* 가 된다.
pub struct TerminalSurface {
    pub id: SurfaceId,
    /// **D.3.E.4 과도기** — 항상 `None` (생성 시 Terminal 은 store 로 이동).
    /// E.4.f 에서 제거 예정. 필드 잔존 이유: Surface trait 의 일부 default-impl
    /// 메서드 (`focused_terminal`, `find_terminal` 등) 가 본 필드를 통한 *legacy
    /// fallback* 으로 동작 → caller refactor 가 끝나야 안전 제거.
    pub terminal: Option<Terminal>,
    /// **D.3.E.4 과도기** — 항상 `None`. deferred 표현은 `EmptySurface { deferred_spawn }`
    /// 가 주 경로 (`engine.terminals.defer(...)` 는 별 책임). E.4.f 에서 제거.
    pub deferred_spawn: Option<DeferredSpawn>,
    /// **D.3.E.4 과도기** — store 로 이전. `engine.terminals.scrollback_persist_id(id)`
    /// 가 source of truth. 본 필드는 *legacy 호환* 용 mirror — E.4.f 에서 제거.
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
    /// **D.3.E.4** — id 만 보유한 marker 생성. caller 가 사전에
    /// `engine.terminals.insert(id, terminal)` 로 Terminal 을 store 에 등록해야
    /// 한다. helper `CoreState::install_terminal_surface` 권장.
    pub fn marker(id: SurfaceId) -> Self {
        Self {
            id,
            terminal: None,
            deferred_spawn: None,
            scrollback_persist_id: None,
        }
    }

    /// Replace the inner terminal with a new one (same surface ID, same layout position).
    /// The old terminal's PTY process is dropped, which sends SIGHUP to the child.
    ///
    /// **D.3.E.4** — *deprecated*. caller 는 `engine.terminals.replace(id, t)` 사용.
    /// 본 메서드는 legacy compat — `self.terminal: Option<Terminal>` 에 새 값 보관.
    #[allow(dead_code)]
    pub fn replace_terminal(&mut self, new_terminal: Terminal) {
        self.terminal = Some(new_terminal);
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
        // **D.3.E.4** — Terminal 이 store 로 이동. 본 메서드는 legacy fallback
        // 으로만 사용. E.4.e 에서 caller 가 `engine.terminals.get(id).get_cwd()`
        // 로 직접 조회하도록 정리하면 본 분기 제거 가능.
        self.terminal.as_ref().and_then(|t| t.get_cwd())
    }

    fn focused_terminal(&self) -> Option<&Terminal> {
        self.terminal.as_ref()
    }

    fn focused_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.terminal.as_mut()
    }

    fn find_terminal(&self, surface_id: SurfaceId) -> Option<&Terminal> {
        if self.id == surface_id {
            self.terminal.as_ref()
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
            self.terminal.as_mut()
        } else {
            None
        }
    }

    fn resize_all(&mut self, _rect: PhysicalRect, _cell_width: f32, _cell_height: f32) {
        // **D.3.E.4** — Terminal 이 store 로 이동. resize 는 caller 가
        // `engine.terminals.iter_mut()` 로 직접 처리. 본 메서드는 no-op.
    }

    fn collect_terminals_mut<'a>(&'a mut self, out: &mut Vec<&'a mut Terminal>) {
        if let Some(t) = self.terminal.as_mut() {
            out.push(t);
        }
    }

    fn for_each_terminal_mut(&mut self, f: &mut dyn FnMut(SurfaceId, &mut Terminal)) {
        if let Some(t) = self.terminal.as_mut() {
            f(self.id, t);
        }
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
        // **D.3.E.4** — cols/rows 는 *legacy 시점에는* terminal 직접, *cutover 후*
        // 에는 caller 가 engine.terminals.get(id) 로 조회해 채워야 함.
        // 본 메서드는 트리 JSON 의 *Surface ID + type* 만 제공.
        let (cols, rows) = self
            .terminal
            .as_ref()
            .map(|t| (t.cols(), t.rows()))
            .unwrap_or((0, 0));
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": cols,
            "rows": rows,
        })
    }
}
