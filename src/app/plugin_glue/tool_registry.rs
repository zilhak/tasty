//! 모든 MainWindow의 도구 메뉴 (사이드바) 를 PluginManager의 현재 상태로 갱신.

use crate::app::App;

impl App {
    /// PluginManager의 현재 `plugin_tool_items()`를 모든 MainWindow의 AppState로
    /// 푸시한다. plugin 라이프사이클 변경 후(install/enable/disable/grant ui.tool_item
    /// /revoke ui.tool_item/uninstall) 호출해야 사이드바 도구 메뉴가 갱신된다.
    pub(crate) fn refresh_tool_registry(&mut self) {
        let items = match self.plugin_manager.as_ref() {
            Some(mgr) => mgr.plugin_tool_items(),
            None => return,
        };
        for main in self.main_windows_iter_mut() {
            main.state.tool_registry.set_plugin_items(items.clone());
        }
    }
}
