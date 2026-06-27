//! Surface 의 "busy" 상태 폴링/조회. `refresh_busy_surfaces` 는 매 tick 에 호출되어
//! 각 PTY 의 foreground process 를 비교해 `busy_surfaces` 를 갱신한다.

use super::CoreState;

impl CoreState {
    /// Recompute `busy_surfaces` by polling every PTY's foreground
    /// process. Returns true if the set changed (caller should redraw).
    pub fn refresh_busy_surfaces(&mut self) -> bool {
        // Resolve every live surface's foreground program from a single system
        // snapshot. On Windows the foreground lookup snapshots all processes
        // (≈6ms with a few hundred); doing it per surface put O(surfaces ×
        // processes) on the main thread every 1Hz tick (≈370ms at 60 live
        // surfaces), stalling workspace switches and input. One snapshot per
        // tick collapses that to O(processes + surfaces).
        let mut sids: Vec<u32> = Vec::new();
        let mut shell_pids: Vec<u32> = Vec::new();
        for (sid, terminal) in self.terminals.iter() {
            if let Some(pid) = terminal.process_id() {
                sids.push(sid);
                shell_pids.push(pid);
            }
        }
        let foregrounds = tasty_terminal::foreground_process::resolve_foreground_many(&shell_pids);

        let mut busy: std::collections::HashSet<u32> = std::collections::HashSet::new();
        // 같은 1Hz foreground resolve 결과를 재사용해 마우스 캡처 블랙리스트 매칭도
        // 함께 계산한다(별도 프로세스 스냅샷 없음). 빈 블랙리스트면 매칭 헬퍼가
        // 즉시 false 라 비용 무시 가능.
        let mut mouse_capture_disabled: std::collections::HashSet<u32> =
            std::collections::HashSet::new();
        for ((&sid, &shell_pid), fg) in sids.iter().zip(shell_pids.iter()).zip(foregrounds.iter()) {
            let Some(terminal) = self.terminals.get(sid) else {
                continue;
            };
            if terminal.busy_with_foreground(shell_pid, fg.as_ref()) {
                busy.insert(sid);
            }
            if let Some(f) = fg.as_ref()
                && self.settings.general.mouse_capture_disabled_for(&f.name)
            {
                mouse_capture_disabled.insert(sid);
            }
        }
        // 블랙리스트 캐시는 다음 입력 시 조회로 반영되므로 redraw 신호(changed)에는
        // 포함하지 않는다 — busy set 변화만으로 dirty 를 판정한다.
        self.mouse_capture_disabled_surfaces = mouse_capture_disabled;
        let changed = self.busy_surfaces != busy;
        self.busy_surfaces = busy;
        changed
    }

    /// Whether the given surface's foreground process matches the mouse-capture
    /// blacklist (cached from the last `refresh_busy_surfaces` poll). When true,
    /// the host treats the surface's click/drag tracking as `None` (local
    /// select / context menu); the wheel is unaffected.
    pub fn is_surface_mouse_capture_disabled(&self, surface_id: u32) -> bool {
        self.mouse_capture_disabled_surfaces.contains(&surface_id)
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
