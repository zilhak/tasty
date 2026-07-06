//! Surface "highlight"(주의 환기) 상태 조작/조회. `highlighted_surfaces` 는
//! producer 중립 공유 primitive — toast 알림, completion IPC/CLI 등 여러 producer 가
//! `raise_surface_highlight` 로 발동하고, surface 가 실제 렌더 시점 포커스를 얻으면
//! (`gpu.rs`) `clear_surface_highlight` 로 해제된다. 세 소비처(테두리·탭 제목·워크스페이스
//! 개수 배지)가 이 상태를 읽는다. `state/busy.rs` 의 조회 헬퍼 형태를 1:1 미러한다.

use super::CoreState;

impl CoreState {
    /// Mark a surface as highlighted (attention needed). Called by any producer
    /// (toast, completion, …). `surface_id == 0` (=미지정) 은 무시한다.
    pub fn raise_surface_highlight(&mut self, surface_id: u32) {
        if surface_id != 0 {
            self.highlighted_surfaces.insert(surface_id);
        }
    }

    /// Clear the highlight for a surface (e.g. when it gains focus).
    pub fn clear_surface_highlight(&mut self, surface_id: u32) {
        self.highlighted_surfaces.remove(&surface_id);
    }

    /// Whether the given surface is currently highlighted.
    pub fn is_surface_highlighted(&self, surface_id: u32) -> bool {
        self.highlighted_surfaces.contains(&surface_id)
    }

    /// Whether any surface in the given list is highlighted.
    pub fn has_highlight(&self, surface_ids: &[u32]) -> bool {
        surface_ids
            .iter()
            .any(|sid| self.highlighted_surfaces.contains(sid))
    }

    /// Number of highlighted surfaces among the given list.
    pub fn highlight_count(&self, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.highlighted_surfaces.contains(sid))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use crate::core::CoreState;

    fn state() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn raise_and_query() {
        let mut s = state();
        assert!(!s.is_surface_highlighted(7));
        s.raise_surface_highlight(7);
        assert!(s.is_surface_highlighted(7));
        assert!(s.has_highlight(&[7]));
        assert!(!s.has_highlight(&[8, 9]));
    }

    #[test]
    fn raise_ignores_zero() {
        let mut s = state();
        s.raise_surface_highlight(0);
        assert!(!s.is_surface_highlighted(0));
        assert_eq!(s.highlight_count(&[0]), 0);
    }

    #[test]
    fn clear_removes() {
        let mut s = state();
        s.raise_surface_highlight(3);
        s.clear_surface_highlight(3);
        assert!(!s.is_surface_highlighted(3));
    }

    #[test]
    fn count_over_list() {
        let mut s = state();
        s.raise_surface_highlight(1);
        s.raise_surface_highlight(2);
        s.raise_surface_highlight(5);
        assert_eq!(s.highlight_count(&[1, 2, 3, 4, 5]), 3);
        assert_eq!(s.highlight_count(&[3, 4]), 0);
    }
}
