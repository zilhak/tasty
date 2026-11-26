//! native webview가 전달한 키·포커스를 처리한다. winit 키 처리 전체를 재현하지는 않는다.
//! 플러그인 명령을 먼저 시도하고 호스트 단축키를 실행한 뒤 공용 후처리를 사용한다.

use crate::app::App;
use crate::app::window_access::engines_mut;
use crate::view::ui::View;

impl App {
    /// 보이는 페이지나 수신 중인 native 작업이 활성·비최소화 창에 있으면 Linux 폴링을 예약한다.
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
                main.webview_any_visible || main.webviews.values().any(received_work_pending),
                main.base.winit.is_minimized() == Some(true),
                main.base.state.focused,
                || main.webviews.values().any(|wv| wv.holds_keyboard_focus()),
            ) {
                needs_poll = true;
            }
        }
        #[cfg(target_os = "linux")]
        let any_webview = any_webview || crate::webview::entry_response::callbacks_pending();
        if !any_webview {
            return false;
        }

        #[cfg(target_os = "linux")]
        self.pump_gtk_events();

        // 누름 알림·전달 키를 창마다 도착 순서대로 소비한다. 단축키는 그 앞 누름이 옮긴 포커스로 판정된다.
        self.consume_webview_focus_requests();
        needs_poll
    }

    /// GTK 대기 이벤트를 하나씩 처리하고, 이벤트마다 native 포커스 알림을 소비한다(`consume_webview_focus_requests`).
    /// 한 번의 처리에 [WebView B 누름, B 편집 키]가 함께 들어도 누름 알림이 키 쥘 곳을 게시한 뒤 키 신호가 판정한다.
    /// 키·포커스를 전달하는 호스트 루프 두 자리(`pump_webview_key_events`, `poll_tray_menu_events`)가 이 함수를
    /// 부른다. halt 펌프(`halted_webview_paint_deadline`)는 입력을 전달하지 않으므로 소비하지 않는 펌프를 쓴다.
    #[cfg(target_os = "linux")]
    pub(crate) fn pump_gtk_events(&mut self) {
        crate::system_tray::pump_gtk_events_each(|| self.consume_webview_focus_requests());
    }

    /// 창마다 native 입력(누름 알림·전달 키)을 도착 순서대로 소비한다(`consume_native_inputs`). 누름으로 포커스가
    /// 바뀐 창은 다시 그린다.
    pub(crate) fn consume_webview_focus_requests(&mut self) {
        let pending = self.view.views.values().any(|w| {
            w.as_main()
                .is_some_and(|main| main.webview_key_bridge.has_focus_requests())
        });
        if !pending {
            return;
        }
        let ids: Vec<winit::window::WindowId> = self
            .view
            .views
            .iter()
            .filter(|(_, w)| w.as_main().is_some())
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            consume_native_inputs(self, id);
        }
    }

    /// 처리 결과를 키를 가져간 WebView에 알린다. Tasty가 처리하지 못한 키는 그 WebView에 다시 넣어
    /// 엔진·페이지로 보낸다. 입력 게이트로 막은 키는 다시 넣지 않는다.
    fn finish_forwarded_webview_key(
        &mut self,
        id: winit::window::WindowId,
        surface_id: u32,
        key_id: u64,
        outcome: ForwardedKeyOutcome,
    ) {
        if let Some(webview) = self
            .view
            .views
            .get(&id)
            .and_then(|w| w.as_main())
            .and_then(|main| main.webviews.get(&surface_id))
        {
            webview.finish_forwarded_key(key_id, outcome.resends_to_page());
        }
    }

    /// 키 도착만으로 모델 포커스를 옮기지 않는다. X11의 자식 창 입력이
    /// 호스트에 없는 focus-follows-mouse 동작을 만들지 않도록 클릭 요청과 구분한다.
    fn dispatch_forwarded_webview_key(
        &mut self,
        id: winit::window::WindowId,
        ev: crate::webview::WebViewKeyEvent,
    ) -> ForwardedKeyOutcome {
        // winit 경로의 modal_blocks_input·입력 게이트와 같이, modal 창이나 overlay·전체화면 무대가
        // 열린 동안에는 plugin·새로고침·host 단축키 어느 것도 처리하지 않는다.
        let modal_active = self.view.is_modal_active();
        {
            // 창이 없으면 어느 단계도 처리할 대상이 없다.
            let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
                return ForwardedKeyOutcome::Gated;
            };
            let webviews = &main.webviews;
            if let Some(outcome) = forwarded_gate_and_plugin_steps(
                &mut main.state,
                &engine,
                modal_active,
                self.plugin_manager.as_mut(),
                |sid| webviews.contains_key(&sid),
                &ev,
            ) {
                return outcome;
            }
        }
        let Some((main, mut engine)) = engines_mut!(self).window_pair(id) else {
            return ForwardedKeyOutcome::Gated;
        };
        // 플러그인 단축키에서 처리하지 않은 키는 원래 webview가 사라졌으면 버린다.
        if !main.webviews.contains_key(&ev.surface_id) {
            return ForwardedKeyOutcome::Gated;
        }
        // WebView 키는 창 이벤트를 거치지 않는다. 호스트 단축키의 레이아웃 스윕이 최신 창 상태를 읽게 한다.
        main.sync_window_drawn();
        if let Some(outcome) = dispatch_native_refresh_key(main, &mut engine, &ev) {
            return outcome;
        }
        // 호스트 단축키는 모델 포커스가 아니라 키를 받은 surface 의 workspace·pane·탭을 대상으로 한다.
        let Some(target) = main
            .state
            .command_target_for_surface(&engine.read(), ev.surface_id)
        else {
            return ForwardedKeyOutcome::Gated;
        };
        let handled = match main.run_received_surface_key(
            &engine.read(),
            &ev.key,
            ev.mods,
            ev.surface_id,
            &target,
        ) {
            Some(handled) => handled,
            None => main.handle_received_shortcut(&engine.read(), &ev.key, ev.mods, &target),
        };
        if handled {
            main.after_shortcut_consumed(&engine.read());
        }
        // 호스트 단축키가 마지막 workspace를 닫았을 수 있어 다음 redraw 전에 닫기 요청을 처리한다.
        self.close_self_requesting_windows();
        ForwardedKeyOutcome::from_handled(handled)
    }
}

/// native WebView 가 넘긴 키 묶음을 키 하나씩 처리하는 쪽. 제품은 `App` 이고, 소비 순서 시험은 대역을 쓴다.
pub(crate) trait ForwardedKeyConsumer {
    /// 키 하나를 plugin·새로고침·host 단축키로 처리한다.
    fn dispatch(
        &mut self,
        id: winit::window::WindowId,
        ev: crate::webview::WebViewKeyEvent,
    ) -> ForwardedKeyOutcome;
    /// 키 하나를 처리한 직후, 그 창 대기열에 포커스를 주는 팝업 열기가 있으면 다음 키 판정 전에 적용한다.
    fn apply_focus_opens(&mut self, id: winit::window::WindowId);
    /// 처리 결과를 키를 가져간 WebView 에 알린다.
    fn finish(
        &mut self,
        id: winit::window::WindowId,
        surface_id: u32,
        key_id: u64,
        outcome: ForwardedKeyOutcome,
    );
}

impl ForwardedKeyConsumer for App {
    fn dispatch(
        &mut self,
        id: winit::window::WindowId,
        ev: crate::webview::WebViewKeyEvent,
    ) -> ForwardedKeyOutcome {
        self.dispatch_forwarded_webview_key(id, ev)
    }

    fn apply_focus_opens(&mut self, id: winit::window::WindowId) {
        self.apply_user_focus_opens(id);
    }

    fn finish(
        &mut self,
        id: winit::window::WindowId,
        surface_id: u32,
        key_id: u64,
        outcome: ForwardedKeyOutcome,
    ) {
        self.finish_forwarded_webview_key(id, surface_id, key_id, outcome);
    }
}

/// 한 창 대기열 앞의 native 누름 알림을 도착 순서대로 소비한다(`WebViewKeyBridge::take_leading_presses`). 첫 전달 키
/// 뒤의 입력은 host 단축키 처리가 있는 소비(`consume_native_inputs`)에 남긴다. 알림마다 누름 전이와 게시
/// (`press_and_publish`)를 적용한다. 포커스가 바뀌었으면 참이다.
pub(crate) fn consume_native_focus_requests(
    state: &mut crate::state::MainViewState,
    core: &crate::core::CoreState,
    bridge: &crate::webview::WebViewKeyBridge,
) -> bool {
    let mut changed = false;
    for (surface_id, kind) in bridge.take_leading_presses() {
        changed |= press_and_publish(state, core, bridge, surface_id, kind);
    }
    changed
}

/// 누름 알림 한 건의 전이(`apply_native_press`)를 적용하고 그 전이 뒤의 키 쥘 곳(`key_release::webview_key_holder`)을
/// 다리에 게시한다. 그래서 누른 직후의 편집 키는 다음 프레임 게시를 기다리지 않고 누른 페이지에서 처리되고, 연속
/// 알림도 알림마다 같은 전이를 받는다. 선택이 바뀌었으면 참이다.
pub(crate) fn press_and_publish(
    state: &mut crate::state::MainViewState,
    core: &crate::core::CoreState,
    bridge: &crate::webview::WebViewKeyBridge,
    surface_id: u32,
    kind: crate::webview::PressKind,
) -> bool {
    let changed = apply_native_press(state, core, surface_id, kind);
    bridge.set_key_holder(crate::app::webview_sync::key_release::webview_key_holder(
        state, core,
    ));
    changed
}

/// 한 창의 native 입력을 도착 순서대로 소비하는 쪽. 제품은 `App` 이고, 순서 시험은 대역을 쓴다.
pub(crate) trait NativeInputConsumer: ForwardedKeyConsumer {
    /// 그 창 대기열의 입력을 모두 꺼낸다. 소비하는 동안 들어오는 키는 대기열이 비어 바로 판정된다.
    fn take_native_inputs(
        &mut self,
        id: winit::window::WindowId,
    ) -> Vec<crate::webview::NativeInput>;
    /// 누름 알림 한 건의 전이와 게시(`press_and_publish`).
    fn native_press(
        &mut self,
        id: winit::window::WindowId,
        surface_id: u32,
        kind: crate::webview::PressKind,
    );
}

/// 한 창의 native 입력을 도착 순서대로 소비한다. 누름은 전이와 게시를, 전달 키는 그 자리에서 키마다 처리 → 포커스
/// 열기 적용(winit 키와 같은 조기 적용, 그 키가 포커스를 주는 팝업을 열었으면 다음 키는 그 팝업의 게이트로 판정한다)
/// → 결과 알림을 받는다.
pub(crate) fn consume_native_inputs(
    consumer: &mut impl NativeInputConsumer,
    id: winit::window::WindowId,
) {
    use crate::webview::NativeInput;
    for input in consumer.take_native_inputs(id) {
        match input {
            NativeInput::Press { surface_id, kind } => consumer.native_press(id, surface_id, kind),
            NativeInput::Forwarded(ev) => {
                let (key_id, surface_id) = (ev.id, ev.surface_id);
                let outcome = dispatch_and_apply(consumer, id, ev);
                consumer.finish(id, surface_id, key_id, outcome);
            }
        }
    }
}

/// 키 하나를 처리하고, 그 키가 포커스를 주는 팝업을 열었으면 다음 입력 판정 전에 적용한다(winit 키와 같은 조기
/// 적용). 결과와 관계없이 적용을 부른다. 대기열에 그런 요청이 없으면 적용은 아무것도 하지 않는다.
fn dispatch_and_apply(
    consumer: &mut impl NativeInputConsumer,
    id: winit::window::WindowId,
    ev: crate::webview::WebViewKeyEvent,
) -> ForwardedKeyOutcome {
    let outcome = consumer.dispatch(id, ev);
    consumer.apply_focus_opens(id);
    outcome
}

impl NativeInputConsumer for App {
    fn take_native_inputs(
        &mut self,
        id: winit::window::WindowId,
    ) -> Vec<crate::webview::NativeInput> {
        self.view
            .views
            .get(&id)
            .and_then(|w| w.as_main())
            .map(|main| main.webview_key_bridge.take_inputs())
            .unwrap_or_default()
    }

    fn native_press(
        &mut self,
        id: winit::window::WindowId,
        surface_id: u32,
        kind: crate::webview::PressKind,
    ) {
        let Some((main, engine)) = engines_mut!(self).window_pair(id) else {
            return;
        };
        if press_and_publish(
            &mut main.state,
            engine.core,
            &main.webview_key_bridge,
            surface_id,
            kind,
        ) {
            main.mark_dirty();
        }
    }
}

/// native 누름 알림 한 건의 전이. 그 surface 의 회수 기록을 지우고(`note_native_webview_focus`), 실제 버튼 누름이면
/// 주소 띠의 키 포커스를 놓은 뒤(native 페이지 누름은 egui 가 보지 않아 주소 띠 입력칸이 스스로 놓지 않는다) 그
/// surface 로 선택을 옮긴다. 누름 없는 포커스 획득은 주소 띠를 놓지 않는다. 덮음과 경로 밖 팝업은 뒤따르는 게시의
/// 정규화(`webview_key_holder`)가 정한다. 선택이 바뀌었으면 참이다.
fn apply_native_press(
    state: &mut crate::state::MainViewState,
    core: &crate::core::CoreState,
    surface_id: u32,
    kind: crate::webview::PressKind,
) -> bool {
    crate::app::webview_sync::key_release::note_native_webview_focus(state, surface_id);
    let released =
        kind == crate::webview::PressKind::Click && state.html_address.has_keyboard_focus();
    if released {
        state.html_address.release_focus();
    }
    state.focus_surface_by_id(core, surface_id) || released
}

/// native WebView가 넘긴 키의 처리 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForwardedKeyOutcome {
    /// Tasty 단축키가 동작했다. 키를 넘기지 않는다.
    Handled,
    /// 단축키가 동작하지 않았다. 키를 엔진·페이지로 넘긴다.
    NotHandled,
    /// 입력 게이트가 막았거나 키를 받은 WebView·창이 없다. 넘기지 않는다.
    Gated,
}

impl ForwardedKeyOutcome {
    fn from_handled(handled: bool) -> Self {
        if handled {
            Self::Handled
        } else {
            Self::NotHandled
        }
    }

    /// 키를 가져간 WebView 에 다시 넣어 엔진·페이지로 보낼지. 처리하지 못한 키만 보낸다. 입력 게이트가 막은 키는
    /// 보내지 않는다.
    fn resends_to_page(self) -> bool {
        self == Self::NotHandled
    }
}

// A receiving page may be hidden by its host state while WebKit still owes native completions.
// Cached-only stores must not keep a hidden view polling.
fn received_work_pending(webview: &crate::webview::PlatformWebView) -> bool {
    webview.entry_response_source().is_some_and(|source| {
        source.is_current()
            && (webview.nav_state() == crate::webview::NavState::Loading
                || source.pending_resources())
    })
}

/// Linux GDK는 별도 X 연결로 이벤트를 받아 winit을 깨우지 못한다. 보이는 페이지 또는
/// native 완료 대기가 있고 창이 활성·비최소화일 때 폴링한다. native 자식이 포커스를
/// 가진 경우도 활성으로 본다. OS 포커스 조회는 앞 조건과 winit 값으로 결정한다.
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

/// 한 창에서 native WebView 가 넘긴 키의 앞 두 단계. 게이트(`forwarded_key_gate_open`)가 닫히면 `Gated`, plugin
/// 단축키(`plugin_shortcut_in_window`)가 처리하면 `Handled` 로 결과가 정해진다. 정해지지 않으면 None 이고 호출자는
/// 새로고침·내장 단축키 단계로 간다. `has_webview` 는 surface 에 native WebView 가 있는지다. App 의 디스패치와 제품 상태
/// 시험이 함께 쓴다.
fn forwarded_gate_and_plugin_steps(
    state: &mut crate::state::MainViewState,
    engine: &crate::runtime::engine_access::EngineMut<'_>,
    modal_active: bool,
    plugin_manager: Option<&mut crate::plugin::PluginManager>,
    has_webview: impl Fn(u32) -> bool,
    ev: &crate::webview::WebViewKeyEvent,
) -> Option<ForwardedKeyOutcome> {
    if !forwarded_key_gate_open(state, engine, modal_active, ev.surface_id) {
        return Some(ForwardedKeyOutcome::Gated);
    }
    if crate::app::plugin_glue::shortcut::plugin_shortcut_in_window(
        state,
        engine,
        plugin_manager,
        has_webview,
        &ev.key,
        ev.mods,
        crate::app::plugin_glue::shortcut::PluginKeyGate::Forwarded(ev.surface_id),
    ) {
        return Some(ForwardedKeyOutcome::Handled);
    }
    None
}

/// 한 창에서 native WebView 가 넘긴 키를 처리해도 되는지. 포커스 노드를 정규화한 뒤 창 전체 차단(`modal_active`(modal
/// 창), 전체화면 무대, 설정 열기 요청)과 받은 surface(`key_nodes::received_key_blocked`)를 본다.
fn forwarded_key_gate_open(
    state: &mut crate::state::MainViewState,
    engine: &crate::runtime::engine_access::EngineMut<'_>,
    modal_active: bool,
    received: u32,
) -> bool {
    state.normalize_focus(engine);
    // 창 전체 차단(modal·전체화면 무대·설정 열기 요청)을 먼저 보고, 그 뒤 받은 surface 를 본다.
    let blocked = state.has_settings_open_request()
        || crate::adapters::ui::input::key_nodes::received_key_blocked(
            state,
            &engine.read(),
            received,
        );
    forwarded_key_may_dispatch(modal_active, blocked, state.fullscreen_stage_active())
}

/// native WebView 가 넘긴 키를 plugin·새로고침·host 단축키로 처리해도 되는지. winit 경로가 막는
/// 상태(modal 창, keyboard overlay, 전체화면 무대)에서는 모두 처리하지 않는다.
fn forwarded_key_may_dispatch(modal_active: bool, overlay_open: bool, fullscreen: bool) -> bool {
    !modal_active && !overlay_open && !fullscreen
}

/// 공통 새로고침은 모델 포커스가 아니라 키를 받은 페이지를 대상으로 한다. 새로고침 키가 아니면 None이다.
fn dispatch_native_refresh_key(
    main: &mut crate::view::main::MainView,
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    ev: &crate::webview::WebViewKeyEvent,
) -> Option<ForwardedKeyOutcome> {
    let outcome = native_refresh_outcome(
        &mut main.state,
        &engine.read(),
        &engine.runtime.settings.keybindings.refresh,
        ev,
    )?;
    if outcome == ForwardedKeyOutcome::Handled {
        main.after_shortcut_consumed(&engine.read());
    }
    Some(outcome)
}

/// 새로고침 키를 키를 받은 페이지에 요청한다. 그 페이지를 대상으로 잡지 못하면 다른 surface로 보내지 않고
/// 처리하지 못한 키로 넘긴다.
fn native_refresh_outcome(
    state: &mut crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    refresh: &[String],
    ev: &crate::webview::WebViewKeyEvent,
) -> Option<ForwardedKeyOutcome> {
    if !tasty_key_match::matches_any_binding(refresh, &ev.key, ev.mods) {
        return None;
    }
    let requested = crate::adapters::ui::input::shortcuts::request_surface_refresh(
        state,
        engine,
        ev.surface_id,
        crate::intent::RefreshTrigger::Shortcut,
    );
    tracing::debug!(
        target: "tasty::refresh",
        "native webview refresh key for surface {}: {}",
        ev.surface_id,
        if requested { "requested" } else { "not handled: the page cannot be targeted" }
    );
    Some(ForwardedKeyOutcome::from_handled(requested))
}

#[cfg(all(test, target_os = "linux"))]
#[path = "webview_keys_state_tests.rs"]
mod state_tests;

#[cfg(test)]
mod tests {
    use super::webview_poll_needed;

    /// 소비 순서 시험의 대역. 첫 키(명령 팔레트 토글)는 포커스를 주는 팝업 열기를 대기열에 넣고, 대기열 적용이 그
    /// 팝업을 연다. 팝업이 열려 있으면 다음 키는 입력 게이트에 막힌다(`Gated`). 열기가 포커스를 주지 않으면
    /// (`focusing` 거짓, 알림 목록 토글) 게이트를 닫지 않는다.
    struct Consumer {
        focusing: bool,
        queued: bool,
        overlay_open: bool,
        sidebar_toggled: bool,
        log: Vec<String>,
        finished: Vec<(u64, bool)>,
    }

    impl Consumer {
        fn new(focusing: bool) -> Self {
            Self {
                focusing,
                queued: false,
                overlay_open: false,
                sidebar_toggled: false,
                log: Vec::new(),
                finished: Vec::new(),
            }
        }
    }

    impl super::ForwardedKeyConsumer for Consumer {
        fn dispatch(
            &mut self,
            _id: winit::window::WindowId,
            ev: crate::webview::WebViewKeyEvent,
        ) -> super::ForwardedKeyOutcome {
            self.log.push(format!("dispatch {}", ev.id));
            if self.overlay_open {
                return super::ForwardedKeyOutcome::Gated;
            }
            if ev.id == 1 {
                self.queued = true;
            } else {
                self.sidebar_toggled = true;
            }
            super::ForwardedKeyOutcome::Handled
        }

        fn apply_focus_opens(&mut self, _id: winit::window::WindowId) {
            self.log.push("apply".into());
            if self.queued && self.focusing {
                self.overlay_open = true;
            }
            self.queued = false;
        }

        fn finish(
            &mut self,
            _id: winit::window::WindowId,
            _surface_id: u32,
            key_id: u64,
            outcome: super::ForwardedKeyOutcome,
        ) {
            self.log.push(format!("finish {key_id}"));
            self.finished
                .push((key_id, outcome == super::ForwardedKeyOutcome::NotHandled));
        }
    }

    /// 대기열 대역: 전달 키 두 개만 든 한 창의 대기열을 내준다.
    impl super::NativeInputConsumer for Consumer {
        fn take_native_inputs(
            &mut self,
            _id: winit::window::WindowId,
        ) -> Vec<crate::webview::NativeInput> {
            use winit::keyboard::{Key, ModifiersState};
            let key = |id, ch: &str| {
                crate::webview::NativeInput::Forwarded(crate::webview::WebViewKeyEvent {
                    id,
                    surface_id: 7,
                    key: Key::Character(ch.into()),
                    mods: ModifiersState::CONTROL,
                })
            };
            vec![key(1, "P"), key(2, "b")]
        }

        fn native_press(
            &mut self,
            _id: winit::window::WindowId,
            _surface_id: u32,
            _kind: crate::webview::PressKind,
        ) {
            unreachable!("대기열에 누름이 없다");
        }
    }

    /// Given 한 창 대기열에 [명령 팔레트를 여는 키, sidebar 접기 키]가 들었을 때, When 소비하면, Then 키마다 처리 →
    /// 포커스 열기 적용 → 결과 알림 순서로 돌아, 둘째 키는 열린 팔레트의 게이트에 막히고(`Gated`) sidebar 가 바뀌지
    /// 않으며 페이지로 다시 넘기지 않는다.
    #[test]
    fn a_focused_popup_opened_by_a_forwarded_key_gates_the_next_key_of_the_batch() {
        let mut consumer = Consumer::new(true);
        super::consume_native_inputs(&mut consumer, winit::window::WindowId::from(1));
        assert_eq!(
            consumer.log,
            [
                "dispatch 1",
                "apply",
                "finish 1",
                "dispatch 2",
                "apply",
                "finish 2"
            ]
        );
        assert!(!consumer.sidebar_toggled, "둘째 키가 sidebar 를 바꿨다");
        assert_eq!(consumer.finished, [(1, false), (2, false)]);
    }

    /// 대조: 첫 키의 열기가 포커스를 주지 않으면(알림 목록 토글) 둘째 키는 그대로 처리된다.
    #[test]
    fn a_passive_open_by_a_forwarded_key_leaves_the_next_key_alone() {
        let mut consumer = Consumer::new(false);
        super::consume_native_inputs(&mut consumer, winit::window::WindowId::from(1));
        assert!(consumer.sidebar_toggled);
        assert_eq!(consumer.finished, [(1, false), (2, false)]);
    }

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

    /// modal 창·overlay·전체화면 무대 가운데 하나라도 열려 있으면 넘어온 키를 처리하지 않는다.
    #[test]
    fn forwarded_keys_wait_for_every_gate() {
        assert!(super::forwarded_key_may_dispatch(false, false, false));
        assert!(!super::forwarded_key_may_dispatch(true, false, false));
        assert!(!super::forwarded_key_may_dispatch(false, true, false));
        assert!(!super::forwarded_key_may_dispatch(false, false, true));
    }

    /// 단축키가 동작했으면 넘기지 않고, 동작하지 않았으면 엔진·페이지로 넘긴다.
    #[test]
    fn the_handler_result_decides_whether_the_key_is_passed_on() {
        use super::ForwardedKeyOutcome as O;
        assert_eq!(O::from_handled(true), O::Handled);
        assert_eq!(O::from_handled(false), O::NotHandled);
    }

    /// 키를 받은 페이지를 새로고침 대상으로 잡으면 처리한 키, 잡지 못하면 처리하지 못한 키다.
    /// 새로고침 바인딩이 아닌 키는 이 경로가 판정하지 않는다.
    #[test]
    fn a_refresh_key_for_a_page_that_cannot_be_targeted_is_not_handled() {
        use super::ForwardedKeyOutcome as O;
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        use winit::keyboard::{Key, ModifiersState};
        let events = vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "w".into(),
                category: 0,
                index: 0,
                pane: 1,
            },
            E::TabCreated {
                id: 11,
                pane: 1,
                index: 0,
                name: "markdown".into(),
                surface: SurfaceSpec {
                    id: 11,
                    kind: "markdown".into(),
                    data: None,
                },
            },
        ];
        let (mut state, mut session) =
            crate::state::tests::test_state_from_model(crate::state::tests::test_model(events));
        let engine = session.borrow_mut();
        let refresh = vec!["f5".to_string()];
        let key = |surface_id, key: &str| crate::webview::WebViewKeyEvent {
            id: 0,
            surface_id,
            key: Key::Named(match key {
                "F5" => winit::keyboard::NamedKey::F5,
                _ => winit::keyboard::NamedKey::F6,
            }),
            mods: ModifiersState::empty(),
        };
        let outcome =
            |state: &mut _, ev| super::native_refresh_outcome(state, &engine.read(), &refresh, &ev);
        assert_eq!(outcome(&mut state, key(11, "F5")), Some(O::Handled));
        assert_eq!(outcome(&mut state, key(99, "F5")), Some(O::NotHandled));
        assert_eq!(outcome(&mut state, key(11, "F6")), None);
    }
}
