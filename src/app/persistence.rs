//! Layout persistence flush — debounce 만료 시 + shutdown 시.

use crate::app::App;

impl App {
    /// Flush layout persistence if debounce timer has elapsed.
    pub(crate) fn flush_layout_persistence(&mut self) {
        // Pick the active workspace index from the focused main window if any,
        // otherwise default to 0.
        let active_ws = self
            .windows
            .values()
            .find_map(|w| w.as_main().map(|m| m.state.active_workspace))
            .unwrap_or(0);
        let engine = self.engine_state_mut();
        if engine.settings.general.restore_layout && engine.layout_dirty.should_flush() {
            crate::engine::layout_persistence::save_to_disk(engine, active_ws);
            engine.layout_dirty.clear();
        }
    }

    /// Force flush layout persistence on shutdown (ignore debounce).
    ///
    /// `restore_terminal_content` 가 켜져 있으면 layout 자체가 dirty 가 아니어도
    /// 저장한다. scrollback 은 매 출력마다 dirty 를 마크하지 않는데, 그래야 종료
    /// 시점에 disk 캡처가 일어난다.
    pub(crate) fn flush_layout_persistence_final(&mut self) {
        let active_ws = self
            .windows
            .values()
            .find_map(|w| w.as_main().map(|m| m.state.active_workspace))
            .unwrap_or(0);
        let engine = self.engine_state_mut();
        let g = &engine.settings.general;
        let should_save =
            g.restore_layout && (engine.layout_dirty.is_dirty() || g.restore_terminal_content);
        if should_save {
            crate::engine::layout_persistence::save_to_disk(engine, active_ws);
            engine.layout_dirty.clear();
        }
    }
}
