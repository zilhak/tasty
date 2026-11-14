//! 플러그인 팝업 편집 요청 시험. 회신을 약속한 플러그인의 열린 팝업에만 요청이 생긴다.

use std::sync::Arc;

use super::*;

const OPTED: &str = "com.example.opted";
const PLAIN: &str = "com.example.plain";

fn popup(plugin_id: &str) -> tasty_host_plugin::PopupInstance {
    use tasty_plugin_manifest::{PopupContribute, PopupRendering, PopupScopeDecl, PopupTrigger};
    tasty_host_plugin::PopupInstance {
        plugin_id: plugin_id.into(),
        popup_id: "file-open".into(),
        contribute: PopupContribute {
            id: "file-open".into(),
            trigger: PopupTrigger::Ipc,
            size_hint: None,
            anchor: crate::plugin::manifest::PopupAnchor::ScreenCenter,
            scope: PopupScopeDecl::Window,
            dismiss_on_outside_click: true,
            rendering: PopupRendering::EguiMesh,
        },
        z_seq: 1,
        scope_surface: None,
    }
}

fn mgr() -> PluginManager {
    let mut mgr = PluginManager::with_registries(
        Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
        Arc::new(crate::file::format::FileFormatRegistry::new()),
        Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    mgr.attach_edit_outcome_stub_for_test(OPTED);
    mgr.insert_popup_instance_for_test(1, popup(OPTED));
    mgr.insert_popup_instance_for_test(2, popup(PLAIN));
    mgr
}

/// 회신을 약속한 플러그인의 팝업은 그 팝업을 대상으로 요청을 받는다. 약속하지 않은 플러그인(번들 플러그인)의
/// 팝업과 닫힌 팝업은 요청이 없어 원래 키가 넘어간다.
#[test]
fn only_an_open_popup_of_an_opted_in_plugin_gets_an_edit_request() {
    let mut mgr = mgr();
    let request = popup_edit_request(&mut mgr, 1).expect("약속한 플러그인의 열린 팝업");
    assert_eq!(request.plugin_id, OPTED);
    assert_eq!(request.target, PluginCopyTarget::Popup(1));
    assert!(popup_edit_request(&mut mgr, 2).is_none());
    assert!(popup_edit_request(&mut mgr, 3).is_none());
}

fn ask(instance: u64) -> PluginEditAsk {
    PluginEditAsk {
        instance,
        edit: EditKey::Paste,
        text: Some("/tmp/a.md".into()),
        original: WindowEvent::Focused(true),
    }
}

/// 회신을 약속하지 않은 플러그인의 팝업, 닫힌 팝업, 플러그인 관리자가 없는 경우는 원래 키를 넘긴 편집으로 한 번
/// 재생한다. 번들 플러그인이 지금 모두 타는 경로다. 약속한 플러그인의 열린 팝업만 요청을 붙이고 보류한다.
#[test]
fn a_popup_without_a_reply_promise_replays_the_original_key_once() {
    let mut mgr = mgr();
    let replay = vec![ReplayStep::Original(
        WindowEvent::Focused(true),
        EditKey::Paste,
    )];
    for instance in [2, 3] {
        match popup_edit_plan(Some(&mut mgr), ask(instance)) {
            PopupEditPlan::Replay(steps) => assert_eq!(steps, replay, "팝업 {instance}"),
            other => panic!("팝업 {instance} 이 보류됐다: {other:?}"),
        }
    }
    match popup_edit_plan(None, ask(1)) {
        PopupEditPlan::Replay(steps) => assert_eq!(steps, replay),
        other => panic!("관리자 없이 보류됐다: {other:?}"),
    }
    match popup_edit_plan(Some(&mut mgr), ask(1)) {
        PopupEditPlan::Hold(hold) => {
            let PopupEditHold {
                instance,
                wire,
                request,
                original,
                edit,
            } = *hold;
            assert_eq!(instance, 1);
            assert_eq!(request.target, PluginCopyTarget::Popup(1));
            assert_eq!(
                wire,
                RawInputEventWire::EditRequest {
                    edit_id: request.edit_id,
                    edit: tasty_plugin_protocol::EditRequestWire::PasteText {
                        text: "/tmp/a.md".into()
                    },
                }
            );
            assert_eq!(
                (original, edit),
                (WindowEvent::Focused(true), EditKey::Paste)
            );
        }
        other => panic!("약속한 팝업이 재생됐다: {other:?}"),
    }
}

/// 보류 대상 판정: 열려 있고 그 창의 키를 받는 팝업만 살아 있다. 닫힌 팝업, 다른 팝업이 키를 받는 창, 플러그인
/// 관리자가 없는 경우는 끝난다. surface 대상은 아직 판정하지 않는다.
#[test]
fn a_hold_target_lives_only_while_its_popup_is_open_and_owns_the_window_keys() {
    use crate::app::plugin_edit_holds::hold_target_live;
    let mgr = mgr();
    let (w1, w2, w3) = (WindowId::from(1), WindowId::from(2), WindowId::from(3));
    let key_target_of = |window: WindowId| match u64::from(window) {
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        _ => None,
    };
    let live = hold_target_live(Some(&mgr), key_target_of);
    assert!(live(w1, &PluginCopyTarget::Popup(1)), "열림·소유");
    assert!(
        !live(w2, &PluginCopyTarget::Popup(1)),
        "다른 instance 가 키를 받는다"
    );
    assert!(!live(w3, &PluginCopyTarget::Popup(3)), "닫힌 팝업");
    assert!(
        !live(WindowId::from(9), &PluginCopyTarget::Popup(1)),
        "창의 키를 받는 plugin 팝업이 없다"
    );
    assert!(live(w1, &PluginCopyTarget::Surface(5)));
    let without = hold_target_live(None, key_target_of);
    assert!(
        !without(w1, &PluginCopyTarget::Popup(1)),
        "플러그인 관리자 없음"
    );
}

/// App 은 보류 창의 키 대상을 그 창 view 의 판정(`plugin_popup_key_target`)으로 읽어 위 판정에 넘기고, 그 판정을
/// 그대로 `settle`에 쓴다. 이 연결은 `MainView` 없이 시험할 수 없어 소스로 고정한다.
#[test]
fn settle_reads_the_window_key_target_into_the_hold_judgment() {
    let src = include_str!("plugin_edit_holds.rs");
    let body = src
        .split_once("pub(super) fn settle_plugin_edit_holds(")
        .and_then(|(_, rest)| rest.split_once("fn finish_plugin_edit("))
        .map(|(body, _)| body)
        .expect("settle_plugin_edit_holds 본문");
    let code: String = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .collect();
    let wiring = "let views = &self.view.views;\
        let live = hold_target_live(self.plugin_manager.as_ref(), |window| {\
        views\
        .get(&window)\
        .and_then(|v| v.as_main())\
        .and_then(|main| main.state.plugin_popup_key_target())\
        });\
        let (resolved, cancelled) = self.plugin_edit_holds.settle(outcomes, now, live);";
    let wiring: String = wiring.split_whitespace().collect::<Vec<_>>().join(" ");
    let code: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
    let squash = |s: &str| s.replace(' ', "");
    assert!(
        squash(&code).contains(&squash(&wiring)),
        "settle_plugin_edit_holds 가 창의 키 대상과 열린 팝업 판정을 settle 에 넘기지 않는다"
    );
}
