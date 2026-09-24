//! 부모 surface에 연결된 soft 점유. 표시용이며 로컬 쓰기를 막지 않는다.
//! 임의 점유용 IPC 메서드 없이 child-terminal 처리에서 내부 호출한다.

use super::CoreState;
use crate::core::attach::OccupancyError;

impl CoreState {
    /// 같은 parent는 라벨을 갱신하고 다른 점유자가 있으면 오류다. 대상 ID는 호출자가 지정한다.
    pub fn occupy_soft(
        &mut self,
        surface_id: u32,
        parent: u32,
        label: Option<String>,
    ) -> Result<(), OccupancyError> {
        self.attach.acquire_soft(surface_id, parent, label)
    }

    /// 기록된 parent만 해제할 수 있다. 점유 부재와 parent 불일치는 오류로 구분한다.
    pub fn release_soft_occupancy(
        &mut self,
        surface_id: u32,
        parent: u32,
    ) -> Result<(), OccupancyError> {
        self.attach.release_soft(surface_id, parent)
    }

    /// 로컬 사용자 강제 해제. workspace의 hard 점유이면 멤버를 함께 해제한다.
    /// soft 점유는 stream client가 아니므로 통지 없이 지운다.
    pub fn release_occupancy(&mut self, surface_id: u32) -> bool {
        if let Some(ws) = self.attach.workspace_of_surface(surface_id) {
            return self.attach.force_detach_workspace(ws).is_some();
        }
        if self.attach.force_detach(surface_id).is_some() {
            return true;
        }
        self.attach.clear_soft(surface_id)
    }

    /// 사용자 포커스 때 parent가 트리에 없는 soft 점유를 정리한다.
    /// 연결 EOF로 수명을 알 수 없는 점유이므로 이 시점에 확인한다.
    #[cfg(any(feature = "gui", test))]
    pub fn reconcile_soft_occupancy_on_focus(&mut self, surface_id: u32) {
        let Some(occ) = self.attach.occupancy_of(surface_id) else {
            return;
        };
        if occ.tier != crate::core::attach::OccupancyTier::Soft {
            return; // hard 는 연결 EOF/force-detach 수명 — 이 경로 무관.
        }
        if let Some(parent) = occ.parent
            && self.find_surface_by_id(parent).is_none()
        {
            self.attach.clear_soft(surface_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CoreState;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn occupy_soft_records_parent_without_hard_predicate() {
        let mut e = engine();
        e.occupy_soft(5000, 99, Some("agent".into())).unwrap();
        assert!(!e.attach.is_hard_occupied(5000));
        let occ = e.attach.occupancy_of(5000).unwrap();
        assert_eq!(occ.parent, Some(99));
    }

    #[test]
    fn self_release_by_subject_and_reject_non_subject() {
        let mut e = engine();
        e.occupy_soft(5000, 99, None).unwrap();
        assert!(e.release_soft_occupancy(5000, 77).is_err());
        assert!(e.attach.occupancy_of(5000).is_some());
        e.release_soft_occupancy(5000, 99).unwrap();
        assert!(e.attach.occupancy_of(5000).is_none());
    }

    #[test]
    fn release_occupancy_clears_soft() {
        let mut e = engine();
        e.occupy_soft(5000, 99, None).unwrap();
        assert!(e.release_occupancy(5000)); // 로컬 force-detach tier 공용
        assert!(e.attach.occupancy_of(5000).is_none());
    }

    #[test]
    fn release_occupancy_clears_hard() {
        let mut e = engine();
        e.attach.acquire(5000, 1).unwrap();
        assert!(e.release_occupancy(5000));
        assert!(!e.attach.is_hard_occupied(5000));
    }

    #[test]
    fn focus_cleanup_releases_soft_when_parent_gone() {
        let mut e = engine();
        e.occupy_soft(5000, 99999, None).unwrap();
        e.reconcile_soft_occupancy_on_focus(5000);
        assert!(e.attach.occupancy_of(5000).is_none()); // 지연 청소.
    }

    #[test]
    fn focus_cleanup_keeps_soft_when_parent_alive() {
        let mut e = engine();
        let parent = e.workspaces[0].all_surface_ids()[0]; // 기본 워크스페이스 live surface
        e.occupy_soft(5000, parent, None).unwrap();
        e.reconcile_soft_occupancy_on_focus(5000);
        assert!(e.attach.occupancy_of(5000).is_some()); // parent 생존 → 유지.
    }

    #[test]
    fn focus_cleanup_ignores_hard_occupancy() {
        let mut e = engine();
        e.attach.acquire(5000, 1).unwrap();
        e.reconcile_soft_occupancy_on_focus(5000);
        assert!(e.attach.is_hard_occupied(5000)); // hard 는 이 경로 무관 — 유지.
    }
}
