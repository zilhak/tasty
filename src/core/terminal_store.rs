//! `TerminalStore` — Surface 트리와 분리된 Terminal/PTY 데이터 owner.
//!
//! Phase D D.3.E.4 — `TerminalSurface { id, terminal: Terminal, deferred_spawn,
//! scrollback_persist_id }` 의 *PTY/Terminal 데이터* 를 본 store 로 이전한다.
//! Surface 트리는 *render layer + id 참조* 로 좁혀지고, Terminal 의 lifecycle 은
//! store 가 책임진다.
//!
//! 마이그레이션 단계 (substep 분할):
//! - **E.4.a (현재)**: struct 신설 + CoreState 필드 추가. 호출처 0.
//! - **E.4.b**: dual-write — Terminal 생성 콜사이트에서 store 에도 insert.
//! - **E.4.c**: read API switch — `find_terminal_by_id*`/`process_*` 가 store 사용.
//! - **E.4.d**: mutate API switch — `replace`/`ensure_initialized`/scrollback inject /
//!   busy_surfaces 이전 + Surface close 시 store.remove cascade.
//! - **E.4.e**: Surface trait 단순화 — terminal 관련 11→8 메서드.
//! - **E.4.f**: cutover — `TerminalSurface.terminal` 필드 + dual-write 폐기.

use std::collections::{HashMap, HashSet};

use tasty_terminal::{ScrollbackLine, Terminal, TerminalEvent};

use crate::model::{DeferredSpawn, SurfaceId};

/// PTY/Terminal 데이터 owner. surface_id ↔ terminal 매핑 (1:1).
///
/// Surface 트리 (`Workspace → Pane → Tab → SurfaceLayout`) 의 `TerminalSurface`
/// Leaf 는 *id 만* 들고, 실제 Terminal 인스턴스는 본 store 가 보관한다.
/// CoreState 안에 위치 (옵션 B — `parked_states` 와 자연 호환, multi-engine
/// 구조 유지).
#[derive(Default)]
pub(crate) struct TerminalStore {
    /// spawn 완료된 Terminal 들. key = surface_id.
    terminals: HashMap<SurfaceId, Terminal>,

    /// Lazy spawn 대기 중인 placeholder 의 spawn parameter. PTY 가 실제로
    /// spawn 되는 시점에 take 되어 Terminal 로 변환된다.
    deferred: HashMap<SurfaceId, DeferredSpawn>,

    /// `~/.tasty/scrollback/<id>.bin` 디스크 영속 scrollback 의 키.
    /// layout 저장/복원 시 사용. surface 별로 *Terminal 과는 별도 lifecycle* —
    /// 생성 즉시에는 None, 첫 dump 후 Some.
    scrollback_persist_ids: HashMap<SurfaceId, String>,

    /// 복원된 scrollback line 들의 inject 대기 큐. PTY 가 spawn 된 직후
    /// `inject_scrollback` 으로 소비.
    pending_scrollback_inject: HashMap<SurfaceId, Vec<ScrollbackLine>>,

    /// foreground process != shell 판정 캐시. BusyPoll 로 갱신. surface 닫힘 시
    /// store 와 함께 entry 제거 (lifecycle 동기 — TODO §4 Q4 결정).
    busy_surfaces: HashSet<SurfaceId>,
}

impl TerminalStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    // ── Terminal lifecycle ──────────────────────────────────────────

    /// Terminal 등록. 같은 id 가 이미 있으면 *overwrite* (옛 Terminal 의 PTY 는
    /// drop → SIGHUP). respawn 경로에서는 `replace` 가 더 명확하므로 그쪽 사용.
    pub(crate) fn insert(&mut self, id: SurfaceId, terminal: Terminal) {
        self.terminals.insert(id, terminal);
    }

    /// Terminal 제거. 반환된 Terminal 이 drop 되며 PTY SIGHUP. surface 닫힘 시
    /// caller 가 호출. 부속 데이터 (deferred / scrollback_persist_ids /
    /// pending_scrollback_inject / busy_surfaces) 도 함께 제거.
    pub(crate) fn remove(&mut self, id: SurfaceId) -> Option<Terminal> {
        self.deferred.remove(&id);
        self.scrollback_persist_ids.remove(&id);
        self.pending_scrollback_inject.remove(&id);
        self.busy_surfaces.remove(&id);
        self.terminals.remove(&id)
    }

    /// PTY 교체 (respawn). 옛 Terminal 이 drop 되며 SIGHUP. 같은 surface_id 가
    /// 유지되어 layout 의 leaf 위치는 그대로. 옛 메서드: `replace_terminal_by_id`.
    pub(crate) fn replace(&mut self, id: SurfaceId, new_terminal: Terminal) -> Option<Terminal> {
        self.terminals.insert(id, new_terminal)
    }

    pub(crate) fn get(&self, id: SurfaceId) -> Option<&Terminal> {
        self.terminals.get(&id)
    }

    pub(crate) fn get_mut(&mut self, id: SurfaceId) -> Option<&mut Terminal> {
        self.terminals.get_mut(&id)
    }

    pub(crate) fn contains(&self, id: SurfaceId) -> bool {
        self.terminals.contains_key(&id)
    }

    // ── Iteration ───────────────────────────────────────────────────

    pub(crate) fn iter(&self) -> impl Iterator<Item = (SurfaceId, &Terminal)> {
        self.terminals.iter().map(|(&id, t)| (id, t))
    }

    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (SurfaceId, &mut Terminal)> {
        self.terminals.iter_mut().map(|(&id, t)| (id, t))
    }

    // ── Deferred spawn ──────────────────────────────────────────────

    pub(crate) fn defer(&mut self, id: SurfaceId, spawn: DeferredSpawn) {
        self.deferred.insert(id, spawn);
    }

    pub(crate) fn take_deferred(&mut self, id: SurfaceId) -> Option<DeferredSpawn> {
        self.deferred.remove(&id)
    }

    pub(crate) fn is_deferred(&self, id: SurfaceId) -> bool {
        self.deferred.contains_key(&id)
    }

    // ── Scrollback persist id ──────────────────────────────────────

    pub(crate) fn scrollback_persist_id(&self, id: SurfaceId) -> Option<&str> {
        self.scrollback_persist_ids.get(&id).map(String::as_str)
    }

    pub(crate) fn set_scrollback_persist_id(&mut self, id: SurfaceId, persist_id: String) {
        self.scrollback_persist_ids.insert(id, persist_id);
    }

    // ── Pending scrollback inject ───────────────────────────────────

    pub(crate) fn enqueue_scrollback_inject(&mut self, id: SurfaceId, lines: Vec<ScrollbackLine>) {
        self.pending_scrollback_inject.insert(id, lines);
    }

    pub(crate) fn take_pending_scrollback_inject(
        &mut self,
        id: SurfaceId,
    ) -> Option<Vec<ScrollbackLine>> {
        self.pending_scrollback_inject.remove(&id)
    }

    // ── Busy surfaces (foreground process != shell) ─────────────────

    pub(crate) fn busy_surfaces(&self) -> &HashSet<SurfaceId> {
        &self.busy_surfaces
    }

    pub(crate) fn set_busy(&mut self, id: SurfaceId, busy: bool) {
        if busy {
            self.busy_surfaces.insert(id);
        } else {
            self.busy_surfaces.remove(&id);
        }
    }

    pub(crate) fn is_busy(&self, id: SurfaceId) -> bool {
        self.busy_surfaces.contains(&id)
    }

    // ── PTY operations (bulk) ───────────────────────────────────────

    /// 모든 terminal 의 PTY 출력 drain. 어느 하나라도 데이터 drain 했으면 true.
    /// 옛 `CoreState::process_all` 의 store-rooted 변형.
    pub(crate) fn process_all(&mut self) -> bool {
        let mut any = false;
        for t in self.terminals.values_mut() {
            if t.process() {
                any = true;
            }
        }
        any
    }

    /// 특정 surface 의 PTY 출력 drain. 옛 `CoreState::process_surface`.
    pub(crate) fn process_surface(&mut self, id: SurfaceId) -> bool {
        if let Some(t) = self.terminals.get_mut(&id) {
            t.process()
        } else {
            false
        }
    }

    /// 모든 terminal 의 TerminalEvent 수집.
    pub(crate) fn collect_events(&mut self) -> Vec<(SurfaceId, TerminalEvent)> {
        let mut out = Vec::new();
        for (&id, t) in self.terminals.iter_mut() {
            for ev in t.take_events() {
                out.push((id, ev));
            }
        }
        out
    }

    /// PTY resize throttled flush. 한 곳이라도 pending 이면 true.
    pub(crate) fn flush_pty_resizes(&mut self) -> bool {
        let mut any_pending = false;
        for t in self.terminals.values_mut() {
            t.flush_pty_resize();
            if t.has_pending_pty_resize() {
                any_pending = true;
            }
        }
        any_pending
    }
}
