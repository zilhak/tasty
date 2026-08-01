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

    /// 알림 읽음 처리(ADR-0039 Reconsideration Triggers 참고) — 두 번째 clear producer.
    /// 특정 알림을 읽음 처리하고,
    /// 그 알림의 source surface 를 source 로 하는 다른 안읽음 알림이 남아있지 않은
    /// 경우에만 highlight 를 지운다. 같은 surface 의 다른 알림이 아직 안읽음이면
    /// clear 하지 않는다(엣지 케이스 — 무조건 clear 시 오해제 발생).
    pub(crate) fn mark_notification_read(&mut self, id: u64) {
        let source_surface = self
            .notifications
            .all()
            .find(|n| n.id == id)
            .map(|n| n.source_surface);
        self.notifications.mark_read(id);
        if let Some(surface_id) = source_surface {
            if !self.notifications.has_unread_for_surface(surface_id) {
                self.clear_surface_highlight(surface_id);
            }
        }
    }

    /// 모든 알림 읽음 처리(ADR-0039 Reconsideration Triggers 참고). 전부 읽음
    /// 처리되므로 엣지 케이스 없이, 읽음
    /// 처리 전 안읽음이었던 모든 알림의 source surface highlight 를 지운다.
    pub(crate) fn mark_all_notifications_read(&mut self) {
        let unread_surfaces: std::collections::HashSet<u32> = self
            .notifications
            .all()
            .filter(|n| !n.read)
            .map(|n| n.source_surface)
            .collect();
        self.notifications.mark_all_read();
        for surface_id in unread_surfaces {
            self.clear_surface_highlight(surface_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::CoreState;

    fn state() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 같은 source_surface 로 연달아 `add()` 해도 coalesce(기본 500ms 창)되지 않게
    /// coalesce window 를 0 으로 둔 state. 한 surface 에서 온 알림 2건 이상을
    /// 별개 엔트리로 만들어야 하는 엣지 케이스 테스트 전용.
    fn state_no_coalesce() -> CoreState {
        let mut s = state();
        s.notifications = crate::notification::NotificationStore::with_coalesce_ms(0);
        s
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

    /// 개별 읽음 처리 시 그 surface 에 다른 안읽음 알림이 남아있지 않으면
    /// highlight 가 지워진다(ADR-0039 Reconsideration Triggers 참고).
    #[test]
    fn mark_notification_read_clears_highlight_when_no_unread_left() {
        let mut s = state();
        let id = s.notifications.add(1, 100, "t".into(), "b".into()).unwrap();
        s.raise_surface_highlight(100);
        assert!(s.is_surface_highlighted(100));

        s.mark_notification_read(id);

        assert!(!s.is_surface_highlighted(100));
    }

    /// 핵심 엣지 케이스(ADR-0039 Reconsideration Triggers 참고) — 같은 surface 에서
    /// 온 다른 알림이 아직 안읽음이면 하나만 읽음 처리해도 highlight 가 지워지면
    /// 안 된다.
    #[test]
    fn mark_notification_read_keeps_highlight_when_sibling_unread_remains() {
        let mut s = state_no_coalesce();
        let id1 = s
            .notifications
            .add(1, 100, "t1".into(), "b1".into())
            .unwrap();
        let _id2 = s
            .notifications
            .add(1, 100, "t2".into(), "b2".into())
            .unwrap();
        s.raise_surface_highlight(100);

        s.mark_notification_read(id1);

        assert!(
            s.is_surface_highlighted(100),
            "다른 알림(id2)이 아직 안읽음이므로 highlight 가 유지돼야 한다"
        );

        s.mark_notification_read(_id2);
        assert!(
            !s.is_surface_highlighted(100),
            "마지막 안읽음 알림까지 읽음 처리되면 highlight 가 지워져야 한다"
        );
    }

    /// 존재하지 않는 알림 id 를 넘겨도 panic 없이 no-op.
    #[test]
    fn mark_notification_read_unknown_id_is_noop() {
        let mut s = state();
        s.raise_surface_highlight(100);
        s.mark_notification_read(9999);
        assert!(s.is_surface_highlighted(100));
    }

    /// "모두 읽음"은 엣지 케이스 없이 안읽음이었던 모든 surface 의 highlight 를
    /// 지운다.
    #[test]
    fn mark_all_notifications_read_clears_all_unread_surfaces() {
        let mut s = state_no_coalesce();
        s.notifications.add(1, 100, "t1".into(), "b1".into());
        s.notifications.add(1, 100, "t2".into(), "b2".into());
        s.notifications.add(1, 200, "t3".into(), "b3".into());
        s.raise_surface_highlight(100);
        s.raise_surface_highlight(200);

        s.mark_all_notifications_read();

        assert!(!s.is_surface_highlighted(100));
        assert!(!s.is_surface_highlighted(200));
    }

    /// 회귀 방지 — 이미 읽은 알림만 있는 surface 의 highlight 는
    /// `mark_all_notifications_read` 가 건드리지 않아도 원래 그 surface 는 안읽음
    /// 집합에서 제외되므로 clear 대상에 포함되지 않는다(다른 surface 의 highlight 는
    /// 보존).
    #[test]
    fn mark_all_notifications_read_leaves_unrelated_surface_highlight_untouched() {
        let mut s = state();
        let id = s.notifications.add(1, 100, "t".into(), "b".into()).unwrap();
        s.notifications.mark_read(id); // 이미 읽음 처리된 알림
        s.raise_surface_highlight(100); // 알림과 무관한 producer(toast 등)가 건 highlight
        s.raise_surface_highlight(200);
        s.notifications.add(1, 200, "t2".into(), "b2".into());

        s.mark_all_notifications_read();

        assert!(
            s.is_surface_highlighted(100),
            "100 은 안읽음 알림이 없었으므로 clear 대상이 아니다 — 무관 producer 의 highlight 보존"
        );
        assert!(!s.is_surface_highlighted(200));
    }
}
