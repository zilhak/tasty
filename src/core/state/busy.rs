//! Surface 의 "busy" 상태 폴링/조회. `refresh_busy_surfaces` 는 매 tick 에 호출되어
//! 각 PTY 의 foreground process 를 비교해 `busy_surfaces` 를 갱신한다.

use super::CoreState;

impl CoreState {
    /// Recompute `busy_surfaces` by polling every PTY's foreground
    /// process. Returns true if the set changed (caller should redraw).
    ///
    /// **D.3.E.4.f** — store iter 로 cutover. busy 의 source of truth 는
    /// `self.busy_surfaces` (frontend 용) 와 `self.terminals.busy_surfaces`
    /// (store 내부, 향후 단일화) 의 dual — frontend caller (`is_surface_busy`,
    /// `any_busy`, `busy_count`) 가 self.busy_surfaces 만 사용하므로 본 메서드가
    /// owner.
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        let mut busy: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (sid, terminal) in self.terminals.iter() {
            if terminal.is_busy() {
                busy.insert(sid);
            }
        }
        let changed = self.busy_surfaces != busy;
        for sid in busy.iter() {
            self.terminals.set_busy(*sid, true);
        }
        for sid in self.busy_surfaces.iter() {
            if !busy.contains(sid) {
                self.terminals.set_busy(*sid, false);
            }
        }
        self.busy_surfaces = busy;
        changed
    }

    /// Whether the given surface is currently running a non-shell foreground
    /// program (cached value from the last `refresh_busy_surfaces` poll).
    pub fn is_surface_busy(&self, surface_id: u32) -> bool {
        self.busy_surfaces.contains(&surface_id)
    }

    /// Whether any surface in the given list is busy.
    pub fn any_busy(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|sid| self.busy_surfaces.contains(sid))
    }

    /// Number of busy surfaces among the given list.
    pub fn busy_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.busy_surfaces.contains(sid))
            .count()
    }
}
