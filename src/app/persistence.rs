//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;

impl App {
    /// Flush layout persistence if debounce timer has elapsed.
    pub(crate) fn flush_layout_persistence(&mut self) {
        for w in self.windows.values_mut() {
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            if main.state.engine.settings.general.restore_layout
                && main.state.engine.layout_dirty.should_flush()
            {
                crate::layout_persistence::save_to_disk(
                    &mut main.state.engine,
                    main.state.active_workspace,
                );
                main.state.engine.layout_dirty.clear();
            }
        }
        for state in &mut self.parked_states {
            if state.engine.settings.general.restore_layout
                && state.engine.layout_dirty.should_flush()
            {
                crate::layout_persistence::save_to_disk(&mut state.engine, 0);
                state.engine.layout_dirty.clear();
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
            let Some(main) = w.as_main_mut() else {
                continue;
            };
            let g = &main.state.engine.settings.general;
            let should_save = g.restore_layout
                && (main.state.engine.layout_dirty.is_dirty() || g.restore_terminal_content);
            if should_save {
                crate::layout_persistence::save_to_disk(
                    &mut main.state.engine,
                    main.state.active_workspace,
                );
                main.state.engine.layout_dirty.clear();
            }
        }
        for state in &mut self.parked_states {
            let g = &state.engine.settings.general;
            let should_save = g.restore_layout
                && (state.engine.layout_dirty.is_dirty() || g.restore_terminal_content);
            if should_save {
                crate::layout_persistence::save_to_disk(&mut state.engine, 0);
                state.engine.layout_dirty.clear();
            }
        }
    }
}
