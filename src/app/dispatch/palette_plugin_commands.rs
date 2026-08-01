//! Command palette 에서 plugin 전역 command 를 실행했을 때의 큐
//! (`pending_plugin_command_invokes`) drain.
//!
//! `App::try_plugin_shortcut`(`src/app/plugin_glue/shortcut.rs`)의 action 실행
//! 분기와 동일한 처리를 palette 실행 경로에도 적용한다. `surface_id`는 항상
//! `None`이다 — palette 에는 포커스된 plugin surface 컨텍스트가 없으므로, 포커스
//! 없이 매칭된 `Global` 단축키(`match_global_shortcut`)와 동일한 처리를 받는다:
//! action이 있으면 호스트가 직접 실행하고, 없으면 Event Bus `command.invoked`만
//! 발사하고 옛 `command.invoke` IPC는 생략한다(그 IPC는 `surface_id`가 필수라
//! "대상 없음"을 표현할 수 없다 — `key_dispatch::dispatch_plugin_command` 문서 참고).

use winit::window::WindowId;

use crate::app::App;
use crate::plugin;

impl App {
    pub(crate) fn dispatch_pending_palette_plugin_commands(&mut self) {
        // main_windows_iter_mut() 은 `&mut self` 전체를 빌리므로(불투명 헬퍼
        // 메서드 경계 — 필드 단위 분리가 안 됨) self.plugin_manager 와 동시에 쓸 수
        // 없다. 그래서 1단계로 (창, plugin_id, command_id) 만 필드 직접 접근으로
        // drain해 그 빌림을 먼저 끝낸다.
        let mut drained: Vec<(WindowId, String, String)> = Vec::new();
        for (wid, w) in self.view.views.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                for (plugin_id, command_id) in main.state.pending_plugin_command_invokes.drain(..) {
                    drained.push((*wid, plugin_id, command_id));
                }
            }
        }
        if drained.is_empty() {
            return;
        }

        for (wid, plugin_id, command_id) in drained {
            let action = self
                .plugin_manager
                .as_ref()
                .and_then(|mgr| mgr.command_registry.find(&plugin_id, &command_id))
                .and_then(|e| e.action.clone());

            if let Some(action) = action {
                // action이 선언된 command: 호스트가 직접 실행. Event Bus
                // `command.invoked`는 informational로 여전히 발사하지만, 옛
                // `command.invoke` IPC는 이 경로에서 아예 발사하지 않는다
                // (`try_plugin_shortcut`과 동일 근거).
                if let Some(mgr) = self.plugin_manager.as_mut() {
                    crate::plugin_bridge::key_dispatch::emit_command_invoked(
                        mgr,
                        &plugin_id,
                        &command_id,
                        None,
                    );
                }
                let item = plugin::tool_registry::ToolItem {
                    source: plugin::tool_registry::ToolSource::Plugin {
                        plugin_id: plugin_id.clone(),
                        tool_id: command_id.clone(),
                    },
                    key: format!("{plugin_id}/{command_id}"),
                    label_i18n_key: String::new(),
                    icon: None,
                    action,
                    order_hint: 0,
                };
                if let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) {
                    crate::adapters::ui::tools_menu::invoke_tool(
                        &mut main.state,
                        &mut main.core_state,
                        &item,
                    );
                }
            } else if let Some(mgr) = self.plugin_manager.as_mut() {
                crate::plugin_bridge::key_dispatch::dispatch_plugin_command(
                    mgr,
                    &plugin_id,
                    &command_id,
                    None,
                );
            }
        }
    }
}
