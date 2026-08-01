//! Command palette 에 노출할 plugin 전역 command 목록을 PluginManager의 현재
//! 상태로 갱신. `tool_registry.rs`와 동형 — Tools 메뉴가 사이드바 항목을 미리
//! 동기화하는 것과 동일한 이유로, palette popup 이 `PluginManager`에 직접 접근할 수
//! 없는 `PopupDef` 고정 시그니처 제약을 우회한다.

use crate::app::App;

impl App {
    /// PluginManager의 현재 `plugin_palette_commands()`를 모든 MainView의 AppState로
    /// 푸시한다. `refresh_tool_registry`와 동일한 시점(plugin 라이프사이클 변경 후)에
    /// 호출해야 한다 — 두 snapshot 모두 `is_disabled` 에 의존하므로 트리거 조건이
    /// 같다.
    ///
    /// INVARIANT: main + parked 두 곳 모두 갱신 (`refresh_tool_registry`와 동일 근거).
    pub(crate) fn refresh_palette_plugin_commands(&mut self) {
        let commands = match self.plugin_manager.as_ref() {
            Some(mgr) => mgr.plugin_palette_commands(),
            None => return,
        };
        for main in self.main_windows_iter_mut() {
            main.state.palette_plugin_commands = commands.clone();
        }
        for (state, _engine) in self.parked_states.iter_mut() {
            state.palette_plugin_commands = commands.clone();
        }
    }
}
