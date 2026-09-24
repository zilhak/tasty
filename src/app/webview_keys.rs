//! native webview가 전달한 키·포커스를 처리한다. winit 키 처리 전체를 재현하지는 않는다.
//! 플러그인 명령을 먼저 시도하고 호스트 단축키를 실행한 뒤 공용 후처리를 사용한다.

use crate::app::App;
use crate::view::ui::View;

impl App {
    /// 보이는 webview가 활성·비최소화 창에 있으면 true를 반환해 Linux 폴링 예약에 사용한다.
    /// Linux는 먼저 GTK 이벤트를 처리하며 macOS·Windows는 별도 폴링 타이머를 쓰지 않는다.
    pub(crate) fn pump_webview_key_events(&mut self) -> bool {
        let mut any_webview = false;
        let mut needs_poll = false;
        for w in self.view.views.values() {
            let Some(main) = w.as_main() else {
                continue;
            };
            if main.webviews.is_empty() {
                continue;
            }
            any_webview = true;
            if main.webview_any_visible
                && main.base.focused
                && main.base.winit.is_minimized() != Some(true)
            {
                needs_poll = true;
            }
        }
        if !any_webview {
            return false;
        }

        #[cfg(target_os = "linux")]
        crate::system_tray::pump_gtk_events();

        // 이후 dispatch가 self를 가변 대여하므로 view 순회 중에는 이벤트만 모은다.
        let mut focus_batch: Vec<(winit::window::WindowId, Vec<u32>)> = Vec::new();
        let mut key_batch: Vec<(
            winit::window::WindowId,
            Vec<crate::webview::WebViewKeyEvent>,
        )> = Vec::new();
        for (id, w) in &self.view.views {
            let Some(main) = w.as_main() else {
                continue;
            };
            let focus = main.webview_key_bridge.take_focus_requests();
            if !focus.is_empty() {
                focus_batch.push((*id, focus));
            }
            let keys = main.webview_key_bridge.take_pending();
            if !keys.is_empty() {
                key_batch.push((*id, keys));
            }
        }

        // 클릭으로 옮긴 포커스를 먼저 반영해야 단축키가 그 surface에 적용된다.
        for (id, surfaces) in focus_batch {
            for sid in surfaces {
                self.focus_surface_from_webview(id, sid);
            }
        }

        for (id, keys) in key_batch {
            for ev in keys {
                self.dispatch_forwarded_webview_key(id, ev);
            }
        }
        needs_poll
    }

    fn focus_surface_from_webview(&mut self, id: winit::window::WindowId, surface_id: u32) {
        let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
            return;
        };
        if main
            .state
            .focus_surface_by_id(&mut main.core_state, surface_id)
        {
            main.mark_dirty();
        }
    }

    /// 키 도착만으로 모델 포커스를 옮기지 않는다. X11의 자식 창 입력이
    /// 호스트에 없는 focus-follows-mouse 동작을 만들지 않도록 클릭 요청과 구분한다.
    fn dispatch_forwarded_webview_key(
        &mut self,
        id: winit::window::WindowId,
        ev: crate::webview::WebViewKeyEvent,
    ) {
        if self.dispatch_plugin_shortcut_key(id, &ev.key, ev.mods) {
            return;
        }
        let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) else {
            return;
        };
        // 플러그인 단축키에서 처리하지 않은 키는 원래 webview가 사라졌으면 버린다.
        if !main.webviews.contains_key(&ev.surface_id) {
            return;
        }
        if main.state.keyboard_overlay_open() || main.state.fullscreen_stage_active() {
            return;
        }
        if main.handle_shortcut(&ev.key, ev.mods) {
            main.after_shortcut_consumed();
        }
        // 호스트 단축키가 마지막 workspace를 닫았을 수 있어 다음 redraw 전에 닫기 요청을 처리한다.
        self.close_self_requesting_windows();
    }
}
