//! soft 점유(ADR-0040)의 호스트 내부 core 결선.
//!
//! soft acquire/release 는 `occupancy.*` **IPC method 로 노출하지 않는다** — 소비자가
//! 호스트 내부(terminal.spawn/kill in-process, 로컬 UI force-detach, focus 지연 청소)
//! 뿐이라 전부 in-process 호출이다(ADR-0040 §주체·범위: 살아있는 임의 surface 를
//! 직접 점유하는 CLI/IPC 는 두지 않는다). hard 의 [`attach_runtime`](crate::core::attach_runtime)
//! 가 stream 바이트를 결선하는 자리와 대칭이되, soft 는 StreamHub/gui 비의존이라
//! headless 에서도 컴파일된다. 단 소비처가 전부 gui(gpu.rs·egui_panels) + 후속 작업
//! (terminal.spawn/kill)이라 headless 빌드엔 아직 호출자가 없어 모듈 단위로 침묵한다.
// 이유: soft 점유의 소비처가 전부 gui 라 headless 엔 아직 호출자가 없다(위 모듈 주석).
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

use super::CoreState;
use crate::core::attach::{OccupancyError, OccupancyTier};

impl CoreState {
    /// soft 점유 획득(표시만, write 차단 없음 — ADR-0040). 주체 = `parent` surface.
    /// in-process 전용(IPC method 아님) — `terminal.spawn`/`terminal.adopt` 이 child 를
    /// soft 점유할 때 호출한다. 대상 surface_id 는 **필수**(포커스 독립, 원칙1) — ID 로 직접
    /// 지정. 같은 주체 재-acquire 는 멱등(라벨만 갱신), 다른 주체면 `AlreadyOccupied`.
    pub fn occupy_soft(
        &mut self,
        surface_id: u32,
        parent: u32,
        label: Option<String>,
    ) -> Result<(), OccupancyError> {
        self.attach.acquire_soft(surface_id, parent, label)
    }

    /// soft 점유 self-release(ADR-0040: 주체 본인 해제). 주체(`parent`) 불일치 →
    /// `NotHolder`, 엔트리 없음 → `NotOccupied`(hard `release` 와 동형). `terminal.release`
    /// IPC 가 in-process 호출한다.
    pub fn release_soft_occupancy(
        &mut self,
        surface_id: u32,
        parent: u32,
    ) -> Result<(), OccupancyError> {
        self.attach.release_soft(surface_id, parent)
    }

    /// surface 의 점유를 tier 무관 강제 해제(로컬 사용자 force-detach 공용, ADR-0040).
    /// hard(workspace/surface)면 force_detach 경로(holder 종료 통지 포함)를, soft 면 주체
    /// 검증 없이 엔트리 제거(soft holder 는 stream client 아님 → StreamHub 통지 불필요)를
    /// 탄다. 반환: 실제로 뭔가 해제됐는지. `egui_panels` 의 force-detach 버튼이 호출한다.
    pub fn release_occupancy(&mut self, surface_id: u32) -> bool {
        // workspace 점유 멤버면 멤버 일괄 해제(단계 6 D6).
        if let Some(ws) = self.attach.workspace_of_surface(surface_id) {
            return self.attach.force_detach_workspace(ws).is_some();
        }
        // hard surface lock 이면 holder 통지 + 해제.
        if self.attach.force_detach(surface_id).is_some() {
            return true;
        }
        // 남은 건 soft — 주체 검증 없이 제거.
        self.attach.clear_soft(surface_id)
    }

    /// focus 지연 청소(ADR-0040 §점유 해제·수명): **실 사용자 포커스**를 얻은 surface 가
    /// soft 점유 중이고 그 주체(`parent`) surface 가 더 이상 live set 에 없으면 그 시점에
    /// 점유 없음으로 청소한다. soft 주체는 연결 기반이 아닐 수 있어 죽음을 즉시 인지하지
    /// 못하므로, parent 를 기록만 해두고 이 지연 청소로 회수한다. surface attention 의
    /// 실-포커스 해제(`clear_attention`)와 **같은 자리**에서 호출되어 원칙1(사용자
    /// 상태 불가침)에 안전하다. parent 가 살아있으면 점유를 유지한다.
    pub fn reconcile_soft_occupancy_on_focus(&mut self, surface_id: u32) {
        let Some(occ) = self.attach.occupancy_of(surface_id) else {
            return;
        };
        if occ.tier != OccupancyTier::Soft {
            return; // hard 는 연결 EOF/force-detach 수명 — 이 경로 무관.
        }
        // soft 는 항상 parent 를 기록한다(occupancy_of 투영). live set 판정은 전
        // 워크스페이스 순회(find_surface_by_id) — 포커스 독립.
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
        assert!(!e.attach.is_hard_occupied(5000)); // 입력차단 회귀 0
        let occ = e.attach.occupancy_of(5000).unwrap();
        assert_eq!(occ.parent, Some(99));
    }

    #[test]
    fn self_release_by_subject_and_reject_non_subject() {
        let mut e = engine();
        e.occupy_soft(5000, 99, None).unwrap();
        // 비-주체 release 는 거부(NotHolder 동형) — 점유 유지.
        assert!(e.release_soft_occupancy(5000, 77).is_err());
        assert!(e.attach.occupancy_of(5000).is_some());
        // 주체 release 로 해제.
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
        // parent 99999 는 live set 에 없음.
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
