//! native webview가 전달한 키·포커스를 처리한다. winit 키 처리 전체를 재현하지는 않는다.
//! 플러그인 명령을 먼저 시도하고 호스트 단축키를 실행한 뒤 공용 후처리를 사용한다.

use crate::app::App;
use crate::app::window_access::engines_mut;
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
            if webview_poll_needed(
                main.webview_any_visible,
                main.base.winit.is_minimized() == Some(true),
                main.base.state.focused,
                || main.webviews.values().any(|wv| wv.holds_keyboard_focus()),
            ) {
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
        let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
            return;
        };
        if main.state.focus_surface_by_id(engine.core, surface_id) {
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
        let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
            return;
        };
        // 플러그인 단축키에서 처리하지 않은 키는 원래 webview가 사라졌으면 버린다.
        if !main.webviews.contains_key(&ev.surface_id) {
            return;
        }
        if main.state.keyboard_overlay_open() || main.state.fullscreen_stage_active() {
            return;
        }
        if main.handle_shortcut(&engine.read(), &ev.key, ev.mods) {
            main.after_shortcut_consumed(&engine.read());
        }
        // 호스트 단축키가 마지막 workspace를 닫았을 수 있어 다음 redraw 전에 닫기 요청을 처리한다.
        self.close_self_requesting_windows();
    }
}

/// GDK 이벤트(WebView의 클릭·키)를 처리하려고 짧은 주기 폴링이 필요한지. Linux GDK는 별도 X
/// 연결로 이벤트를 받아 winit을 깨우지 못한다. 보이는 WebView가 있고 창이 최소화되지 않았으며
/// 창이 활성일 때 폴링한다. WebView 자식이 X 포커스를 쥐면 winit은 부모 창에 Focused(false)를
/// 보내므로 자식이 포커스를 가진 경우도 활성으로 본다 — 빠뜨리면 클릭이 다음 깨움(약 1.5초 뒤)까지
/// 처리되지 않는다. `webview_holds` 는 OS에 묻는 호출이라 앞 조건이 모두 참이고 winit 값이 거짓일
/// 때만 부른다.
fn webview_poll_needed(
    any_visible: bool,
    minimized: bool,
    winit_focused: bool,
    webview_holds: impl FnOnce() -> bool,
) -> bool {
    any_visible
        && !minimized
        && crate::view::main::redraw::host_window_has_os_focus(winit_focused, webview_holds)
}

#[cfg(test)]
mod tests {
    use super::webview_poll_needed;

    /// 자식 WebView가 포커스를 쥐어 winit이 창을 비활성으로 알려도 폴링한다.
    #[test]
    fn a_webview_holding_the_focus_keeps_the_poll_running() {
        assert!(webview_poll_needed(true, false, false, || true));
        assert!(webview_poll_needed(true, false, true, || false));
        assert!(!webview_poll_needed(true, false, false, || false));
    }

    /// 보이는 WebView가 없거나 최소화됐으면 OS에 포커스를 묻지 않고 폴링하지 않는다.
    #[test]
    fn hidden_or_minimized_windows_do_not_poll_or_ask_the_os() {
        let asked = std::cell::Cell::new(false);
        let ask = || {
            asked.set(true);
            true
        };
        assert!(!webview_poll_needed(false, false, false, ask));
        assert!(!webview_poll_needed(true, true, false, ask));
        assert!(!asked.get());
    }
}
