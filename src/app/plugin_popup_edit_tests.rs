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
