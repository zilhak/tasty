//! native 입력 순서 경계(`webview_keys::consume_native_inputs`). Windows 메시지 훅·`AcceleratorKeyPressed` 는 판정
//! 순서(`route_key_down`)로 키를 받고, 누름 알림과 host 가 가져간 조합은 다리 대기열에 도착 순서대로 쌓인 뒤 회차
//! 끝에 소비된다. 이 대역은 그 순서로 사건을 넣고 회차 끝 소비를 제품 소비 함수로 돈다.

use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::tests::{strings, two_surfaces, unknown_physical};
use crate::app::webview_keys::{
    ForwardedKeyConsumer, ForwardedKeyOutcome, NativeInputConsumer, consume_native_inputs,
    press_and_publish,
};
use crate::host_api::webview::keys::{
    HostShortcutPolicy, KeyDownRoute, NativeInput, PressKind, ShortcutSources, WebViewEditAction,
    WebViewKeyBridge, WebViewKeyEvent, WebViewKeySink, route_key_down,
};

/// Alt+V 붙여넣기 바인딩과 host 조합 F2.
fn bridge(holder: u32) -> WebViewKeyBridge {
    let bridge = WebViewKeyBridge::new();
    bridge.set_policy(HostShortcutPolicy::new(ShortcutSources {
        host: strings(&["f2"]),
        edit: vec![(WebViewEditAction::Paste, strings(&["alt+v"]))],
        ..Default::default()
    }));
    bridge.set_key_holder(Some(holder));
    bridge
}

/// 회차 끝 소비의 대역. 누름 전이와 게시는 제품 함수이고, host 단축키 처리는 제품 게이트와 같은 비교(정규화 뒤
/// 받은 surface 가 키 쥘 곳인지, `key_nodes::received_key_blocked`)만 한다.
struct Turn<'a> {
    state: &'a mut crate::state::MainViewState,
    core: &'a crate::core::CoreState,
    bridge: &'a WebViewKeyBridge,
    log: Vec<String>,
    /// 첫 host 단축키를 처리하는 동안 native 콜백으로 들어오는 사건(소비 안에서 돈 GTK 펌프·창 정리 모사). 넣은 뒤
    /// 프레임 동기화의 선두 누름 소비와 같은 창 소비를 한 번씩 더 부른다.
    nested: Vec<Native>,
}

impl ForwardedKeyConsumer for Turn<'_> {
    fn dispatch(
        &mut self,
        _id: winit::window::WindowId,
        ev: WebViewKeyEvent,
    ) -> ForwardedKeyOutcome {
        let nested = std::mem::take(&mut self.nested);
        if !nested.is_empty() {
            for event in &nested {
                send(self.bridge, *event, &mut self.log);
            }
            if crate::app::webview_keys::consume_native_focus_requests(
                self.state,
                self.core,
                self.bridge,
            ) {
                self.log.push("sync took a press".into());
            }
            consume_native_inputs(self, winit::window::WindowId::from(1));
        }
        self.state.normalize_focus(self.core);
        let blocked = crate::adapters::ui::input::key_nodes::received_key_blocked(
            self.state,
            self.core,
            ev.surface_id,
        );
        let outcome = if blocked {
            ForwardedKeyOutcome::Gated
        } else {
            ForwardedKeyOutcome::Handled
        };
        self.log
            .push(format!("shortcut {} {outcome:?}", ev.surface_id));
        outcome
    }

    fn apply_focus_opens(&mut self, _id: winit::window::WindowId) {}

    fn finish(
        &mut self,
        _id: winit::window::WindowId,
        surface_id: u32,
        _key_id: u64,
        outcome: ForwardedKeyOutcome,
    ) {
        self.log.push(format!("forwarded {surface_id} {outcome:?}"));
    }
}

impl NativeInputConsumer for Turn<'_> {
    fn begin_native_inputs(&mut self, _id: winit::window::WindowId) -> bool {
        self.bridge.begin_consuming()
    }

    fn next_native_input(&mut self, _id: winit::window::WindowId) -> Option<NativeInput> {
        self.bridge.pop_input()
    }

    fn end_native_inputs(&mut self, _id: winit::window::WindowId) {
        self.bridge.end_consuming();
    }

    fn native_press(&mut self, _id: winit::window::WindowId, surface_id: u32, kind: PressKind) {
        press_and_publish(self.state, self.core, self.bridge, surface_id, kind);
    }
}

/// 한 회차의 사건.
#[derive(Clone, Copy)]
enum Native {
    Press(u32),
    /// F2
    Shortcut(u32),
    /// 그 surface 의 view 를 지웠다(`WebViewKeyBridge::retire_view`).
    Retire(u32),
}

/// `events` 를 한 회차에 넣고(키는 backend 판정 `route_key_down`, 그 자리의 편집 실행은 기록) 회차 끝에 제품 소비
/// 함수로 소비한다. 기록과 끝 포커스 surface 를 돌려준다.
fn turn(holder: u32, events: &[Native]) -> (Vec<String>, Option<u32>) {
    turn_with_nested(holder, events, &[])
}

/// 사건 하나를 backend 판정(`route_key_down`)으로 다리에 넣는다. 그 자리의 편집 실행과 판정 결과를 기록한다.
fn send(bridge: &WebViewKeyBridge, event: Native, log: &mut Vec<String>) {
    match event {
        Native::Press(sid) => bridge.note_press(sid),
        Native::Retire(sid) => bridge.retire_view(sid),
        Native::Shortcut(sid) => {
            let mut ran = None;
            let route = route_key_down(
                bridge,
                sid,
                Key::Named(NamedKey::F2),
                unknown_physical(),
                ModifiersState::empty(),
                false,
                |action| ran = Some(action),
            );
            if let Some(action) = ran {
                log.push(format!("edit now {sid} {action:?}"));
            }
            if let KeyDownRoute::Forwarded(_) = route {
                log.push(format!("held {sid}"));
            }
        }
    }
}

/// `turn` 과 같고, 첫 host 단축키 처리 중에 `nested` 사건이 native 콜백으로 들어온다.
fn turn_with_nested(
    holder: u32,
    events: &[Native],
    nested: &[Native],
) -> (Vec<String>, Option<u32>) {
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    if holder != 11 {
        state.focus_surface_by_id(engine.core, holder);
    }
    let bridge = bridge(holder);
    let mut log = Vec::new();
    for event in events {
        send(&bridge, *event, &mut log);
    }
    let mut consumer = Turn {
        state: &mut state,
        core: engine.core,
        bridge: &bridge,
        log,
        nested: nested.to_vec(),
    };
    consume_native_inputs(&mut consumer, winit::window::WindowId::from(1));
    let log = consumer.log;
    let focused = state.focused_surface_id(&engine.read());
    (log, focused)
}

/// 프레임 동기화는 대기열 앞 누름만 소비하고, 전달 키와 그 뒤 입력은 host 단축키 처리가 있는 소비에 남긴다
/// (`consume_native_focus_requests`).
#[test]
fn the_frame_sync_consumes_only_the_presses_before_a_forwarded_key() {
    let (mut state, mut session) = two_surfaces();
    let engine = session.borrow_mut();
    let bridge = bridge(11);
    bridge.note_press(12);
    assert!(
        bridge
            .forward_key(
                12,
                Key::Named(NamedKey::F2),
                unknown_physical(),
                ModifiersState::empty()
            )
            .is_some()
    );
    bridge.note_press(11);
    assert!(crate::app::webview_keys::consume_native_focus_requests(
        &mut state,
        engine.core,
        &bridge
    ));
    assert_eq!(state.focused_surface_id(&engine.read()), Some(12));
    let left = bridge.take_inputs();
    assert_eq!(left.len(), 2, "전달 키와 뒤 누름은 남는다: {left:?}");
    assert!(matches!(left[0], NativeInput::Forwarded(_)));
}

/// X4 앞 문단(누름 뒤에 온 단축키): Given A(11)가 키 쥘 곳이다, When 한 회차에 [B(12) 누름, B F2, A 누름, B F2]가
/// 들면, Then 첫 F2 는 B 누름 전이 뒤에 처리돼 동작하고(Handled), 둘째 F2 는 A 누름 뒤라 게이트에 막힌다(Gated).
/// 끝 포커스는 A 다. 두 F2 모두 페이지로 다시 보내지 않는다.
#[test]
fn a_shortcut_after_a_press_is_judged_after_that_press() {
    let (log, focused) = turn(
        11,
        &[
            Native::Press(12),
            Native::Shortcut(12),
            Native::Press(11),
            Native::Shortcut(12),
        ],
    );
    assert_eq!(
        log,
        [
            "held 12",
            "held 12",
            "shortcut 12 Handled",
            "forwarded 12 Handled",
            "shortcut 12 Gated",
            "forwarded 12 Gated",
        ]
    );
    assert_eq!(focused, Some(11));
}

/// X4 앞 문단(누름 앞에 온 단축키): Given B(12)가 키 쥘 곳이다, When 한 회차에 [B F2, A(11) 누름]이 들면, Then F2 는 도착 순서대로 A 누름보다 먼저 처리돼 B 에서 동작하고
/// (Handled) 끝 포커스는 A 다.
#[test]
fn a_shortcut_before_a_press_is_judged_before_that_press() {
    let (log, focused) = turn(12, &[Native::Shortcut(12), Native::Press(11)]);
    assert_eq!(
        log,
        ["held 12", "shortcut 12 Handled", "forwarded 12 Handled"]
    );
    assert_eq!(focused, Some(11));
}

/// X2: Given A(11)가 키 쥘 곳이다, When 한 회차에 [B(12) 누름, B F2]가 들고 회차 끝 소비가 F2 를 처리하는 동안 native
/// 콜백으로 [A 누름, A F2]가 들어와 프레임 동기화의 선두 누름 소비와 같은 창 소비가 다시 불리면, Then 둘 다 아무것도
/// 꺼내지 않는다. 바깥 소비가 B F2 를 B 에서 처리한 뒤 A 누름 → A F2 를 도착 순서대로 한 번씩 처리하고, 끝 포커스는
/// A 다.
#[test]
fn nested_gtk_input_stays_behind_the_active_native_queue() {
    let (log, focused) = turn_with_nested(
        11,
        &[Native::Press(12), Native::Shortcut(12)],
        &[Native::Press(11), Native::Shortcut(11)],
    );
    assert_eq!(
        log,
        [
            "held 12",
            "held 11",
            "shortcut 12 Handled",
            "forwarded 12 Handled",
            "shortcut 11 Handled",
            "forwarded 11 Handled",
        ],
        "중첩 입력이 바깥 소비에서 도착 순서대로 처리된다"
    );
    assert_eq!(focused, Some(11));
}

/// X4: Given A(11)가 키 쥘 곳이다, When B(12) view 의 [누름, F2]가 대기열에 있는 채 그 view 가 지워지고 같은 surface 의
/// 새 view 가 F2 를 내면, Then 회차 끝 소비는 옛 view 의 누름과 단축키를 하나도 처리하지 않고 새 view 의 F2 만 한 번
/// 처리한다(옛 누름이 없어 A 가 키 쥘 곳이라 막힌다). 대조: view 를 지우지 않으면 옛 누름 뒤 F2 가 B 에서 동작한다.
#[test]
fn retiring_a_webview_cancels_its_queued_inputs() {
    let old = [Native::Press(12), Native::Shortcut(12)];
    let mut events = old.to_vec();
    events.extend([Native::Retire(12), Native::Shortcut(12)]);
    let (log, focused) = turn(11, &events);
    assert_eq!(
        log,
        [
            "held 12",
            "held 12",
            "shortcut 12 Gated",
            "forwarded 12 Gated",
        ]
    );
    assert_eq!(focused, Some(11));
    let (log, focused) = turn(11, &old);
    assert_eq!(
        log,
        ["held 12", "shortcut 12 Handled", "forwarded 12 Handled"],
        "대조: 지우지 않은 view 의 항목은 처리된다"
    );
    assert_eq!(focused, Some(12));
}
