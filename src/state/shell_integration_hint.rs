//! 셸 통합 안내 배너의 표시 기록. 안내가 필요한지는 Core가 판단하고 여기서는 한 번만 보이게 한다.

use super::RequestContext;

impl RequestContext {
    /// 처음 요청받은 surface면 기록하고 true를 반환한다.
    /// 배너를 넣기 전에 기록하므로 표시 실패를 재시도하지 않는다.
    pub(crate) fn take_first_shell_integration_hint(&mut self, surface_id: u32) -> bool {
        self.shell_integration_hint_shown.insert(surface_id)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hint_is_taken_once_per_surface() {
        let (mut state, mut _engine_session) = crate::state::tests::test_state();
        let _engine = _engine_session.borrow_mut();
        assert!(state.take_first_shell_integration_hint(1));
        assert!(!state.take_first_shell_integration_hint(1));
        assert!(state.take_first_shell_integration_hint(2));
    }

    /// 닫힌 surface의 창 표시 기록과 AppServices 요청 기록만 지우고 다른 surface의 기록은 남긴다.
    #[test]
    fn surface_cleanup_forgets_only_that_surface() {
        let (mut state, mut engine_session) = crate::state::tests::test_state();
        let mut engine = engine_session.borrow_mut();
        let live = engine.workspace_at(0).unwrap().all_surface_ids()[0];
        let removed = u32::MAX;
        state.shell_integration_hint_shown.extend([live, removed]);
        engine
            .live
            .shell_integration_hint_requested
            .extend([live, removed]);
        engine.cleanup_surface_observations(removed);
        state.reconcile_presentation(&engine);
        assert!(!state.shell_integration_hint_shown.contains(&removed));
        assert!(state.shell_integration_hint_shown.contains(&live));
        assert!(
            !engine
                .live
                .shell_integration_hint_requested
                .contains(&removed)
        );
        assert!(engine.live.shell_integration_hint_requested.contains(&live));
    }
}
