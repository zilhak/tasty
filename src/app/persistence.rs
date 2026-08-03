//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;
use crate::core::intent::DomainIntent;

impl App {
    /// Flush layout persistence — main + parked engine 마다 독립 발화.
    ///
    /// `force=false`: main loop tick. settings.restore_layout + debounce
    ///   (`layout_dirty.should_flush()`) 통과 시에만 저장.
    /// `force=true`: shutdown / quit modal. debounce 무시, `restore_surface_content`
    ///   설정이 켜져 있으면 layout_dirty 가 false 여도 저장.
    ///
    /// 조건 분기 + `layout_dirty.clear()` 는 Core::apply 안에서 처리.
    /// Intent 큐 우회 — *system loop tick / shutdown* 의 부수효과 (D.3.C.D.4 §8.H).
    pub(crate) fn flush_layout_persistence(&mut self, force: bool) {
        let label = if force { "final" } else { "tick" };
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                Self::flush_one_engine(
                    &mut self.core,
                    &mut main.core_state,
                    main.state.active_workspace,
                    force,
                    label,
                    "main",
                );
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            Self::flush_one_engine(
                &mut self.core,
                engine,
                state.active_workspace,
                force,
                label,
                "parked",
            );
        }
    }

    /// `flush_layout_persistence` 의 공용 루프 바디 — main-view engine 과
    /// parked engine 모두 동일 로직(intent 생성 + apply + 실패 warn)이라 통합.
    fn flush_one_engine(
        core: &mut crate::core::Core,
        engine: &mut crate::core::CoreState,
        active_workspace: usize,
        force: bool,
        label: &str,
        kind: &str,
    ) {
        let intent = DomainIntent::SaveLayoutNow {
            active_workspace,
            force,
        };
        if let Err(e) = core.apply(engine, intent) {
            tracing::warn!("SaveLayoutNow({label}) failed ({kind}): {e}");
        }
    }
}
