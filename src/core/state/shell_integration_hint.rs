//! OSC 133 셸 통합 미설치 감지. surface 가 PTY 출력을 내고 있는데도 일정
//! 시간이 지나도록 `PromptBoundary`(A/B/C/D 아무 phase)를 한 번도 못 받으면, 그
//! 셸에는 OSC 133 통합 스크립트가 로드되지 않았다고 보고 안내 배너를 1회 띄운다.
//!
//! 마우스 캡처 안내 배너(`mouse_capture_banner_suppressed_surfaces`)와 동일하게
//! 자동 조치 없이 설명만 한다 — highlight 연결은 이 판정과 무관하며 별도 경로로
//! 분리되어 있다(여기서는 다루지 않음, 상세 `docs/features/surface-highlight/index.md`).

use super::CoreState;

/// 배너 판정 전 최소 대기 시간. 셸이 뜨고 최소한의 출력(첫 프롬프트 등)이 나올
/// 시간을 준다 — 너무 짧으면 정상적으로 통합된 셸에서도 최초 프롬프트가 그려지기
/// 전에 오탐할 수 있고, 너무 길면 사용자가 이미 여러 명령을 실행한 뒤에야 안내를
/// 받는다.
const SHELL_INTEGRATION_HINT_DELAY: std::time::Duration = std::time::Duration::from_secs(10);

impl CoreState {
    /// 이 surface 의 첫 PTY 출력을 관측한 시각을 기록한다(이미 기록돼 있으면 no-op).
    pub(crate) fn note_first_output(&mut self, surface_id: u32) {
        self.shell_integration_first_output_at
            .entry(surface_id)
            .or_insert_with(std::time::Instant::now);
    }

    /// 이 surface 가 OSC 133 boundary(A/B/C/D 아무 phase)를 한 번이라도 받았음을
    /// 기록한다 — 이후 이 surface 는 배너 판정에서 영구 제외된다.
    pub(crate) fn note_prompt_boundary_seen(&mut self, surface_id: u32) {
        self.shell_integration_boundary_seen.insert(surface_id);
    }

    /// "셸 통합 미설치" 배너를 지금 띄워야 하는지 판정한다. 아직 안 띄웠고, 첫
    /// 출력 후 [`SHELL_INTEGRATION_HINT_DELAY`] 가 지나도록 boundary 를 한 번도
    /// 못 받았으면 `true` 를 반환하고 동시에 1회성 표시 플래그를 세운다(dedup —
    /// 호출자는 반환값이 `true` 일 때만 배너를 push 하면 된다).
    pub(crate) fn take_shell_integration_hint_due(&mut self, surface_id: u32) -> bool {
        if self.shell_integration_boundary_seen.contains(&surface_id) {
            return false;
        }
        if self.shell_integration_hint_shown.contains(&surface_id) {
            return false;
        }
        let Some(&first_output) = self.shell_integration_first_output_at.get(&surface_id) else {
            return false;
        };
        if first_output.elapsed() < SHELL_INTEGRATION_HINT_DELAY {
            return false;
        }
        self.shell_integration_hint_shown.insert(surface_id);
        true
    }

    /// surface 종료 시 3 개 캐시 모두 정리한다 (`AppState::cleanup_surface` 가 호출).
    pub(crate) fn forget_shell_integration_hint(&mut self, surface_id: u32) {
        self.shell_integration_first_output_at.remove(&surface_id);
        self.shell_integration_boundary_seen.remove(&surface_id);
        self.shell_integration_hint_shown.remove(&surface_id);
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
    fn no_hint_before_first_output_recorded() {
        let mut e = engine();
        assert!(!e.take_shell_integration_hint_due(1));
    }

    #[test]
    fn no_hint_immediately_after_first_output() {
        let mut e = engine();
        e.note_first_output(1);
        // 지연 시간이 아직 지나지 않음 — 방금 기록했으니 즉시 due 일 수 없다.
        assert!(!e.take_shell_integration_hint_due(1));
    }

    #[test]
    fn boundary_seen_suppresses_hint_permanently() {
        let mut e = engine();
        e.note_first_output(1);
        e.note_prompt_boundary_seen(1);
        // 임계 시간이 지났다고 가정해도(과거 시각 주입) boundary 를 받았으므로 안 뜬다.
        e.shell_integration_first_output_at.insert(
            1,
            std::time::Instant::now() - std::time::Duration::from_secs(999),
        );
        assert!(!e.take_shell_integration_hint_due(1));
    }

    #[test]
    fn hint_fires_once_after_delay_elapsed_without_boundary() {
        let mut e = engine();
        e.shell_integration_first_output_at.insert(
            1,
            std::time::Instant::now() - std::time::Duration::from_secs(999),
        );
        assert!(e.take_shell_integration_hint_due(1));
        // 1회성 — 다시 호출하면 이미 표시됨으로 처리되어 false.
        assert!(!e.take_shell_integration_hint_due(1));
    }

    #[test]
    fn different_surfaces_are_independent() {
        let mut e = engine();
        e.shell_integration_first_output_at.insert(
            1,
            std::time::Instant::now() - std::time::Duration::from_secs(999),
        );
        e.note_prompt_boundary_seen(2);
        e.shell_integration_first_output_at.insert(
            2,
            std::time::Instant::now() - std::time::Duration::from_secs(999),
        );
        assert!(e.take_shell_integration_hint_due(1));
        assert!(!e.take_shell_integration_hint_due(2));
    }

    /// surface **둘**을 채우고 하나만 잊는다. 하나만 채우면 "그것만 지웠다" 와
    /// "세 캐시를 통째로 비웠다" 가 같은 관측이라, 세 `remove` 를 `clear` 로 바꿔도
    /// 안 죽는다.
    #[test]
    fn forget_shell_integration_hint_clears_all_three_caches() {
        let mut e = engine();
        for sid in [1, 2] {
            e.note_first_output(sid);
            e.note_prompt_boundary_seen(sid);
            e.shell_integration_hint_shown.insert(sid);
        }
        e.forget_shell_integration_hint(1);
        assert!(!e.shell_integration_first_output_at.contains_key(&1));
        assert!(!e.shell_integration_boundary_seen.contains(&1));
        assert!(!e.shell_integration_hint_shown.contains(&1));
        // 잊으라고 하지 않은 surface 는 세 캐시에 그대로 남는다.
        assert!(e.shell_integration_first_output_at.contains_key(&2));
        assert!(e.shell_integration_boundary_seen.contains(&2));
        assert!(e.shell_integration_hint_shown.contains(&2));
    }
}
