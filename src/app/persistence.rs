//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;
use crate::core::intent::DomainIntent;

impl App {
    /// Flush layout persistence if debounce timer has elapsed.
    ///
    /// 모든 MainWindow + parked 쌍을 순회하며 각각의 engine 에 대해 독립적으로
    /// `DomainIntent::SaveLayoutNow { force: false }` 발화. settings 검사 +
    /// debounce gate + `layout_dirty.clear()` 는 Core::apply 안에서 처리한다.
    ///
    /// Intent 큐 우회 — *system loop tick* 의 부수효과이므로 main / parked 별
    /// engine 에 `core.apply` 직접 호출 (D.3.C.D.4 결정 §8.H).
    pub(crate) fn flush_layout_persistence(&mut self) {
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let active_ws = main.state.active_workspace;
                let intent = DomainIntent::SaveLayoutNow {
                    active_workspace: active_ws,
                    force: false,
                };
                if let Err(e) = self.core.apply(&mut main.engine_state, intent) {
                    tracing::warn!("SaveLayoutNow failed (main): {e}");
                }
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            let intent = DomainIntent::SaveLayoutNow {
                active_workspace: state.active_workspace,
                force: false,
            };
            if let Err(e) = self.core.apply(engine, intent) {
                tracing::warn!("SaveLayoutNow failed (parked): {e}");
            }
        }
    }

    /// Force flush layout persistence on shutdown (ignore debounce).
    ///
    /// `restore_terminal_content` 가 켜져 있으면 layout 자체가 dirty 가 아니어도
    /// 저장한다 — Core::apply 의 force=true 분기가 처리.
    pub(crate) fn flush_layout_persistence_final(&mut self) {
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let active_ws = main.state.active_workspace;
                let intent = DomainIntent::SaveLayoutNow {
                    active_workspace: active_ws,
                    force: true,
                };
                if let Err(e) = self.core.apply(&mut main.engine_state, intent) {
                    tracing::warn!("SaveLayoutNow(final) failed (main): {e}");
                }
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            let intent = DomainIntent::SaveLayoutNow {
                active_workspace: state.active_workspace,
                force: true,
            };
            if let Err(e) = self.core.apply(engine, intent) {
                tracing::warn!("SaveLayoutNow(final) failed (parked): {e}");
            }
        }
    }
}
