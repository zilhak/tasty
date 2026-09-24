//! PopupDef는 PluginManager에 직접 접근할 수 없어 팔레트 명령 목록을 AppState에 복사한다.

use crate::app::App;

impl App {
    /// 플러그인 활성 상태가 바뀌면 갱신한다.
    /// 창 복원 때 옛 목록을 표시하지 않도록 parked 상태도 함께 갱신한다.
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
