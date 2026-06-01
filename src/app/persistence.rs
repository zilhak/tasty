//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;
use crate::core::intent::DomainIntent;

impl App {
    /// Flush layout persistence — main + parked engine 마다 독립 발화.
    ///
    /// `force=false`: main loop tick. settings.restore_layout + debounce
    ///   (`layout_dirty.should_flush()`) 통과 시에만 저장.
    /// `force=true`: shutdown / quit modal. debounce 무시, `restore_terminal_content`
    ///   설정이 켜져 있으면 layout_dirty 가 false 여도 저장.
    ///
    /// 조건 분기 + `layout_dirty.clear()` 는 Core::apply 안에서 처리.
    /// Intent 큐 우회 — *system loop tick / shutdown* 의 부수효과 (D.3.C.D.4 §8.H).
    pub(crate) fn flush_layout_persistence(&mut self, force: bool) {
        let label = if force { "final" } else { "tick" };
        for w in self.view.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let intent = DomainIntent::SaveLayoutNow {
                    active_workspace: main.state.active_workspace,
                    force,
                };
                if let Err(e) = self.core.apply(&mut main.engine_state, intent) {
                    tracing::warn!("SaveLayoutNow({label}) failed (main): {e}");
                }
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            let intent = DomainIntent::SaveLayoutNow {
                active_workspace: state.active_workspace,
                force,
            };
            if let Err(e) = self.core.apply(engine, intent) {
                tracing::warn!("SaveLayoutNow({label}) failed (parked): {e}");
            }
        }
    }
}
