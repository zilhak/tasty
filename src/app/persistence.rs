//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;

impl App {
    /// Flush layout persistence if debounce timer has elapsed.
    ///
    /// 모든 MainWindow + parked 쌍을 순회하며 각각의 engine 에 대해 독립적으로 flush.
    /// 원래 디자인에서 각 AppState 가 자기 engine 을 갖고 있었던 의미를 보존한다.
    pub(crate) fn flush_layout_persistence(&mut self) {
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let active_ws = main.state.active_workspace;
                let engine = &mut main.engine_state;
                if engine.settings.general.restore_layout && engine.layout_dirty.should_flush() {
                    crate::engine::layout_persistence::save_to_disk(engine, active_ws);
                    engine.layout_dirty.clear();
                }
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            if engine.settings.general.restore_layout && engine.layout_dirty.should_flush() {
                crate::engine::layout_persistence::save_to_disk(engine, state.active_workspace);
                engine.layout_dirty.clear();
            }
        }
    }

    /// Force flush layout persistence on shutdown (ignore debounce).
    ///
    /// `restore_terminal_content` 가 켜져 있으면 layout 자체가 dirty 가 아니어도
    /// 저장한다. scrollback 은 매 출력마다 dirty 를 마크하지 않는데, 그래야 종료
    /// 시점에 disk 캡처가 일어난다.
    pub(crate) fn flush_layout_persistence_final(&mut self) {
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                let active_ws = main.state.active_workspace;
                let engine = &mut main.engine_state;
                let g = &engine.settings.general;
                let should_save = g.restore_layout
                    && (engine.layout_dirty.is_dirty() || g.restore_terminal_content);
                if should_save {
                    crate::engine::layout_persistence::save_to_disk(engine, active_ws);
                    engine.layout_dirty.clear();
                }
            }
        }
        for (state, engine) in self.parked_states.iter_mut() {
            let g = &engine.settings.general;
            let should_save =
                g.restore_layout && (engine.layout_dirty.is_dirty() || g.restore_terminal_content);
            if should_save {
                crate::engine::layout_persistence::save_to_disk(engine, state.active_workspace);
                engine.layout_dirty.clear();
            }
        }
    }
}
